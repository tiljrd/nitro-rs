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
use crate::util::safe_from_for_proxy;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;

#[derive(Deserialize)]
struct RpcLog {
    address: String,
    data: String,
    topics: Vec<String>,
    #[serde(default)]
    blockNumber: Option<String>,
    #[serde(default)]
    blockHash: Option<String>,
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
        let from_hex = safe_from_for_proxy(&self.rpc, self.inbox_addr).await?;
        let res_hex: String = self.rpc.call("eth_call", json!([{
            "to": to_hex.clone(),
            "from": from_hex,
            "data": format!("0x{}", hex::encode(&data)),
        }, "latest"])).await?;
        let mut res = hex::decode(res_hex.trim_start_matches("0x"))?;
        if res.len() < 32 {
            let impl_addr = crate::util::proxy_impl_address(&self.rpc, self.inbox_addr).await?;
            let impl_hex = format!("{:#x}", impl_addr);
            let res2_hex: String = self.rpc.call("eth_call", json!([{
                "to": impl_hex,
                "data": format!("0x{}", hex::encode(&data)),
            }, "latest"])).await?;
            res = hex::decode(res2_hex.trim_start_matches("0x"))?;
            if res.len() < 32 {
                anyhow::bail!("short returndata for batchCount")
            }
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
        let from_hex = safe_from_for_proxy(&self.rpc, self.inbox_addr).await?;
        let res_hex: String = self.rpc.call("eth_call", json!([{
            "to": to_hex.clone(),
            "from": from_hex,
            "data": format!("0x{}", hex::encode(&data)),
        }, "latest"])).await?;
        let mut res = hex::decode(res_hex.trim_start_matches("0x"))?;
        if res.len() < 32 {
            let impl_addr = crate::util::proxy_impl_address(&self.rpc, self.inbox_addr).await?;
            let impl_hex = format!("{:#x}", impl_addr);
            let res2_hex: String = self.rpc.call("eth_call", json!([{
                "to": impl_hex,
                "data": format!("0x{}", hex::encode(&data)),
            }, "latest"])).await?;
            res = hex::decode(res2_hex.trim_start_matches("0x"))?;
            if res.len() < 32 {
                anyhow::bail!("short returndata for inboxAccs")
            }
        }
        Ok(Self::decode_b256_word(&res[0..32])?)
    }

    async fn lookup_batches_in_range(&self, from_block: u64, to_block: u64) -> anyhow::Result<Vec<SequencerInboxBatch>> {
        let topic0: B256 = keccak256(EVT_SEQUENCER_BATCH_DELIVERED.as_bytes());
        let filter = json!({
            "fromBlock": format!("0x{:x}", from_block),
            "toBlock": format!("0x{:x}", to_block),
            "address": format!("{:#x}", self.inbox_addr),
            "topics": [[format!("{:#x}", topic0)]],
        });
        info!("eth_sequencer: lookup_batches_in_range from={} to={}", from_block, to_block);
        let logs: Vec<RpcLog> = self.rpc.call("eth_getLogs", json!([filter])).await?;
        info!("eth_sequencer: raw SequencerBatchDelivered logs fetched: {}", logs.len());
        
        let mut out = Vec::with_capacity(logs.len());
        
        let mut last_seq: Option<u64> = None;
        for lg in logs {
            let data_bytes = hex::decode(lg.data.trim_start_matches("0x"))?;
            if lg.topics.len() < 3 || data_bytes.len() < 32 * 7 {
                continue;
            }
            let seq = U256::from_be_bytes(B256::from_str(&lg.topics[1]).unwrap_or_default().0).to::<u64>();
            if let Some(prev) = last_seq {
                if seq != prev + 1 {
                    anyhow::bail!("batches out of order: after {} got {}", prev, seq)
                }
            }
            last_seq = Some(seq);

            let before_acc = B256::from_str(&lg.topics[2]).unwrap_or_default();

            let after_acc = Self::decode_b256_word(&data_bytes[0..32])?;
            let delayed_acc = Self::decode_b256_word(&data_bytes[32..64])?;
            let after_delayed_count = Self::decode_u256_word(&data_bytes[64..96])?.to::<u64>();

            let min_ts = Self::decode_u256_word(&data_bytes[96..128])?.to::<u64>();
            let max_ts = Self::decode_u256_word(&data_bytes[128..160])?.to::<u64>();
            let min_bn = Self::decode_u256_word(&data_bytes[160..192])?.to::<u64>();
            let max_bn = Self::decode_u256_word(&data_bytes[192..224])?.to::<u64>();
            let time_bounds = crate::types::TimeBounds {
                min_timestamp: min_ts,
                max_timestamp: max_ts,
                min_block_number: min_bn,
                max_block_number: max_bn,
            };

            let data_loc_u256 = Self::decode_u256_word(&data_bytes[224..256])?;
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
    ) -> anyhow::Result<(Vec<u8>, B256, Vec<u64>)> {
        let topic0: B256 = keccak256(EVT_SEQUENCER_BATCH_DATA.as_bytes());
        let mut topic1_bytes = [0u8; 32];
        topic1_bytes[24..32].copy_from_slice(&seq_num.to_be_bytes());
        let topic1 = B256::from_slice(&topic1_bytes);
        let filter = json!({
            "fromBlock": format!("0x{:x}", block_number),
            "toBlock": format!("0x{:x}", block_number),
            "address": format!("{:#x}", self.inbox_addr),
            "topics": [[format!("{:#x}", topic0)], [format!("{:#x}", topic1)]],
        });
        let logs: Vec<RpcLog> = self.rpc.call("eth_getLogs", json!([filter])).await?;
        if logs.len() != 1 {
            anyhow::bail!("expected exactly 1 SequencerBatchData log for seq {} at block {}", seq_num, block_number);
        }
        let lg = &logs[0];
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
}
