
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
use reth_arbitrum_evm::header::extract_send_root_from_header_extra;
use reth_arbitrum_node::follower::DynFollowerExecutor;


type PayloadTy = ArbEngineTypes<ArbPayloadTypes>;

pub struct RethExecEngine {
    beacon_engine_handle: Option<BeaconConsensusEngineHandle<PayloadTy>>,
    payload_builder_handle: Option<PayloadBuilderHandle<PayloadTy>>,
    follower_executor: Option<DynFollowerExecutor>,
    db: Arc<dyn Database>,
    genesis_hash: B256,
    last_timestamp: AtomicU64,
}

impl RethExecEngine {
    pub fn new(db: Arc<dyn Database>, genesis_hash: B256, genesis_timestamp: u64) -> Arc<Self> {
        Arc::new(Self {
            beacon_engine_handle: None,
            payload_builder_handle: None,
            follower_executor: None,
            db,
            genesis_hash,
            last_timestamp: AtomicU64::new(genesis_timestamp),
        })
    }

    pub fn new_with_handles(
        db: Arc<dyn Database>,
        beacon_engine_handle: BeaconConsensusEngineHandle<PayloadTy>,
        payload_builder_handle: Option<PayloadBuilderHandle<PayloadTy>>,
        follower_executor: Option<DynFollowerExecutor>,
        genesis_hash: B256,
        genesis_timestamp: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            beacon_engine_handle: Some(beacon_engine_handle),
            payload_builder_handle,
            follower_executor,
            db,
            genesis_hash,
            last_timestamp: AtomicU64::new(genesis_timestamp),
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
        tracing::info!("engine_adapter: head_message_index message_count={}", count);
        if count == 0 {
            return Ok(u64::MAX);
        }
        let mut i = count - 1;
        loop {
            let key = db_key(MESSAGE_RESULT_PREFIX, i);
            let has = self.db.has(&key)?;
            tracing::info!("engine_adapter: head_message_index check idx={} has={}", i, has);
            if has {
                return Ok(i);
            } else {
                if i == 0 {
                    break;
                }
                i -= 1;
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
        let mut ts = msg.message.header.timestamp;
        let prev = self.last_timestamp.load(Ordering::Relaxed);
        if ts <= prev {
            ts = prev.saturating_add(1);
        }
        self.last_timestamp.store(ts, Ordering::Relaxed);

        let rpc_attrs = EthPayloadAttributes {
            timestamp: ts,
            prev_randao: B256::ZERO,
            suggested_fee_recipient: Address::ZERO,
            withdrawals: None,
            parent_beacon_block_root: None,
        };
        let follower_attrs: alloy_rpc_types_engine::PayloadAttributes = alloy_rpc_types_engine::PayloadAttributes {
            timestamp: ts,
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

        let pre_fcu = ForkchoiceState {
            head_block_hash: parent_hash,
            safe_block_hash: parent_hash,
            finalized_block_hash: parent_hash,
        };
        let pre_resp = beacon
            .fork_choice_updated(pre_fcu, None, EngineApiMessageVersion::default())
            .await
            .map_err(|e| anyhow!("engine pre fork_choice_updated error: {e}"))?;
        if pre_resp.is_invalid() {
            return Err(anyhow!("engine pre fork_choice_updated invalid: {:?}", pre_resp.payload_status));
        }

        if self.payload_builder_handle.is_none() {
            let follower = self.follower_executor.as_ref().ok_or_else(|| anyhow!("missing follower executor"))?;
            let l2msg_bytes = msg.message.l2msg.as_ref();
            let (block_hash, send_root) = follower
                .execute_message_to_block(
                    parent_hash,
                    follower_attrs.clone(),
                    l2msg_bytes,
                    msg.message.header.poster,
                    msg.message.header.request_id,
                    msg.message.header.kind,
                    msg.message.header.l1_base_fee,
                    msg.message.batch_gas_cost,
                )
                .await
                .map_err(|e| anyhow!("follower execute_message_to_block failed: {e}"))?;
            tracing::info!("engine_adapter: follower produced block idx={} hash={:?} parent={:?}", msg_idx, block_hash, parent_hash);
            let _ = self.last_timestamp.fetch_max(ts, Ordering::Relaxed);
            return Ok(MessageResult { block_hash, send_root });
        }

        let builder = self.payload_builder_handle.as_ref().unwrap();

        let fcu_with_attrs = beacon
            .fork_choice_updated(
                ForkchoiceState {
                    head_block_hash: parent_hash,
                    safe_block_hash: parent_hash,
                    finalized_block_hash: parent_hash,
                },
                Some(rpc_attrs.clone()),
                EngineApiMessageVersion::default(),
            )
            .await
            .map_err(|e| anyhow!("engine fork_choice_updated(with attrs) error: {e}"))?;
        if fcu_with_attrs.is_invalid() {
            return Err(anyhow!("engine fork_choice_updated with attrs invalid: {:?}", fcu_with_attrs.payload_status));
        }

        let attrs = EthPayloadBuilderAttributes::new(parent_hash, rpc_attrs.clone());
        let id = {
            let mut attempts = 0usize;
            loop {
                match builder.send_new_payload(attrs.clone()).await {
                    Ok(Ok(id)) => break id,
                    Ok(Err(e)) => {
                        let msg = format!("{e}");
                        if msg.contains("missing parent header") && attempts < 60 {
                            attempts += 1;
                            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                            continue;
                        }
                        return Err(anyhow!("failed to get payload id: {e}"));
                    }
                    Err(_) => {
                        if attempts < 10 {
                            attempts += 1;
                            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
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

        let block_hash = built.block().hash();
        let header = built.block().header();
        let send_root = extract_send_root_from_header_extra(header.extra_data.as_ref());
        let fcu_state = ForkchoiceState {
            head_block_hash: block_hash,
            safe_block_hash: parent_hash,
            finalized_block_hash: parent_hash,
        };
        let fcu_resp = beacon
            .fork_choice_updated(fcu_state, None, EngineApiMessageVersion::default())
            .await
            .map_err(|e| anyhow!("engine fork_choice_updated error: {e}"))?;
        if !matches!(fcu_resp.payload_status.status, PayloadStatusEnum::Valid | PayloadStatusEnum::Syncing) {
            return Err(anyhow!("engine fork_choice_updated not valid/syncing: {:?}", fcu_resp.payload_status));
        }
        let _ = self.last_timestamp.fetch_max(header.timestamp, Ordering::Relaxed);
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
