use hex::FromHex;
use std::str::FromStr;
use std::sync::Arc;
use async_trait::async_trait;
use alloy_primitives::B256;
use nitro_inbox::tracker::InboxTracker;
use nitro_inbox::db::Database;
use nitro_streamer::streamer::TransactionStreamer;

pub struct NitroRpcBackend<D: Database> {
    tracker: Arc<InboxTracker<D>>,
    streamer: Arc<TransactionStreamer<D>>,
}

impl<D: Database> NitroRpcBackend<D> {
    pub fn new(tracker: Arc<InboxTracker<D>>, streamer: Arc<TransactionStreamer<D>>) -> Self {
        Self { tracker, streamer }
    }
fn parse_hex_bytes(s: &str) -> Result<Vec<u8>, String> {
    let ss = s.trim_start_matches("0x");
    Vec::from_hex(ss).map_err(|e| e.to_string())
}

fn parse_opt_b256(v: &serde_json::Value) -> Result<Option<alloy_primitives::B256>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let s = v.as_str().ok_or_else(|| "expected hex string".to_string())?;
    let b = alloy_primitives::B256::from_str(s).map_err(|e| e.to_string())?;
    Ok(Some(b))
}

fn parse_u256_hex(s: &str) -> Result<alloy_primitives::U256, String> {
    alloy_primitives::U256::from_str(s).map_err(|e| e.to_string())
}

fn parse_msg_with_meta_and_block(val: &serde_json::Value) -> Result<nitro_primitives::message::MessageWithMetadataAndBlockInfo, String> {
    use nitro_primitives::l1::{L1IncomingMessage, L1IncomingMessageHeader};
    let obj = val.as_object().ok_or_else(|| "expected object".to_string())?;
    let delayed = obj.get("delayedMessagesRead").and_then(|x| x.as_u64()).ok_or_else(|| "missing delayedMessagesRead".to_string())?;

    let msg_v = obj.get("message").ok_or_else(|| "missing message".to_string())?;
    let msg_obj = msg_v.as_object().ok_or_else(|| "message not object".to_string())?;

    let header_v = msg_obj.get("header").ok_or_else(|| "missing header".to_string())?;
    let header_obj = header_v.as_object().ok_or_else(|| "header not object".to_string())?;

    let kind = header_obj.get("kind").and_then(|x| x.as_u64()).ok_or_else(|| "missing kind".to_string())? as u8;
    let poster_s = header_obj.get("poster").and_then(|x| x.as_str()).ok_or_else(|| "missing poster".to_string())?;
    let poster = alloy_primitives::Address::from_str(poster_s).map_err(|e| e.to_string())?;
    let block_number = header_obj.get("blockNumber").and_then(|x| x.as_u64()).ok_or_else(|| "missing blockNumber".to_string())?;
    let timestamp = header_obj.get("timestamp").and_then(|x| x.as_u64()).ok_or_else(|| "missing timestamp".to_string())?;
    let request_id = match header_obj.get("requestId") {
        Some(x) if !x.is_null() => {
            let s = x.as_str().ok_or_else(|| "requestId not string".to_string())?;
            Some(alloy_primitives::B256::from_str(s).map_err(|e| e.to_string())?)
        }
        _ => None,
    };
    let l1_base_fee = match header_obj.get("l1BaseFee") {
        Some(x) if x.is_string() => Self::parse_u256_hex(x.as_str().unwrap())?,
        Some(x) if x.is_u64() => alloy_primitives::U256::from(x.as_u64().unwrap()),
        Some(_) => return Err("l1BaseFee invalid".to_string()),
        None => alloy_primitives::U256::from(0u64),
    };

    let l2msg_s = msg_obj.get("l2msg").and_then(|x| x.as_str()).ok_or_else(|| "missing l2msg".to_string())?;
    let l2msg = Self::parse_hex_bytes(l2msg_s)?;

    let batch_gas_cost = msg_obj.get("batchGasCost").and_then(|x| x.as_u64());

    let header = L1IncomingMessageHeader {
        kind,
        poster,
        block_number,
        timestamp,
        request_id,
        l1_base_fee,
    };
    let message = L1IncomingMessage { header, l2msg, batch_gas_cost };

    let block_hash = if let Some(v) = obj.get("blockHash") { Self::parse_opt_b256(v)? } else { None };
    let block_metadata = if let Some(v) = obj.get("blockMetadata") {
        if v.is_null() { None } else {
            let s = v.as_str().ok_or_else(|| "blockMetadata not string".to_string())?;
            Some(Self::parse_hex_bytes(s)?)
        }
    } else { None };

    Ok(nitro_primitives::message::MessageWithMetadataAndBlockInfo {
        message_with_meta: nitro_primitives::message::MessageWithMetadata {
            message,
            delayed_messages_read: delayed,
        },
        block_hash,
        block_metadata,
    })
}

}

