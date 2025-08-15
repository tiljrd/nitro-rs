use nitro_inbox::streamer::Streamer;
use anyhow::{Result, anyhow};
use alloy_primitives::B256;
use alloy_rlp::{Decodable, Encodable};
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
use crate::engine::ExecEngine;

pub struct TransactionStreamer<D: Database> {
    db: Arc<D>,
    exec: Arc<dyn ExecEngine>,
}

impl<D: Database> TransactionStreamer<D> {
    pub fn new(db: Arc<D>, exec: Arc<dyn ExecEngine>) -> Self {
        Self { db, exec }
    }

    pub async fn start(&self) -> Result<()> {
        info!("starting transaction streamer");
        let mut delay = tokio::time::interval(std::time::Duration::from_millis(200));
        delay.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            match self.execute_next_msg().await {
                Ok(true) => {}
                Ok(false) => {
                    delay.tick().await;
                }
                Err(e) => {
                    tracing::error!("streamer execute_next_msg error: {e:?}");
                    delay.tick().await;
                }
            }
        }
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
    fn get_prev_prev_delayed_read(&self, msg_idx: u64) -> Result<u64> {
        if msg_idx == 0 {
            return Ok(0);
        }
        let prev = self.get_message(msg_idx - 1)?;
        Ok(prev.delayed_messages_read)
    }

    fn count_duplicate_messages(
        &self,
        mut msg_idx: u64,
        messages: &[MessageWithMetadataAndBlockInfo],
        mut batch: Option<&mut dyn Batch>,
    ) -> Result<(u64, bool, Option<MessageWithMetadata>)> {
        let mut cur: u64 = 0;
        while (cur as usize) < messages.len() {
            let key = db_key(MESSAGE_PREFIX, msg_idx);
            let have = match self.db.get(&key) {
                Ok(v) => v,
                Err(e) if e.to_string().contains("not found") => break,
                Err(e) => return Err(e),
            };

            let want = alloy_rlp::encode(&messages[cur as usize].message_with_meta);
            if have != want {
                let mut slice = have.as_slice();
                let db_msg = match MessageWithMetadata::decode(&mut slice) {
                    Ok(m) => m,
                    Err(_) => {
                        return Ok((cur, true, None));
                    }
                };

                let next = &messages[cur as usize].message_with_meta;

                let mut is_duplicate = false;
                let mut db_cmp = db_msg.clone();
                let mut next_cmp = next.clone();
                if db_cmp.message.batch_gas_cost.is_none() || next_cmp.message.batch_gas_cost.is_none() {
                    db_cmp.message.batch_gas_cost = None;
                    next_cmp.message.batch_gas_cost = None;
                    if db_cmp == next_cmp {
                        is_duplicate = true;
                        if let Some(b) = batch.as_deref_mut() {
                            if messages[cur as usize].message_with_meta.message.batch_gas_cost.is_some() {
                                let _ = self.write_message(
                                    msg_idx,
                                    &messages[cur as usize],
                                    b,
                                    None,
                                );
                            }
                        }
                    }
                }

                if !is_duplicate {
                    return Ok((cur, true, Some(db_msg)));
                }
            }

            cur += 1;
            msg_idx += 1;
        }
        Ok((cur, false, None))
    }
    fn add_messages_and_end_batch_impl(
        &self,
        mut first_msg_idx: u64,
        messages_are_confirmed: bool,
        mut messages: Vec<MessageWithMetadataAndBlockInfo>,
        track_block_metadata_from: Option<u64>,
    ) -> Result<()> {
        let mut confirmed_reorg = false;
        let mut old_msg: Option<MessageWithMetadata> = None;
        let mut last_delayed_read: u64 = 0;

        if messages_are_confirmed {
            let mut _batch_for_writeback = self.db.new_batch();
            let (num_dups, conf_reorg, old) =
                self.count_duplicate_messages(first_msg_idx, &messages, Some(_batch_for_writeback.as_mut()))?;
            confirmed_reorg = conf_reorg;
            old_msg = old;
            if num_dups > 0 {
                last_delayed_read = messages[num_dups as usize - 1].message_with_meta.delayed_messages_read;
                messages.drain(0..num_dups as usize);
                first_msg_idx += num_dups;
            }
        } else {
            let (num_dups, feed_reorg, old) = self.count_duplicate_messages(first_msg_idx, &messages, None)?;
            old_msg = old;
            if feed_reorg {
                return Ok(());
            }
            if num_dups > 0 {
                last_delayed_read = messages[num_dups as usize - 1].message_with_meta.delayed_messages_read;
                messages.drain(0..num_dups as usize);
                first_msg_idx += num_dups;
            }
        }

        if last_delayed_read == 0 {
            last_delayed_read = self.get_prev_prev_delayed_read(first_msg_idx)?;
        }

        for (i, msg) in messages.iter().enumerate() {
            let msg_idx = first_msg_idx + i as u64;
            let dm_read = msg.message_with_meta.delayed_messages_read;
            let diff = dm_read.saturating_sub(last_delayed_read);
            if diff != 0 && diff != 1 {
                return Err(anyhow!(
                    "attempted to insert jump from {} delayed messages read to {} delayed messages read at message index {}",
                    last_delayed_read,
                    dm_read,
                    msg_idx
                ));
            }
            last_delayed_read = dm_read;
        }

        if confirmed_reorg {
            self.add_messages_and_reorg(first_msg_idx, &messages, track_block_metadata_from)?;
            return Ok(());
        }
        if messages.is_empty() {
            return Ok(());
        }

        self.write_messages(first_msg_idx, &messages, None, track_block_metadata_from)
    }
    pub async fn execute_next_msg(&self) -> Result<bool> {
        let consensus_head = self.get_head_message_index()?;
        if consensus_head == u64::MAX {
            return Ok(false);
        }
        let exec_head = self.exec.head_message_index().await?;
        let msg_idx = if exec_head == u64::MAX { 0 } else { exec_head + 1 };
        if msg_idx > consensus_head {
            return Ok(false);
        }
        let msg_and_block = self.get_message_with_metadata_and_block_info(msg_idx)?;
        let mut msg_for_prefetch: Option<MessageWithMetadata> = None;
        if msg_idx + 1 <= consensus_head {
            msg_for_prefetch = Some(self.get_message(msg_idx + 1)?);
        }
        let res = self.exec.digest_message(
            msg_idx,
            &msg_and_block.message_with_meta,
            msg_for_prefetch.as_ref(),
        ).await?;
        let mut batch = self.db.new_batch();
        self.store_result(msg_idx, &res, batch.as_mut())?;
        batch.write()?;
        Ok(msg_idx + 1 <= consensus_head)
    }


