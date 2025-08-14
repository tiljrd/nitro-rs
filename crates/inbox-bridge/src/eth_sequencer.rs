use crate::traits::SequencerInbox;
use crate::types::SequencerInboxBatch;
use alloy_primitives::{keccak256, Address, B256, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::{BlockNumberOrTag, Filter};
use async_trait::async_trait;
use std::sync::Arc;

pub struct EthSequencerInbox {
    provider: Arc<dyn Provider>,
    inbox_addr: Address,
}

impl EthSequencerInbox {
    pub async fn new_http(rpc_url: &str, inbox_addr: Address) -> anyhow::Result<Self> {
        let provider = Arc::new(ProviderBuilder::default().connect(rpc_url).await?);
        Ok(Self { provider, inbox_addr })
    }

    fn encode_selector(sig: &str) -> [u8; 4] {
        let h = keccak256(sig.as_bytes());
        [h[0], h[1], h[2], h[3]]
    }

    fn encode_u256(v: U256) -> [u8; 32] {
        v.to_be_bytes()
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
        let mut data = Vec::with_capacity(4);
        data.extend_from_slice(&Self::encode_selector("batchCount()"));
        let to = self.inbox_addr;
        let res = self
            .provider
            .call_raw(to, data.into(), Some(BlockNumberOrTag::Number(block_number.into())))
            .await?;
        if res.len() < 32 {
            anyhow::bail!("short returndata for batchCount")
        }
        let count = Self::decode_u256_word(&res[0..32])?;
        Ok(count.try_into().map_err(|_| anyhow::anyhow!("count overflow"))?)
    }

    async fn get_accumulator(&self, seq_num: u64, block_number: u64) -> anyhow::Result<B256> {
        let mut data = Vec::with_capacity(4 + 32);
        data.extend_from_slice(&Self::encode_selector("inboxAccs(uint256)"));
        data.extend_from_slice(&Self::encode_u256(U256::from(seq_num)));
        let to = self.inbox_addr;
        let res = self
            .provider
            .call_raw(to, data.into(), Some(BlockNumberOrTag::Number(block_number.into())))
            .await?;
        if res.len() < 32 {
            anyhow::bail!("short returndata for inboxAccs")
        }
        Ok(Self::decode_b256_word(&res[0..32])?)
    }

    async fn lookup_batches_in_range(&self, from_block: u64, to_block: u64) -> anyhow::Result<Vec<SequencerInboxBatch>> {
        let topic0 = B256::from_slice(&keccak256(
            "SequencerBatchDelivered(uint256,bytes32,bytes32,bytes32,uint256,(uint64,uint64,uint64,uint64),uint8,uint8,uint8)"
                .as_bytes(),
        ));
        let filter = Filter {
            from_block: Some(from_block.into()),
            to_block: Some(to_block.into()),
            address: Some(vec![self.inbox_addr]),
            topics: Some(vec![vec![topic0]]),
            block_hash: None,
        };
        let logs = self.provider.get_logs(&filter).await?;
        let mut out = Vec::with_capacity(logs.len());
        let mut last_seq: Option<u64> = None;
        for lg in logs {
            if lg.topics.len() < 2 || lg.data.len() < 32 * 4 {
                continue;
            }
            let seq = U256::from_be_bytes(lg.topics[1].0).to::<u64>();
            if let Some(prev) = last_seq {
                if seq != prev + 1 {
                    anyhow::bail!("batches out of order: after {} got {}", prev, seq)
                }
            }
            last_seq = Some(seq);
            let before_acc = Self::decode_b256_word(&lg.data[0..32])?;
            let after_acc = Self::decode_b256_word(&lg.data[32..64])?;
            let delayed_acc = Self::decode_b256_word(&lg.data[64..96])?;
            let after_delayed_count = Self::decode_u256_word(&lg.data[96..128])?.to::<u64>();
            let batch = SequencerInboxBatch {
                sequence_number: seq,
                before_inbox_acc: before_acc,
                after_inbox_acc: after_acc,
                after_message_count: 0,
                after_delayed_count,
                after_delayed_acc: delayed_acc,
                parent_chain_block_number: lg.block_number.unwrap_or_default(),
                block_hash: lg.block_hash.unwrap_or_default(),
                serialized: Vec::new(),
            };
            out.push(batch);
        }
        Ok(out)
    }

    async fn get_sequencer_message_bytes_in_block(
        &self,
        _block_number: u64,
        _seq_num: u64,
    ) -> anyhow::Result<(Vec<u8>, B256, Vec<u64>)> {
        anyhow::bail!("unimplemented")
    }
}
