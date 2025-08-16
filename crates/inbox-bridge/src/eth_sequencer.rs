use crate::rpc::RpcClient;
use crate::traits::SequencerInbox;
use crate::types::SequencerInboxBatch;
use crate::selectors::{
    SIG_BATCH_COUNT, SIG_INBOX_ACCS, EVT_SEQUENCER_BATCH_DELIVERED, EVT_SEQUENCER_BATCH_DATA,
};
use alloy_primitives::{keccak256, Address, B256, U256};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;

#[derive(Clone, Deserialize)]
struct RpcLog {
    address: String,
    data: String,
    topics: Vec<String>,
    #[serde(default)]
    blockNumber: Option<String>,
    #[serde(default)]
    blockHash: Option<String>,
    #[serde(default)]
    transactionHash: Option<String>,
}

#[derive(Deserialize)]
struct RpcReceipt {
    #[serde(default)]
    blockNumber: Option<String>,
    #[serde(default)]
    blockHash: Option<String>,
    #[serde(default)]
    logs: Vec<RpcLog>,
}

#[derive(Deserialize)]
struct RpcTx {
    #[serde(default)]
    input: String,
    #[serde(default)]
    blobVersionedHashes: Option<Vec<String>>,
}

pub struct EthSequencerInbox {
    rpc: Arc<RpcClient>,
    inbox_addr: Address,
}

impl EthSequencerInbox {
    pub async fn new_http(rpc_url: &str, inbox_addr: Address) -> anyhow::Result<Self> {
        let rpc = Arc::new(RpcClient::new(rpc_url.to_string()));
        Ok(Self { rpc, inbox_addr })
    }

    fn encode_selector(sig: &str) -> [u8; 4] {
        let h = keccak256(sig.as_bytes());
        [h[0], h[1], h[2], h[3]]
    }

    fn encode_u256(v: U256) -> [u8; 32] {
        v.to_be_bytes::<32>()
    }

    fn decode_b256_word(word: &[u8]) -> anyhow::Result<B256> {
        if word.len() != 32 {
            anyhow::bail!("bad b256")
        }
        Ok(B256::from_slice(word))
    }

    fn decode_u256_word(word: &[u8]) -> anyhow::Result<U256> {
        if word.len() != 32 {
            anyhow::bail!("bad u256")
        }
        Ok(U256::from_be_bytes(<[u8; 32]>::try_from(word)?))
    }
}

#[async_trait]
impl SequencerInbox for EthSequencerInbox {
    async fn get_batch_count(&self, block_number: u64) -> anyhow::Result<u64> {
        info!("eth_sequencer: get_batch_count at block {}", block_number);
        let mut data = Vec::with_capacity(4);
        data.extend_from_slice(&Self::encode_selector(SIG_BATCH_COUNT));
        let to_hex = format!("{:#x}", self.inbox_addr);
        let block_tag = format!("0x{:x}", block_number);
        let res_hex: String = self.rpc.call("eth_call", json!([{
            "to": to_hex.clone(),
            "data": format!("0x{}", hex::encode(&data)),
        }, block_tag])).await?;
        let mut res = hex::decode(res_hex.trim_start_matches("0x"))?;
        if res.len() < 32 {
            anyhow::bail!("short returndata for batchCount")
        }
        let count = Self::decode_u256_word(&res[0..32])?;
        Ok(count.try_into().map_err(|_| anyhow::anyhow!("count overflow"))?)
    }

    async fn get_accumulator(&self, seq_num: u64, block_number: u64) -> anyhow::Result<B256> {
        info!("eth_sequencer: get_accumulator seq={} block={}", seq_num, block_number);
        let mut data = Vec::with_capacity(4 + 32);
        data.extend_from_slice(&Self::encode_selector(SIG_INBOX_ACCS));
        data.extend_from_slice(&Self::encode_u256(U256::from(seq_num)));
        let to_hex = format!("{:#x}", self.inbox_addr);
        let block_tag = format!("0x{:x}", block_number);
        let res_hex: String = self.rpc.call("eth_call", json!([{
            "to": to_hex.clone(),
            "data": format!("0x{}", hex::encode(&data)),
        }, block_tag])).await?;
        let mut res = hex::decode(res_hex.trim_start_matches("0x"))?;
        if res.len() < 32 {
            anyhow::bail!("short returndata for inboxAccs")
        }
        Ok(Self::decode_b256_word(&res[0..32])?)
    }

