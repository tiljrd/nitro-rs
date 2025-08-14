use std::sync::Arc;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use nitro_streamer::engine::ExecEngine;
use nitro_primitives::message::{MessageResult, MessageWithMetadata};
use nitro_inbox::db::Database;

use nitro_primitives::dbkeys::{MESSAGE_COUNT_KEY, MESSAGE_RESULT_PREFIX, db_key};
use alloy_rlp::Decodable;

use reth_node_api::BeaconConsensusEngineHandle;
use reth_payload_builder::PayloadBuilderHandle;
use reth_arbitrum_node::ArbEngineTypes;
use reth_arbitrum_payload::ArbPayloadTypes;

type PayloadTy = ArbEngineTypes<ArbPayloadTypes>;

pub struct RethExecEngine {
    beacon_engine_handle: Option<BeaconConsensusEngineHandle<PayloadTy>>,
    payload_builder_handle: Option<PayloadBuilderHandle<PayloadTy>>,
    db: Arc<dyn Database>,
}

impl RethExecEngine {
    pub fn new(db: Arc<dyn Database>) -> Arc<Self> {
        Arc::new(Self {
            beacon_engine_handle: None,
            payload_builder_handle: None,
            db,
        })
    }

    pub fn new_with_handles(
        db: Arc<dyn Database>,
        beacon_engine_handle: BeaconConsensusEngineHandle<PayloadTy>,
        payload_builder_handle: PayloadBuilderHandle<PayloadTy>,
    ) -> Arc<Self> {
        Arc::new(Self {
            beacon_engine_handle: Some(beacon_engine_handle),
            payload_builder_handle: Some(payload_builder_handle),
            db,
        })
    }
}

#[async_trait]
impl ExecEngine for RethExecEngine {
    async fn head_message_index(&self) -> Result<u64> {
        let data = self.db.get(MESSAGE_COUNT_KEY)?;
        let mut slice = data.as_slice();
        let count = alloy_rlp::decode::<u64>(&mut slice)
            .map_err(|e| anyhow!("failed to decode message count: {e}"))?;
        if count == 0 {
            return Err(anyhow!("no messages"));
        }
        Ok(count - 1)
    }

    async fn digest_message(
        &self,
        _msg_idx: u64,
        _msg: &MessageWithMetadata,
        _msg_for_prefetch: Option<&MessageWithMetadata>,
    ) -> Result<MessageResult> {
        anyhow::bail!("RethExecEngine::digest_message not implemented yet")
    }

    async fn result_at_message_index(&self, msg_idx: u64) -> Result<MessageResult> {
        let key = db_key(MESSAGE_RESULT_PREFIX, msg_idx);
        let data = self.db.get(&key)?;
        let mut slice = data.as_slice();
        let res = MessageResult::decode(&mut slice)
            .map_err(|e| anyhow!("failed to decode MessageResult: {e}"))?;
        Ok(res)
    }

    async fn mark_feed_start(&self, _to: u64) -> Result<()> {
        Ok(())
    }
}
