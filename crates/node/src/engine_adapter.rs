use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use nitro_streamer::engine::ExecEngine;
use nitro_primitives::message::{MessageResult, MessageWithMetadata};

use reth_node_api::BeaconConsensusEngineHandle;
use reth_payload_builder::PayloadBuilderHandle;
use reth_arbitrum_node::ArbEngineTypes;
use reth_arbitrum_payload::ArbPayloadTypes;

type PayloadTy = ArbEngineTypes<ArbPayloadTypes>;

pub struct RethExecEngine {
    beacon_engine_handle: Option<BeaconConsensusEngineHandle<PayloadTy>>,
    payload_builder_handle: Option<PayloadBuilderHandle<PayloadTy>>,
}

impl RethExecEngine {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            beacon_engine_handle: None,
            payload_builder_handle: None,
        })
    }

    pub fn new_with_handles(
        beacon_engine_handle: BeaconConsensusEngineHandle<PayloadTy>,
        payload_builder_handle: PayloadBuilderHandle<PayloadTy>,
    ) -> Arc<Self> {
        Arc::new(Self {
            beacon_engine_handle: Some(beacon_engine_handle),
            payload_builder_handle: Some(payload_builder_handle),
        })
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