    async fn lookup_batches_in_range(&self, from_block: u64, to_block: u64) -> anyhow::Result<Vec<SequencerInboxBatch>> {
        let topic0: B256 = keccak256(EVT_SEQUENCER_BATCH_DELIVERED.as_bytes());
        let addr_hex = format!("{:#x}", self.inbox_addr);
        let topic_hex = format!("{:#x}", topic0);
        let filter = json!({
            "fromBlock": format!("0x{:x}", from_block),
            "toBlock": format!("0x{:x}", to_block),
            "address": addr_hex,
            "topics": [[topic_hex]],
        });
        info!("eth_sequencer: lookup_batches_in_range from={} to={} addr={} topic0={}", from_block, to_block, format!("{:#x}", self.inbox_addr), format!("{:#x}", topic0));
        let logs: Vec<RpcLog> = self.rpc.call("eth_getLogs", json!([filter])).await?;
        info!("eth_sequencer: raw SequencerBatchDelivered logs fetched: {}", logs.len());

        let mut out = Vec::with_capacity(logs.len());

        for lg in logs {
            let data_bytes = hex::decode(lg.data.trim_start_matches("0x"))?;
            let topics_len = lg.topics.len();
            if topics_len < 3 || data_bytes.len() < 32 * 7 {
                continue;
            }

            let seq = U256::from_be_bytes(B256::from_str(&lg.topics[1]).unwrap_or_default().0).to::<u64>();
            let before_acc = B256::from_str(&lg.topics[2]).unwrap_or_default();

            let (after_acc, delayed_acc, after_delayed_count, min_ts, max_ts, min_bn, max_bn, data_loc_u256) = if topics_len >= 4 {
                let after_acc = B256::from_str(&lg.topics[3]).unwrap_or_default();
                let delayed_acc = Self::decode_b256_word(&data_bytes[0..32])?;
                let after_delayed_count = Self::decode_u256_word(&data_bytes[32..64])?.to::<u64>();
                let min_ts = Self::decode_u256_word(&data_bytes[64..96])?.to::<u64>();
                let max_ts = Self::decode_u256_word(&data_bytes[96..128])?.to::<u64>();
                let min_bn = Self::decode_u256_word(&data_bytes[128..160])?.to::<u64>();
                let max_bn = Self::decode_u256_word(&data_bytes[160..192])?.to::<u64>();
                let data_loc_u256 = Self::decode_u256_word(&data_bytes[192..224])?;
                (after_acc, delayed_acc, after_delayed_count, min_ts, max_ts, min_bn, max_bn, data_loc_u256)
            } else {
                let after_acc = Self::decode_b256_word(&data_bytes[0..32])?;
                let delayed_acc = Self::decode_b256_word(&data_bytes[32..64])?;
                let after_delayed_count = Self::decode_u256_word(&data_bytes[64..96])?.to::<u64>();
                let min_ts = Self::decode_u256_word(&data_bytes[96..128])?.to::<u64>();
                let max_ts = Self::decode_u256_word(&data_bytes[128..160])?.to::<u64>();
                let min_bn = Self::decode_u256_word(&data_bytes[160..192])?.to::<u64>();
                let max_bn = Self::decode_u256_word(&data_bytes[192..224])?.to::<u64>();
                let data_loc_u256 = Self::decode_u256_word(&data_bytes[224..256])?;
                (after_acc, delayed_acc, after_delayed_count, min_ts, max_ts, min_bn, max_bn, data_loc_u256)
            };

            let time_bounds = crate::types::TimeBounds {
                min_timestamp: min_ts,
                max_timestamp: max_ts,
                min_block_number: min_bn,
                max_block_number: max_bn,
            };
            let data_location: u8 = (data_loc_u256 & U256::from(0xff)).to::<u8>();

            let batch = SequencerInboxBatch {
                sequence_number: seq,
                before_inbox_acc: before_acc,
                after_inbox_acc: after_acc,
                after_message_count: 0,
                after_delayed_count,
                after_delayed_acc: delayed_acc,
                time_bounds,
                data_location,
                bridge_address: Address::from_str(&lg.address).unwrap_or_default(),
                parent_chain_block_number: lg.blockNumber.as_deref().and_then(|h| u64::from_str_radix(h.trim_start_matches("0x"), 16).ok()).unwrap_or_default(),
                block_hash: lg.blockHash.as_deref().and_then(|h| B256::from_str(h).ok()).unwrap_or_default(),
                tx_hash: lg.transactionHash.as_deref().and_then(|h| B256::from_str(h).ok()).unwrap_or_default(),
                serialized: Vec::new(),
            };
            out.push(batch);
        }
        info!("eth_sequencer: got {} SequencerBatchDelivered logs", out.len());
        Ok(out)
    }

