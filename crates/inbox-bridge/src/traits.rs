use alloy_primitives::{Address, B256, U256};
use async_trait::async_trait;

#[async_trait]
pub trait DelayedBridge: Send + Sync {
    async fn get_message_count(&self, block_height: u64) -> anyhow::Result<u64>;
    async fn get_accumulator(&self, seq_num: u64, block_height: u64, block_hash: B256) -> anyhow::Result<B256>;
}

#[async_trait]
pub trait SequencerInbox: Send + Sync {
    async fn get_batch_count(&self, block_height: u64) -> anyhow::Result<u64>;
}

#[derive(Clone, Debug)]
pub struct L1Context {
    pub first_message_block: U256,
    pub sequencer_inbox_addr: Address,
}
