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
use reth_ethereum_engine_primitives::EthPayloadAttributes;
use alloy_rpc_types_engine::ForkchoiceState;
use reth_payload_primitives::PayloadKind;
use reth_payload_primitives::EngineApiMessageVersion;
use alloy_rpc_types_engine::PayloadStatusEnum;


type PayloadTy = ArbEngineTypes<ArbPayloadTypes>;

pub struct RethExecEngine {
    beacon_engine_handle: Option<BeaconConsensusEngineHandle<PayloadTy>>,
    payload_builder_handle: Option<PayloadBuilderHandle<PayloadTy>>,
    db: Arc<dyn Database>,
    genesis_hash: B256,
}

impl RethExecEngine {
    pub fn new(db: Arc<dyn Database>, genesis_hash: B256) -> Arc<Self> {
        Arc::new(Self {
            beacon_engine_handle: None,
            payload_builder_handle: None,
            db,
            genesis_hash,
        })
    }

    pub fn new_with_handles(
        db: Arc<dyn Database>,
        beacon_engine_handle: BeaconConsensusEngineHandle<PayloadTy>,
        payload_builder_handle: PayloadBuilderHandle<PayloadTy>,
        genesis_hash: B256,
    ) -> Arc<Self> {
        Arc::new(Self {
            beacon_engine_handle: Some(beacon_engine_handle),
            payload_builder_handle: Some(payload_builder_handle),
            db,
            genesis_hash,
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
            return Ok(u64::MAX);
        }
        let mut i = count - 1;
        loop {
            let key = db_key(MESSAGE_RESULT_PREFIX, i);
            match self.db.has(&key)? {
                true => return Ok(i),
                false => {
                    if i == 0 {
                        break;
                    }
                    i -= 1;
                }
            }
        }
        Ok(u64::MAX)
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

        let parent_hash = if msg_idx == 0 {
            self.genesis_hash
        } else {
            let prev_key = db_key(MESSAGE_RESULT_PREFIX, msg_idx - 1);
            let prev = {
                let data = self.db.get(&prev_key)?;
                let mut slice = data.as_slice();
                MessageResult::decode(&mut slice)
                    .map_err(|e| anyhow!("failed to decode prev MessageResult: {e}"))?
            };
            prev.block_hash
        };

        tracing::info!("engine_adapter: digest_message idx={} using parent_hash={:?}", msg_idx, parent_hash);
        let rpc_attrs = EthPayloadAttributes {
            timestamp: msg.message.header.timestamp,
            prev_randao: B256::ZERO,
            suggested_fee_recipient: Address::ZERO,
            withdrawals: None,
            parent_beacon_block_root: None,
        };

        if msg_idx == 0 {
            let fc = ForkchoiceState {
                head_block_hash: self.genesis_hash,
                safe_block_hash: self.genesis_hash,
                finalized_block_hash: self.genesis_hash,
            };
            let _ = beacon
                .fork_choice_updated(fc, None, EngineApiMessageVersion::default())
                .await
                .map_err(|e| anyhow!("engine initial fork_choice_updated error: {e}"))?;
            tracing::info!("engine_adapter: seeded forkchoice with genesis={:?}", self.genesis_hash);
        }

        let attrs = EthPayloadBuilderAttributes::new(parent_hash, rpc_attrs);
        let id = {
            let mut attempts = 0usize;
            loop {
                match builder.send_new_payload(attrs.clone()).await {
                    Ok(Ok(id)) => break id,
                    Ok(Err(e)) => {
                        let msg = format!("{e}");
                        if msg.contains("missing parent header") && attempts < 20 {
                            attempts += 1;
                            tracing::warn!("payload_builder: parent header not yet available, retry {attempts}/20");
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            continue;
                        }
                        return Err(anyhow!("failed to get payload id: {e}"));
                    }
                    Err(_) => {
                        if attempts < 5 {
                            attempts += 1;
                            tracing::warn!("payload_builder: channel closed? retrying {attempts}/5");
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            continue;
                        }
                        return Err(anyhow!("failed to get payload id: channel closed"));
                    }
                }
            }
        };

        let built = builder
            .resolve_kind(id, PayloadKind::Earliest)
            .await
            .ok_or_else(|| anyhow!("payload resolve channel closed"))??;

        let sealed = built.clone().into_sealed_block();
        let exec_data =
            <ArbPayloadTypes as reth_payload_primitives::PayloadTypes>::block_to_payload(sealed);

        let status = beacon
            .new_payload(exec_data)
            .await
            .map_err(|e| anyhow!("engine new_payload error: {e}"))?;
        tracing::info!("engine_adapter: new_payload status={:?}", status);
        if !matches!(status.status, PayloadStatusEnum::Valid) {
            return Err(anyhow!("engine new_payload not valid: {:?}", status));
        }

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
        let fcu_state = ForkchoiceState {
            head_block_hash: block_hash,
            safe_block_hash: block_hash,
            finalized_block_hash: block_hash,
        };
        let fcu_resp = beacon
            .fork_choice_updated(fcu_state, None, EngineApiMessageVersion::default())
            .await
            .map_err(|e| anyhow!("engine fork_choice_updated error: {e}"))?;
        tracing::info!("engine_adapter: forkchoiceUpdated payload_status={:?}", fcu_resp.payload_status);
        if !matches!(fcu_resp.payload_status.status, PayloadStatusEnum::Valid) {
            return Err(anyhow!("engine fork_choice_updated not valid: {:?}", fcu_resp.payload_status));
        }
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
