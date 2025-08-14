use anyhow::Result;
use alloy_primitives::B256;
use alloy_rlp::{Decodable, Decoder, Encodable};
use nitro_inbox::db::{Batch, Database};
use nitro_inbox::util::delete_starting_at;
use nitro_primitives::dbkeys::{
    db_key, BLOCK_HASH_INPUT_FEED_PREFIX, BLOCK_METADATA_INPUT_FEED_PREFIX,
    MESSAGE_COUNT_KEY, MESSAGE_PREFIX, MESSAGE_RESULT_PREFIX,
    MISSING_BLOCK_METADATA_INPUT_FEED_PREFIX,
};
use nitro_primitives::message::{BlockHashDbValue, MessageResult, MessageWithMetadata, MessageWithMetadataAndBlockInfo};
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

    fn set_message_count(&self, batch: &mut dyn Batch, count: u64) -> Result<()> {
        let enc = alloy_rlp::encode(&count);
        batch.put(MESSAGE_COUNT_KEY, &enc)?;
        Ok(())
    }

    fn write_message(
        &self,
        msg_idx: u64,
        msg: &MessageWithMetadataAndBlockInfo,
        batch: &mut dyn Batch,
        track_block_metadata_from: Option<u64>,
    ) -> Result<()> {
        let key_msg = db_key(MESSAGE_PREFIX, msg_idx);
        let msg_bytes = alloy_rlp::encode(&msg.message_with_meta);
        batch.put(&key_msg, &msg_bytes)?;

        let key_bh = db_key(BLOCK_HASH_INPUT_FEED_PREFIX, msg_idx);
        let bh_bytes = alloy_rlp::encode(&BlockHashDbValue { block_hash: msg.block_hash });
        batch.put(&key_bh, &bh_bytes)?;

        if let Some(start_from) = track_block_metadata_from {
            if msg_idx >= start_from {
                if let Some(ref meta) = msg.block_metadata {
                    let key_meta = db_key(BLOCK_METADATA_INPUT_FEED_PREFIX, msg_idx);
                    batch.put(&key_meta, meta)?;
                } else {
                    let key_missing = db_key(MISSING_BLOCK_METADATA_INPUT_FEED_PREFIX, msg_idx);
                    batch.put(&key_missing, &[])?;
                }
            }
        }
        Ok(())
    }

    fn write_messages(
        &self,
        first_msg_idx: u64,
        messages: &[MessageWithMetadataAndBlockInfo],
        mut batch: Option<Box<dyn Batch>>,
        track_block_metadata_from: Option<u64>,
    ) -> Result<()> {
        let mut local = false;
        if batch.is_none() {
            batch = Some(self.db.new_batch());
            local = true;
        }
        let b = batch.as_mut().unwrap().as_mut();
        for (i, m) in messages.iter().enumerate() {
            let idx = first_msg_idx + i as u64;
            self.write_message(idx, m, b, track_block_metadata_from)?;
        }
        self.set_message_count(b, first_msg_idx + messages.len() as u64)?;
        if local {
            batch.unwrap().write()?;
        }
        Ok(())
    }

    pub fn add_messages_and_reorg(
        &self,
        msg_idx_of_first_msg_to_add: u64,
        new_messages: &[MessageWithMetadataAndBlockInfo],
        track_block_metadata_from: Option<u64>,
    ) -> Result<()> {
        if msg_idx_of_first_msg_to_add == 0 {
            return Err(anyhow::anyhow!("cannot reorg out init message"));
        }
        let mut batch = self.db.new_batch();

        delete_starting_at(
            self.db.as_ref(),
            batch.as_mut(),
            MESSAGE_RESULT_PREFIX,
            &msg_idx_of_first_msg_to_add.to_be_bytes(),
        )?;
        delete_starting_at(
            self.db.as_ref(),
            batch.as_mut(),
            BLOCK_HASH_INPUT_FEED_PREFIX,
            &msg_idx_of_first_msg_to_add.to_be_bytes(),
        )?;
        delete_starting_at(
            self.db.as_ref(),
            batch.as_mut(),
            BLOCK_METADATA_INPUT_FEED_PREFIX,
            &msg_idx_of_first_msg_to_add.to_be_bytes(),
        )?;
        delete_starting_at(
            self.db.as_ref(),
            batch.as_mut(),
            MISSING_BLOCK_METADATA_INPUT_FEED_PREFIX,
            &msg_idx_of_first_msg_to_add.to_be_bytes(),
        )?;
        delete_starting_at(
            self.db.as_ref(),
            batch.as_mut(),
            MESSAGE_PREFIX,
            &msg_idx_of_first_msg_to_add.to_be_bytes(),
        )?;

        self.set_message_count(batch.as_mut(), msg_idx_of_first_msg_to_add)?;
        batch.write()?;

        if !new_messages.is_empty() {
            self.write_messages(
                msg_idx_of_first_msg_to_add,
                new_messages,
                None,
                track_block_metadata_from,
            )?;
        }
        Ok(())
    }

    pub fn message_count(&self) -> Result<u64> {
        let data = self.db.get(MESSAGE_COUNT_KEY)?;
        let mut dec = Decoder::new(&data);
        Ok(u64::decode(&mut dec)?)
    }
    fn store_result(&self, msg_idx: u64, res: &MessageResult, batch: &mut dyn Batch) -> Result<()> {
        let bytes = alloy_rlp::encode(res);
        let key = db_key(MESSAGE_RESULT_PREFIX, msg_idx);
        batch.put(&key, &bytes)?;
        Ok(())
    }


    pub fn result_at_message_index(&self, index: u64) -> Result<Option<MessageResult>> {
        let key = db_key(MESSAGE_RESULT_PREFIX, index);
        match self.db.get(&key) {
            Ok(v) => {
                let mut dec = Decoder::new(&v);
                let res = MessageResult::decode(&mut dec)?;
                Ok(Some(res))
            }
            Err(e) if e.to_string().contains("not found") => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn block_metadata_at_message_index(&self, index: u64) -> Result<Option<Vec<u8>>> {
        let key = db_key(BLOCK_METADATA_INPUT_FEED_PREFIX, index);
        match self.db.get(&key) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.to_string().contains("not found") => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub fn block_hash_at_message_index(&self, index: u64) -> Result<Option<B256>> {
        let key = db_key(BLOCK_HASH_INPUT_FEED_PREFIX, index);
        match self.db.get(&key) {
            Ok(v) => {
                let mut dec = Decoder::new(&v);
                let bh: BlockHashDbValue = BlockHashDbValue::decode(&mut dec)?;
                Ok(bh.block_hash)
            }
            Err(e) if e.to_string().contains("not found") => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub fn get_message(&self, index: u64) -> Result<MessageWithMetadata> {
        let key = db_key(MESSAGE_PREFIX, index);
        let data = self.db.get(&key)?;
        let mut dec = Decoder::new(&data);
        Ok(MessageWithMetadata::decode(&mut dec)?)
    }

    pub fn get_message_with_metadata_and_block_info(&self, index: u64) -> Result<MessageWithMetadataAndBlockInfo> {
        let message_with_meta = self.get_message(index)?;
        let block_hash = self.block_hash_at_message_index(index)?;
        let block_metadata = self.block_metadata_at_message_index(index)?;
        Ok(MessageWithMetadataAndBlockInfo {
            message_with_meta,
            block_hash,
            block_metadata,
        })
    }



    }
}