    async fn get_sequencer_message_bytes_in_block(
        &self,
        block_number: u64,
        seq_num: u64,
        tx_hash: B256,
        block_hash: B256,
    ) -> anyhow::Result<(Vec<u8>, B256, Vec<u64>)> {
        let topic0: B256 = keccak256(EVT_SEQUENCER_BATCH_DATA.as_bytes());
        let mut topic1_bytes = [0u8; 32];
        topic1_bytes[24..32].copy_from_slice(&seq_num.to_be_bytes());
        let topic1 = B256::from_slice(&topic1_bytes);
        let addr_hex = format!("{:#x}", self.inbox_addr);
        let t0_hex = format!("{:#x}", topic0);
        let t1_hex = format!("{:#x}", topic1);
        let filter = if block_hash != B256::ZERO {
            json!({
                "blockHash": format!("{:#x}", block_hash),
                "address": addr_hex,
                "topics": [[t0_hex], [t1_hex]],
            })
        } else {
            json!({
                "fromBlock": format!("0x{:x}", block_number),
                "toBlock": format!("0x{:x}", block_number),
                "address": addr_hex,
                "topics": [[t0_hex], [t1_hex]],
            })
        };
        info!("eth_sequencer: get_sequencer_message_bytes_in_block block={} hash={} addr={} topic0={} topic1={}", block_number, format!("{:#x}", block_hash), format!("{:#x}", self.inbox_addr), format!("{:#x}", topic0), format!("{:#x}", topic1));
        let logs: Vec<RpcLog> = self.rpc.call("eth_getLogs", json!([filter])).await?;
        let mut chosen: Option<RpcLog> = None;
        if logs.len() == 1 {
            chosen = Some(logs[0].clone());
        } else if logs.len() > 1 && tx_hash != B256::ZERO {
            let wanted = format!("{:#x}", tx_hash);
            for l in logs {
                if let Some(th) = &l.transactionHash {
                    if th.eq_ignore_ascii_case(&wanted) {
                        chosen = Some(l);
                        break;
                    }
                }
            }
        }
        let lg = if chosen.is_none() {
            if tx_hash == B256::ZERO {
                anyhow::bail!("no unambiguous SequencerBatchData log for seq {} at block {} and no tx_hash provided", seq_num, block_number);
            }
            let tx_hex = format!("{:#x}", tx_hash);
            let receipt: RpcReceipt = self.rpc.call("eth_getTransactionReceipt", json!([tx_hex])).await?;
            let t0_hex = format!("{:#x}", topic0);
            let t1_hex = format!("{:#x}", topic1);
            let mut found: Option<RpcLog> = None;
            for l in receipt.logs {
                let ok_addr = l.address.eq_ignore_ascii_case(&format!("{:#x}", self.inbox_addr));
                let ok_topics = l.topics.len() >= 2 && l.topics[0].eq_ignore_ascii_case(&t0_hex) && l.topics[1].eq_ignore_ascii_case(&t1_hex);
                if ok_addr && ok_topics {
                    found = Some(l);
                    break;
                }
            }
            found.ok_or_else(|| anyhow::anyhow!("no SequencerBatchData in receipt for seq {} (tx {})", seq_num, tx_hex))?
        } else {
            chosen.unwrap()
        };
        let data_hex = &lg.data;
        let data = hex::decode(data_hex.trim_start_matches("0x"))?;
        if data.len() < 64 {
            anyhow::bail!("invalid SequencerBatchData encoding");
        }
        let mut len_bytes = [0u8; 32];
        len_bytes.copy_from_slice(&data[32..64]);
        let len = U256::from_be_bytes(len_bytes).to::<usize>();
        if data.len() < 64 + len {
            anyhow::bail!("short SequencerBatchData payload");
        }
        let batch_bytes = data[64..64 + len].to_vec();
        let block_hash = lg
            .blockHash
            .as_deref()
            .and_then(|h| B256::from_str(h).ok())
            .unwrap_or_default();
        Ok((batch_bytes, block_hash, Vec::new()))
    }
    async fn get_tx_input_and_blobs(&self, tx_hash: B256) -> anyhow::Result<(Vec<u8>, Vec<B256>)> {
        let tx_hex = format!("{:#x}", tx_hash);
        let tx: RpcTx = self.rpc.call("eth_getTransactionByHash", json!([tx_hex])).await?;
        let input = hex::decode(tx.input.trim_start_matches("0x"))?;
        let mut blobs: Vec<B256> = Vec::new();
        if let Some(hashes) = tx.blobVersionedHashes {
            for h in hashes {
                if let Ok(b) = B256::from_str(&h) {
                    blobs.push(b);
                }
            }
        }
        Ok((input, blobs))
    }
}
