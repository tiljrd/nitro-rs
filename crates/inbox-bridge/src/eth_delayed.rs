use crate::rpc::RpcClient;
use crate::traits::DelayedBridge;
use crate::types::DelayedInboxMessage;
use alloy_primitives::{keccak256, Address, B256, U256};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::str::FromStr;
use std::sync::Arc;

#[derive(Deserialize)]
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
struct RpcTx {
    input: String,
}

pub struct EthDelayedBridge {
    rpc: Arc<RpcClient>,
    bridge_addr: Address,
}

impl EthDelayedBridge {
    pub async fn new_http(rpc_url: &str, bridge_addr: Address) -> anyhow::Result<Self> {
        let rpc = Arc::new(RpcClient::new(rpc_url.to_string()));
        Ok(Self { rpc, bridge_addr })
    }

    fn encode_selector(sig: &str) -> [u8; 4] {
        let h = keccak256(sig.as_bytes());
        [h[0], h[1], h[2], h[3]]
    }

    fn encode_u256(v: U256) -> [u8; 32] {
        v.to_be_bytes::<32>()
    }

    fn decode_u256_word(word: &[u8]) -> anyhow::Result<U256> {
        if word.len() != 32 {
            anyhow::bail!("bad u256 word len")
        }
        Ok(U256::from_be_bytes(<[u8; 32]>::try_from(word)?))
    }

    fn decode_b256_word(word: &[u8]) -> anyhow::Result<B256> {
        if word.len() != 32 {
            anyhow::bail!("bad b256 word len")
        }
        Ok(B256::from_slice(word))
    }
}

#[async_trait]
impl DelayedBridge for EthDelayedBridge {
    async fn get_message_count(&self, block_number: u64) -> anyhow::Result<u64> {
        let mut data = Vec::with_capacity(4);
        data.extend_from_slice(&Self::encode_selector("delayedMessageCount()"));
        let to_hex = format!("{:#x}", self.bridge_addr);
        let block_tag = format!("0x{:x}", block_number);
        let res_hex: String = self.rpc.call("eth_call", json!([{
            "to": to_hex,
            "data": format!("0x{}", hex::encode(data)),
        }, block_tag])).await?;
        let res = hex::decode(res_hex.trim_start_matches("0x"))?;
        if res.len() < 32 {
            anyhow::bail!("short returndata for delayedMessageCount")
        }
        let count = Self::decode_u256_word(&res[0..32])?;
        Ok(count.try_into().map_err(|_| anyhow::anyhow!("count overflow"))?)
    }

    async fn get_accumulator(&self, seq_num: u64, block_number: u64, _block_hash: B256) -> anyhow::Result<B256> {
        let mut data = Vec::with_capacity(4 + 32);
        data.extend_from_slice(&Self::encode_selector("delayedInboxAccs(uint256)"));
        data.extend_from_slice(&Self::encode_u256(U256::from(seq_num)));
        let to_hex = format!("{:#x}", self.bridge_addr);
        let block_tag = format!("0x{:x}", block_number);
        let res_hex: String = self.rpc.call("eth_call", json!([{
            "to": to_hex,
            "data": format!("0x{}", hex::encode(data)),
        }, block_tag])).await?;
        let res = hex::decode(res_hex.trim_start_matches("0x"))?;
        if res.len() < 32 {
            anyhow::bail!("short returndata for delayedInboxAccs")
        }
        Ok(Self::decode_b256_word(&res[0..32])?)
    }