#[async_trait]
impl<D> reth_arbitrum_rpc::nitro::ArbNitroBackend for NitroRpcBackend<D>
where
    D: Database + Sync + Send + 'static,
{
    async fn new_message(
        &self,
        msg_idx: u64,
        msg: serde_json::Value,
        _msg_for_prefetch: Option<serde_json::Value>,
    ) -> Result<reth_arbitrum_rpc::nitro::ArbMessageResult, String> {
        let parsed = Self::parse_msg_with_meta_and_block(&msg)?;
        self.streamer
            .add_messages_and_end_batch(msg_idx, &[parsed], None)
            .map_err(|e| e.to_string())?;
        if let Some(res) = self.streamer.result_at_message_index(msg_idx).map_err(|e| e.to_string())? {
            Ok(reth_arbitrum_rpc::nitro::ArbMessageResult { block_hash: res.block_hash, send_root: res.send_root })
        } else {
            Ok(reth_arbitrum_rpc::nitro::ArbMessageResult { block_hash: B256::ZERO, send_root: B256::ZERO })
        }
    }

    async fn reorg(
        &self,
        first_msg_to_add: u64,
        new_messages: Vec<serde_json::Value>,
        _old_messages: Vec<serde_json::Value>,
    ) -> Result<Vec<reth_arbitrum_rpc::nitro::ArbMessageResult>, String> {
        let mut parsed: Vec<nitro_primitives::message::MessageWithMetadataAndBlockInfo> = Vec::with_capacity(new_messages.len());
        for v in new_messages.iter() {
            parsed.push(Self::parse_msg_with_meta_and_block(v)?);
        }
        self.streamer
            .add_messages_and_reorg(first_msg_to_add, &parsed, None)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(parsed.len());
        for i in 0..parsed.len() {
            let idx = first_msg_to_add + i as u64;
            if let Some(res) = self.streamer.result_at_message_index(idx).map_err(|e| e.to_string())? {
                out.push(reth_arbitrum_rpc::nitro::ArbMessageResult { block_hash: res.block_hash, send_root: res.send_root });
            } else {
                out.push(reth_arbitrum_rpc::nitro::ArbMessageResult { block_hash: B256::ZERO, send_root: B256::ZERO });
            }
        }
        Ok(out)
    }

    async fn head_message_index(&self) -> Result<u64, String> {
        self.streamer.get_head_message_index().map_err(|e| e.to_string())
    }

    async fn result_at_message_index(
        &self,
        msg_idx: u64,
    ) -> Result<reth_arbitrum_rpc::nitro::ArbMessageResult, String> {
        let res = self
            .streamer
            .result_at_message_index(msg_idx)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "result not found".to_string())?;
        Ok(reth_arbitrum_rpc::nitro::ArbMessageResult { block_hash: res.block_hash, send_root: res.send_root })
    }

    async fn message_index_to_block_number(&self, msg_idx: u64) -> Result<u64, String> {
        Ok(msg_idx)
    }

    async fn block_number_to_message_index(&self, block_number: u64) -> Result<u64, String> {
        Ok(block_number)
    }

    async fn set_finality_data(
        &self,
        _safe: Option<serde_json::Value>,
        _finalized: Option<serde_json::Value>,
        _validated: Option<serde_json::Value>,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn mark_feed_start(&self, _to: u64) -> Result<(), String> {
        Ok(())
    }

    async fn trigger_maintenance(&self) -> Result<(), String> {
        Ok(())
    }

    async fn should_trigger_maintenance(&self) -> Result<bool, String> {
        Ok(false)
    }

    async fn maintenance_status(
        &self,
    ) -> Result<reth_arbitrum_rpc::nitro::ArbMaintenanceStatus, String> {
        Ok(reth_arbitrum_rpc::nitro::ArbMaintenanceStatus { status: "ok".to_string() })
    }

    async fn record_block_creation(
        &self,
        _pos: u64,
        _msg: serde_json::Value,
    ) -> Result<reth_arbitrum_rpc::nitro::ArbRecordResult, String> {
        Ok(reth_arbitrum_rpc::nitro::ArbRecordResult { result_hash: B256::ZERO })
    }

    async fn mark_valid(&self, _pos: u64, _result_hash: B256) -> Result<(), String> {
        Ok(())
    }

    async fn prepare_for_record(&self, _start: u64, _end: u64) -> Result<(), String> {
        Ok(())
    }

    async fn pause_sequencer(&self) -> Result<(), String> {
        Ok(())
    }

    async fn activate_sequencer(&self) -> Result<(), String> {
        Ok(())
    }

    async fn forward_to(&self, _url: String) -> Result<(), String> {
        Ok(())
    }

    async fn sequence_delayed_message(
        &self,
        _message: serde_json::Value,
        _delayed_seq_num: u64,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn next_delayed_message_number(&self) -> Result<u64, String> {
        self.tracker.get_delayed_count().map_err(|e| e.to_string())
    }

    async fn synced(&self) -> Result<bool, String> {
        Ok(true)
    }

    async fn full_sync_progress(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"status":"idle"}))
    }

    async fn arbos_version_for_message_index(&self, _msg_idx: u64) -> Result<u64, String> {
        Ok(1)
    }
}

pub fn register_backend<D>(tracker: Arc<InboxTracker<D>>, streamer: Arc<TransactionStreamer<D>>) -> bool
where
    D: Database + Sync + Send + 'static,
{
    let backend = NitroRpcBackend::new(tracker, streamer);
    reth_arbitrum_rpc::nitro::set_arb_nitro_backend(Arc::new(backend))
}
