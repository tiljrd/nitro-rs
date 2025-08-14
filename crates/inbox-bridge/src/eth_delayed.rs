use crate::traits::DelayedBridge;
use crate::types::DelayedInboxMessage;
use alloy_primitives::{keccak256, Address, B256, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::{BlockNumberOrTag, Filter};
use async_trait::async_trait;
use std::sync::Arc;

pub struct EthDelayedBridge {
    provider: Arc<dyn Provider>,
    bridge_addr: Address,
}

impl EthDelayedBridge {
    pub async fn new_http(rpc_url: &str, bridge_addr: Address) -> anyhow::Result<Self> {
        let provider = Arc::new(ProviderBuilder::default().connect(rpc_url).await?);
        Ok(Self { provider, bridge_addr })
    }

    fn encode_selector(sig: &str) -> [u8; 4] {
        let h = keccak256(sig.as_bytes());
        [h[0], h[1], h[2], h[3]]
    }

    fn encode_u256(v: U256) -> [u8; 32] {
        v.to_be_bytes()
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
        let to = self.bridge_addr;
        let res = self
            .provider
            .call_raw(to, data.into(), Some(BlockNumberOrTag::Number(block_number.into())))
            .await?;
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
        let to = self.bridge_addr;
        let res = self
            .provider
            .call_raw(to, data.into(), Some(BlockNumberOrTag::Number(block_number.into())))
            .await?;
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
        let topic0 = B256::from_slice(&keccak256("MessageDelivered(address,address,uint8,uint256,bytes32,bytes32,uint64,bytes32,uint64)".as_bytes()));
        let filter = Filter {
            from_block: Some(from_block.into()),
            to_block: Some(to_block.into()),
            address: Some(vec![self.bridge_addr]),
            topics: Some(vec![vec![topic0]]),
            block_hash: None,
        };
        let logs = self.provider.get_logs(&filter).await?;
        let mut out = Vec::with_capacity(logs.len());
        for lg in logs {
            if lg.data.len() < 32 * 7 {
                continue;
            }
            let before_acc = B256::from_slice(&lg.data[0..32]);
            let kind = u8::try_from(Self::decode_u256_word(&lg.data[32..64])?.to::<u64>()).unwrap_or(0);
            let timestamp = Self::decode_u256_word(&lg.data[64..96])?.to::<u64>();
            let sender = Address::from_slice(&lg.data[96 + 12..96 + 32]);
            let request_id = B256::from_slice(&lg.data[128..160]);
            let basefee = Self::decode_u256_word(&lg.data[160..192])?;
            let header = nitro_primitives::l1::L1IncomingMessageHeader {
                kind,
                poster: sender,
                block_number: lg.block_number.unwrap_or_default(),
                timestamp,
                request_id: Some(request_id),
                l1_base_fee: basefee,
            };
            let msg = nitro_primitives::l1::L1IncomingMessage { header, l2msg: Vec::new(), batch_gas_cost: None };
            out.push(DelayedInboxMessage {
                seq_num: U256::from_be_bytes(request_id.0).to::<u64>(),
                block_hash: lg.block_hash.unwrap_or_default(),
                before_inbox_acc: before_acc,
                message: msg,
                parent_chain_block_number: lg.block_number.unwrap_or_default(),
            });
        }
        out.sort_by_key(|m| U256::from_be_bytes(m.message.header.request_id.unwrap().0));
        Ok(out)
    }
}
