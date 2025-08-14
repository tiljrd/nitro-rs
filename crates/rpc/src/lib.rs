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
}

#[async_trait]
impl<D> reth_arbitrum_rpc::nitro::ArbNitroBackend for NitroRpcBackend<D>
where
    D: Database + Sync + Send + 'static,
{
    async fn new_message(
        &self,
        _msg_idx: u64,
        _msg: serde_json::Value,
        _msg_for_prefetch: Option<serde_json::Value>,
    ) -> Result<reth_arbitrum_rpc::nitro::ArbMessageResult, String> {
        Err("not implemented".to_string())
    }

    async fn reorg(
        &self,
        _first_msg_to_add: u64,
        _new_messages: Vec<serde_json::Value>,
        _old_messages: Vec<serde_json::Value>,
    ) -> Result<Vec<reth_arbitrum_rpc::nitro::ArbMessageResult>, String> {
        Err("not implemented".to_string())
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
