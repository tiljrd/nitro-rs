use async_trait::async_trait;
use alloy_primitives::B256;
use nitro_primitives::message::{MessageResult, MessageWithMetadata};
use anyhow::Result;

#[async_trait]
pub trait ExecEngine: Send + Sync {
    async fn head_message_index(&self) -> Result<u64>;
    async fn digest_message(
        &self,
        msg_idx: u64,
        msg: &MessageWithMetadata,
        msg_for_prefetch: Option<&MessageWithMetadata>,
    ) -> Result<MessageResult>;
    async fn result_at_message_index(&self, msg_idx: u64) -> Result<MessageResult>;
    async fn mark_feed_start(&self, to: u64) -> Result<()>;
}