    async fn lookup_messages_in_range<F>(
        &self,
        from_block: u64,
        to_block: u64,
        _batch_fetcher: F,
    ) -> anyhow::Result<Vec<DelayedInboxMessage>>
    where
        F: Fn(u64) -> anyhow::Result<Vec<u8>> + Send + Sync,
    {
        let message_delivered_topic: B256 =
            keccak256("MessageDelivered(address,address,uint8,uint256,bytes32,bytes32,uint64,bytes32,uint64)".as_bytes());
        let filter = json!({
            "fromBlock": format!("0x{:x}", from_block),
            "toBlock": format!("0x{:x}", to_block),
            "address": format!("{:#x}", self.bridge_addr),
            "topics": [[format!("{:#x}", message_delivered_topic)]],
        });
        let logs: Vec<RpcLog> = self.rpc.call("eth_getLogs", json!([filter])).await?;

        let mut inbox_addresses: BTreeSet<Address> = BTreeSet::new();
        let mut message_ids: Vec<B256> = Vec::with_capacity(logs.len());
        let mut parsed: Vec<(DelayedInboxMessage, Address, B256)> = Vec::with_capacity(logs.len());

        for lg in logs {
            let data_bytes = hex::decode(lg.data.trim_start_matches("0x"))?;
            if data_bytes.len() < 32 * 7 {
                continue;
            }
            let before_acc = B256::from_slice(&data_bytes[0..32]);
            let kind = u8::try_from(Self::decode_u256_word(&data_bytes[32..64])?.to::<u64>()).unwrap_or(0);
            let timestamp = Self::decode_u256_word(&data_bytes[64..96])?.to::<u64>();
            let sender = Address::from_slice(&data_bytes[96 + 12..96 + 32]);
            let request_id = B256::from_slice(&data_bytes[128..160]);
            let basefee = Self::decode_u256_word(&data_bytes[160..192])?;

            let inbox_addr = if lg.topics.len() > 1 {
                let t1 = &lg.topics[1];
                let t1_bytes = hex::decode(t1.trim_start_matches("0x"))?;
                Address::from_slice(&t1_bytes[12..32])
            } else {
                Address::ZERO
            };

            let block_number = lg.blockNumber.as_deref().and_then(|h| u64::from_str_radix(h.trim_start_matches("0x"), 16).ok()).unwrap_or_default();
            let block_hash = lg.blockHash.as_deref().and_then(|h| B256::from_str(h).ok()).unwrap_or_default();

            let header = nitro_primitives::l1::L1IncomingMessageHeader {
                kind,
                poster: sender,
                block_number,
                timestamp,
                request_id: Some(request_id),
                l1_base_fee: basefee,
            };
            let msg = nitro_primitives::l1::L1IncomingMessage { header, l2msg: Vec::new(), batch_gas_cost: None };
            let dim = DelayedInboxMessage {
                seq_num: U256::from_be_bytes(request_id.0).to::<u64>(),
                block_hash,
                before_inbox_acc: before_acc,
                message: msg,
                parent_chain_block_number: block_number,
            };
            inbox_addresses.insert(inbox_addr);
            message_ids.push(request_id);
            parsed.push((dim, inbox_addr, request_id));
        }

        if parsed.is_empty() {
            return Ok(Vec::new());
        }

        let inbox_msg_delivered: B256 = keccak256("InboxMessageDelivered(uint256,bytes)".as_bytes());
        let inbox_msg_from_origin: B256 = keccak256("InboxMessageDeliveredFromOrigin(uint256)".as_bytes());

        let mut data_by_id: HashMap<B256, Vec<u8>> = HashMap::with_capacity(message_ids.len());
        if !inbox_addresses.is_empty() {
            let addresses = inbox_addresses.into_iter().map(|a| format!("{:#x}", a)).collect::<Vec<_>>();
            let topics = json!([
                [format!("{:#x}", inbox_msg_delivered), format!("{:#x}", inbox_msg_from_origin)],
                message_ids.iter().map(|id| format!("{:#x}", id)).collect::<Vec<_>>()
            ]);
            let q = json!({
                "fromBlock": format!("0x{:x}", from_block),
                "toBlock": format!("0x{:x}", to_block),
                "address": addresses,
                "topics": topics
            });
            let ilogs: Vec<RpcLog> = self.rpc.call("eth_getLogs", json!([q])).await?;
            for lg in ilogs {
                if lg.topics.len() < 2 {
                    continue;
                }
                let topic0 = B256::from_str(&lg.topics[0]).unwrap_or_default();
                let msg_id = B256::from_str(&lg.topics[1]).unwrap_or_default();
                let data_bytes = hex::decode(lg.data.trim_start_matches("0x"))?;
                if topic0 == inbox_msg_delivered {
                    if data_bytes.len() >= 64 {
                        let mut len_bytes = [0u8; 32];
                        len_bytes.copy_from_slice(&data_bytes[32..64]);
                        let len = U256::from_be_bytes(len_bytes).to::<usize>();
                        if data_bytes.len() >= 64 + len {
                            let bytes = data_bytes[64..64 + len].to_vec();
                            data_by_id.insert(msg_id, bytes);
                        }
                    }
                } else if topic0 == inbox_msg_from_origin {
                    if let Some(tx_hash) = lg.transactionHash {
                        let tx: RpcTx = self.rpc.call("eth_getTransactionByHash", json!([tx_hash])).await?;
                        let input = hex::decode(tx.input.trim_start_matches("0x"))?;
                        if input.len() >= 4 + 32 {
                            let selector = &input[0..4];
                            let expected = Self::encode_selector("sendL2MessageFromOrigin(bytes)");
                            if selector == expected {
                                if input.len() >= 4 + 64 {
                                    let mut len_bytes = [0u8; 32];
                                    len_bytes.copy_from_slice(&input[4 + 32..4 + 64]);
                                    let len = U256::from_be_bytes(len_bytes).to::<usize>();
                                    if input.len() >= 4 + 64 + len {
                                        let bytes = input[4 + 64..4 + 64 + len].to_vec();
                                        data_by_id.insert(msg_id, bytes);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut out: Vec<DelayedInboxMessage> = Vec::with_capacity(parsed.len());
        for (mut dim, _inbox, req_id) in parsed {
            if let Some(data) = data_by_id.get(&req_id) {
                dim.message.l2msg = data.clone();
            }
            out.push(dim);
        }

        out.sort_by_key(|m| U256::from_be_bytes(m.message.header.request_id.unwrap().0));
        Ok(out)
    }
}
