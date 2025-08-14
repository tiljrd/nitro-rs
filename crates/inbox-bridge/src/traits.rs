use alloy_primitives::B256;
use async_trait::async_trait;
use crate::types::{DelayedInboxMessage, SequencerInboxBatch};
use tokio::sync::mpsc::Receiver;

#[async_trait]
pub trait DelayedBridge: Send + Sync {
    async fn get_message_count(&self, block_number: u64) -> anyhow::Result<u64>;
    async fn get_accumulator(&self, seq_num: u64, block_number: u64, block_hash: B256) -> anyhow::Result<B256>;
    async fn lookup_messages_in_range<F>(
        &self,
        from_block: u64,
        to_block: u64,
        batch_fetcher: F,
    ) -> anyhow::Result<Vec<DelayedInboxMessage>>
    where
        F: Fn(u64) -> anyhow::Result<Vec<u8>> + Send + Sync;
}

#[async_trait]
pub trait SequencerInbox: Send + Sync {
    async fn get_batch_count(&self, block_number: u64) -> anyhow::Result<u64>;
    async fn get_accumulator(&self, seq_num: u64, block_number: u64) -> anyhow::Result<B256>;
    async fn lookup_batches_in_range(&self, from_block: u64, to_block: u64) -> anyhow::Result<Vec<SequencerInboxBatch>>;
}

#[derive(Clone, Debug)]
pub struct L1Header {
    pub number: u64,
}

#[async_trait]
pub trait L1HeaderReader: Send + Sync {
    async fn last_header(&self) -> anyhow::Result<L1Header>;
    async fn latest_safe_block_nr(&self) -> anyhow::Result<u64>;
    async fn latest_finalized_block_nr(&self) -> anyhow::Result<u64>;
    async fn subscribe(&self) -> (Receiver<L1Header>, Box<dyn FnOnce() + Send>);
}
