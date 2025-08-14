use anyhow::Result;
use alloy_primitives::B256;
use tracing::info;

pub struct TransactionStreamer;

impl TransactionStreamer {
    pub fn new() -> Self { Self }

    pub async fn start(&self) -> Result<()> {
        info!("starting transaction streamer");
        Ok(())
    }

    pub fn add_messages_and_reorg(
        &self,
        delayed: &[(u64, B256, Vec<u8>)],
        sequencer_batches: &[(u64, Vec<u8>)],
    ) -> Result<()> {
        Ok(())
    }

    pub fn message_count(&self) -> Result<u64> {
        Ok(0)
    }

    pub fn result_at_message_index(&self, _index: u64) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}
