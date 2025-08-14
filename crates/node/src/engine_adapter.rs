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
use reth_payload_builder::EthPayloadBuilderAttributes;
use alloy_primitives::{Address, B256};
use reth_ethereum_engine_primitives::PayloadAttributes;
use reth_payload_primitives::PayloadKind;


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
        let count = <u64 as alloy_rlp::Decodable>::decode(&mut slice)
            .map_err(|e| anyhow!("failed to decode message count: {e}"))?;
        if count == 0 {
            return Err(anyhow!("no messages"));
        }
        Ok(count - 1)
    }

    async fn digest_message(
        &self,
        msg_idx: u64,
        msg: &MessageWithMetadata,
        _msg_for_prefetch: Option<&MessageWithMetadata>,
    ) -> Result<MessageResult> {
        let beacon = self
            .beacon_engine_handle
            .as_ref()
            .ok_or_else(|| anyhow!("missing beacon engine handle"))?;
        let builder = self
            .payload_builder_handle
            .as_ref()
            .ok_or_else(|| anyhow!("missing payload builder handle"))?;

        if msg_idx == 0 {
            return Err(anyhow!("cannot determine parent for first message index 0"));
        }
        let prev_key = db_key(MESSAGE_RESULT_PREFIX, msg_idx - 1);
        let prev = {
            let data = self.db.get(&prev_key)?;
            let mut slice = data.as_slice();
            MessageResult::decode(&mut slice)
                .map_err(|e| anyhow!("failed to decode prev MessageResult: {e}"))?
        };
        let parent_hash = prev.block_hash;

        let rpc_attrs = PayloadAttributes {
            timestamp: msg.message.header.timestamp,
            prev_randao: B256::ZERO,
            suggested_fee_recipient: Address::ZERO,
            withdrawals: None,
            parent_beacon_block_root: None,
        };

        let attrs = EthPayloadBuilderAttributes::new(parent_hash, rpc_attrs);
        let id = builder
            .send_new_payload(attrs)
            .await
            .map_err(|_| anyhow!("failed to get payload id"))??;

        let built = builder
            .resolve_kind(id, PayloadKind::Earliest)
            .await
            .ok_or_else(|| anyhow!("payload resolve channel closed"))??;

        let sealed = built.clone().into_sealed_block();
        let exec_data =
            <ArbPayloadTypes as reth_payload_primitives::PayloadTypes>::block_to_payload(sealed);

        let _status = beacon
            .new_payload(&exec_data)
            .await
            .map_err(|e| anyhow!("engine new_payload error: {e}"))?;

        let block_hash = built.block().hash();
        let header = built.block().header();
        let send_root = {
            let bytes = header.extra_data.as_ref();
            if bytes.len() >= 32 {
                B256::from_slice(&bytes[..32])
            } else {
                B256::ZERO
            }
        };

        Ok(MessageResult { block_hash, send_root })
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
