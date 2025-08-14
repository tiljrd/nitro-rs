use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use nitro_streamer::engine::ExecEngine;
use nitro_primitives::message::{MessageResult, MessageWithMetadata};

pub struct RethExecEngine {
}

impl RethExecEngine {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { })
    }
}

#[async_trait]
impl ExecEngine for RethExecEngine {
    async fn head_message_index(&self) -> Result<u64> {
        anyhow::bail!("RethExecEngine::head_message_index not implemented yet")
    }

    async fn digest_message(
        &self,
        _msg_idx: u64,
        _msg: &MessageWithMetadata,
        _msg_for_prefetch: Option<&MessageWithMetadata>,
    ) -> Result<MessageResult> {
        anyhow::bail!("RethExecEngine::digest_message not implemented yet")
    }

    async fn result_at_message_index(&self, _msg_idx: u64) -> Result<MessageResult> {
        anyhow::bail!("RethExecEngine::result_at_message_index not implemented yet")
    }

    async fn mark_feed_start(&self, _to: u64) -> Result<()> {
        anyhow::bail!("RethExecEngine::mark_feed_start not implemented yet")
    }
}
