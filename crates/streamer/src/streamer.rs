use anyhow::Result;
use alloy_primitives::B256;
use nitro_inbox::db::Database;
use nitro_primitives::dbkeys::{MESSAGE_COUNT_KEY, MESSAGE_RESULT_PREFIX, db_key};
use std::sync::Arc;
use tracing::info;

pub struct TransactionStreamer<D: Database> {
    db: Arc<D>,
}

impl<D: Database> TransactionStreamer<D> {
    pub fn new(db: Arc<D>) -> Self {
        Self { db }
    }

    pub async fn start(&self) -> Result<()> {
        info!("starting transaction streamer");
        Ok(())
    }

    pub fn add_messages_and_reorg(
        &self,
        _delayed: &[(u64, B256, Vec<u8>)],
        _sequencer_batches: &[(u64, Vec<u8>)],
    ) -> Result<()> {
        Ok(())
    }

    pub fn message_count(&self) -> Result<u64> {
        let data = self.db.get(MESSAGE_COUNT_KEY)?;
        let mut dec = alloy_rlp::Decoder::new(&data);
        Ok(u64::decode(&mut dec)?)
    }

    pub fn result_at_message_index(&self, index: u64) -> Result<Option<Vec<u8>>> {
        let key = db_key(MESSAGE_RESULT_PREFIX, index);
        match self.db.get(&key) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.to_string().contains("not found") => Ok(None),
            Err(e) => Err(e),
        }
    }
}