    pub fn message_count(&self) -> Result<u64> {
        let data = self.db.get(MESSAGE_COUNT_KEY)?;
        let mut slice = data.as_slice();
        let count: u64 = u64::decode(&mut slice)?;
        Ok(count)
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
                let mut slice = v.as_slice();
                let res: MessageResult = MessageResult::decode(&mut slice)?;
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
                let mut slice = v.as_slice();
                let bh: BlockHashDbValue = BlockHashDbValue::decode(&mut slice)?;
                Ok(bh.block_hash)
            }
            Err(e) if e.to_string().contains("not found") => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub fn get_message(&self, index: u64) -> Result<MessageWithMetadata> {
        let key = db_key(MESSAGE_PREFIX, index);
        let data = self.db.get(&key)?;
        let mut slice = data.as_slice();
        Ok(MessageWithMetadata::decode(&mut slice)?)
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
    pub fn reorg_at_and_end_batch(&self, first_msg_idx_reorged: u64) -> Result<()> {
        self.add_messages_and_reorg(first_msg_idx_reorged, &[], None)
    }

    pub fn add_messages_and_end_batch(
        &self,
        first_msg_idx: u64,
        new_messages: &[MessageWithMetadataAndBlockInfo],
        track_block_metadata_from: Option<u64>,
    ) -> Result<()> {
        self.add_messages_and_end_batch_impl(
            first_msg_idx,
            false,
            new_messages.to_vec(),
            track_block_metadata_from,
        )
    }

    pub fn add_confirmed_messages_and_end_batch(
        &self,
        first_msg_idx: u64,
        new_messages: &[MessageWithMetadataAndBlockInfo],
        track_block_metadata_from: Option<u64>,
    ) -> Result<()> {
        self.add_messages_and_end_batch_impl(
            first_msg_idx,
            true,
            new_messages.to_vec(),
            track_block_metadata_from,
        )
    }

    pub fn get_head_message_index(&self) -> Result<u64> {
        let count = self.message_count()?;
        if count == 0 {
            Ok(u64::MAX)
        } else {
            Ok(count - 1)
        }
    }

    pub fn get_message_count(&self) -> Result<u64> {
        self.message_count()
    }



}
impl<D: Database> Streamer for TransactionStreamer<D> {
    fn reorg_at_and_end_batch(&self, first_msg_idx_reorged: u64) -> Result<()> {
        TransactionStreamer::reorg_at_and_end_batch(self, first_msg_idx_reorged)
    }
    fn add_messages_and_end_batch(
        &self,
        first_msg_idx: u64,
        new_messages: &[MessageWithMetadataAndBlockInfo],
        track_block_metadata_from: Option<u64>,
    ) -> Result<()> {
        TransactionStreamer::add_messages_and_end_batch(self, first_msg_idx, new_messages, track_block_metadata_from)
    }
    fn add_confirmed_messages_and_end_batch(
        &self,
        first_msg_idx: u64,
        new_messages: &[MessageWithMetadataAndBlockInfo],
        track_block_metadata_from: Option<u64>,
    ) -> Result<()> {
        TransactionStreamer::add_confirmed_messages_and_end_batch(self, first_msg_idx, new_messages, track_block_metadata_from)
    }
}
