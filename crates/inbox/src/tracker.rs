use crate::db::Database;
use alloy_primitives::B256;
use alloy_rlp::Decodable;
use nitro_primitives::dbkeys::*;
use nitro_primitives::accumulator::hash_after;
use nitro_primitives::l1::{L1IncomingMessage, parse_incoming_l1_message_legacy, serialize_incoming_l1_message_legacy};
use inbox_bridge::types::SequencerInboxBatch;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SnapSyncConfig {
    pub enabled: bool,
    pub delayed_count: u64,
    pub batch_count: u64,
    pub prev_batch_message_count: u64,
}

#[derive(Clone)]
pub struct BatchMetadata {
    pub accumulator: B256,
    pub message_count: u64,
    pub delayed_message_count: u64,
    pub parent_chain_block: u64,
}

pub struct InboxTracker<D: Database> {
    pub db: Arc<D>,
    pub batch_meta_cache: Mutex<lru::LruCache<u64, BatchMetadata>>,
}

impl<D: Database> InboxTracker<D> {
    pub fn new(db: Arc<D>) -> Self {
        Self {
            db,
            batch_meta_cache: Mutex::new(lru::LruCache::new(std::num::NonZeroUsize::new(1000).unwrap())),
        }
    }

    pub fn initialize(&self) -> anyhow::Result<()> {
        let mut batch = self.db.new_batch();
        if !self.db.has(DELAYED_MESSAGE_COUNT_KEY)? {
            let value = alloy_rlp::encode(&0u64);
            batch.put(DELAYED_MESSAGE_COUNT_KEY, &value)?;
        }
        if !self.db.has(SEQUENCER_BATCH_COUNT_KEY)? {
            let value = alloy_rlp::encode(&0u64);
            batch.put(SEQUENCER_BATCH_COUNT_KEY, &value)?;
        }
        batch.write()?;
        Ok(())
    }

    pub fn get_delayed_acc(&self, seqnum: u64) -> anyhow::Result<B256> {
        let mut key = db_key(RLP_DELAYED_MESSAGE_PREFIX, seqnum);
        if !self.db.has(&key)? {
            key = db_key(LEGACY_DELAYED_MESSAGE_PREFIX, seqnum);
            if !self.db.has(&key)? {
                anyhow::bail!("accumulator not found: delayed {}", seqnum);
            }
        }
        let data = self.db.get(&key)?;
        if data.len() < 32 {
            anyhow::bail!("delayed message entry missing accumulator");
        }
        Ok(B256::from_slice(&data[..32]))
    }

    pub fn get_delayed_count(&self) -> anyhow::Result<u64> {
        let data = self.db.get(DELAYED_MESSAGE_COUNT_KEY)?;
        let mut bytes = &data[..];
        Ok(u64::decode(&mut bytes)?)
    }

    pub fn get_batch_metadata(&self, seqnum: u64) -> anyhow::Result<BatchMetadata> {
        if let Some(m) = self.batch_meta_cache.lock().unwrap().get(&seqnum).cloned() {
            return Ok(m);
        }
        let key = db_key(SEQUENCER_BATCH_META_PREFIX, seqnum);
        if !self.db.has(&key)? {
            anyhow::bail!("accumulator not found: no metadata for batch {}", seqnum);
        }
        let data = self.db.get(&key)?;
        let mut bytes = &data[..];
        let acc: B256 = B256::decode(&mut bytes)?;
        let msg_count: u64 = u64::decode(&mut bytes)?;
        let delayed_count: u64 = u64::decode(&mut bytes)?;
        let parent_block: u64 = u64::decode(&mut bytes)?;
        let meta = BatchMetadata {
            accumulator: acc,
            message_count: msg_count,
            delayed_message_count: delayed_count,
            parent_chain_block: parent_block,
        };
        self.batch_meta_cache.lock().unwrap().put(seqnum, meta.clone());
        Ok(meta)
    }

    pub fn get_batch_message_count(&self, seqnum: u64) -> anyhow::Result<u64> {
        Ok(self.get_batch_metadata(seqnum)?.message_count)
    }

    pub fn get_batch_parent_chain_block(&self, seqnum: u64) -> anyhow::Result<u64> {
        Ok(self.get_batch_metadata(seqnum)?.parent_chain_block)
    }

    pub fn get_batch_acc(&self, seqnum: u64) -> anyhow::Result<B256> {
        Ok(self.get_batch_metadata(seqnum)?.accumulator)
    }

    pub fn get_batch_count(&self) -> anyhow::Result<u64> {
        let data = self.db.get(SEQUENCER_BATCH_COUNT_KEY)?;
        let mut bytes = &data[..];
        Ok(u64::decode(&mut bytes)?)
    }
    pub fn reorg_delayed_to(&self, new_delayed_count: u64) -> anyhow::Result<()> {
        let mut batch = self.db.new_batch();
        self.set_delayed_count_reorg_and_write_batch(
            batch.as_mut(),
            new_delayed_count,
            new_delayed_count,
            true,
        )?;
        batch.write()?;
        Ok(())
    }

    pub fn add_sequencer_batches(&self, batches: &[SequencerInboxBatch]) -> anyhow::Result<()> {
        if batches.is_empty() {
            return Ok(());
        }
        let mut next_acc = B256::ZERO;
        let mut prev_meta = BatchMetadata {
            accumulator: B256::ZERO,
            message_count: 0,
            delayed_message_count: 0,
            parent_chain_block: 0,
        };
        let mut pos = batches[0].sequence_number;
        if pos > 0 {
            prev_meta = self.get_batch_metadata(pos - 1)?;
            next_acc = prev_meta.accumulator;
        }
        let mut db_batch = self.db.new_batch();
        crate::util::delete_starting_at(
            self.db.as_ref(),
            db_batch.as_mut(),
            DELAYED_SEQUENCED_PREFIX,
            &uint64_to_key(prev_meta.delayed_message_count + 1),
        )?;
        for b in batches {
            if b.sequence_number != pos {
                anyhow::bail!("unexpected batch sequence number {} expected {}", b.sequence_number, pos);
            }
            if next_acc != b.before_inbox_acc {
                anyhow::bail!("previous batch accumulator mismatch");
            }
            if b.after_delayed_count > 0 {
                let have_delayed = self.get_delayed_acc(b.after_delayed_count - 1).ok();
                if have_delayed != Some(b.after_delayed_acc) {
                    anyhow::bail!("delayed message accumulator doesn't match sequencer batch");
                }
            }
            next_acc = b.after_inbox_acc;
            pos += 1;
        }

        let mut last_meta = prev_meta.clone();
        let mut to_cache: Vec<(u64, BatchMetadata)> = Vec::with_capacity(batches.len());
        for b in batches {
            let msg_count_for_batch = last_meta.message_count + 1;
            let meta = BatchMetadata {
                accumulator: b.after_inbox_acc,
                message_count: msg_count_for_batch,
                delayed_message_count: b.after_delayed_count,
                parent_chain_block: b.parent_chain_block_number,
            };
            let mut meta_bytes = Vec::new();
            meta_bytes.extend_from_slice(&alloy_rlp::encode(&meta.accumulator));
            meta_bytes.extend_from_slice(&alloy_rlp::encode(&meta.message_count));
            meta_bytes.extend_from_slice(&alloy_rlp::encode(&meta.delayed_message_count));
            meta_bytes.extend_from_slice(&alloy_rlp::encode(&meta.parent_chain_block));
            let meta_key = db_key(SEQUENCER_BATCH_META_PREFIX, b.sequence_number);
            db_batch.put(&meta_key, &meta_bytes)?;

            if b.after_delayed_count > last_meta.delayed_message_count {
                let val = alloy_rlp::encode(&b.sequence_number);
                let key = db_key(DELAYED_SEQUENCED_PREFIX, b.after_delayed_count);
                db_batch.put(&key, &val)?;
            }

            to_cache.push((b.sequence_number, meta.clone()));
            last_meta = meta;
        }

        self.delete_batch_metadata_starting_at(pos)?;
        let enc = alloy_rlp::encode(&pos);
        db_batch.put(SEQUENCER_BATCH_COUNT_KEY, &enc)?;
        db_batch.write()?;

        let mut cache = self.batch_meta_cache.lock().unwrap();
        for (seq, meta) in to_cache {
            cache.put(seq, meta);
        }

        Ok(())
    }


    pub fn set_delayed_count_reorg_and_write_batch(
        &self,
        batch: &mut dyn crate::db::Batch,
        first_new_delayed_message_pos: u64,
        new_delayed_count: u64,
        can_reorg_batches: bool,
    ) -> anyhow::Result<()> {
        if first_new_delayed_message_pos > new_delayed_count {
            anyhow::bail!(
                "firstNewDelayedMessagePos {} is after newDelayedCount {}",
                first_new_delayed_message_pos,
                new_delayed_count
            );
        }

        crate::util::delete_starting_at(
            self.db.as_ref(),
            batch,
            RLP_DELAYED_MESSAGE_PREFIX,
            &uint64_to_key(new_delayed_count),
        )?;
        crate::util::delete_starting_at(
            self.db.as_ref(),
            batch,
            PARENT_CHAIN_BLOCK_NUMBER_PREFIX,
            &uint64_to_key(new_delayed_count),
        )?;
        crate::util::delete_starting_at(
            self.db.as_ref(),
            batch,
            LEGACY_DELAYED_MESSAGE_PREFIX,
            &uint64_to_key(new_delayed_count),
        )?;

        let enc = alloy_rlp::encode(&new_delayed_count);
        batch.put(DELAYED_MESSAGE_COUNT_KEY, &enc)?;

        let mut seq_iter = self
            .db
            .new_iterator(DELAYED_SEQUENCED_PREFIX, &uint64_to_key(first_new_delayed_message_pos + 1));
        let mut reorg_seq_batches_to_count: Option<u64> = None;
        while seq_iter.next() {
            let val = seq_iter.value();
            let mut bytes = val;
            let batch_seq_num: u64 = u64::decode(&mut bytes)?;
            if !can_reorg_batches {
                anyhow::bail!(
                    "reorging of sequencer batch number {} via delayed messages reorg to count {} disabled",
                    batch_seq_num,
                    new_delayed_count
                );
            }
            let key = seq_iter.key().to_vec();
            batch.delete(&key)?;
            if reorg_seq_batches_to_count.is_none() {
                reorg_seq_batches_to_count = Some(batch_seq_num);
            }
        }
        if let Some(err) = seq_iter.error() {
            return Err(err);
        }
        seq_iter.release();

        if let Some(count) = reorg_seq_batches_to_count {
            let count_enc = alloy_rlp::encode(&count);
            batch.put(SEQUENCER_BATCH_COUNT_KEY, &count_enc)?;
            self.delete_batch_metadata_starting_at(count)?;
        }

        Ok(())
    }

    pub fn add_delayed_messages(&self, messages: &[(u64, B256, Vec<u8>)], snap_sync: Option<&SnapSyncConfig>) -> anyhow::Result<()> {
        let mut next_acc = B256::ZERO;
        let mut msgs = messages;
        if msgs.is_empty() {
            return Ok(());
        }
        let mut pos = msgs[0].0;
        if let Some(cfg) = snap_sync {
            if cfg.enabled && pos < cfg.delayed_count {
                let mut first_keep = cfg.delayed_count;
                if first_keep > 0 { first_keep -= 1; }
                loop {
                    if msgs.is_empty() { return Ok(()); }
                    pos = msgs[0].0;
                    if pos + 1 == first_keep {
                        next_acc = msgs[0].1;
                    }
                    if pos < first_keep {
                        msgs = &msgs[1..];
                    } else {
                        break;
                    }
                }
            }
        }
        if pos > 0 {
            if let Ok(prev) = self.get_delayed_acc(pos - 1) {
                next_acc = prev;
            } else {
                anyhow::bail!("missing previous delayed message");
            }
        }
        let mut batch = self.db.new_batch();
        let first_pos = pos;
        for (seqnum, before_acc, msg_bytes) in msgs.iter() {
            if *seqnum != pos {
                anyhow::bail!("unexpected delayed sequence number {}, expected {}", seqnum, pos);
            }
            if next_acc != *before_acc {
                anyhow::bail!("previous delayed accumulator mismatch for message {}", seqnum);
            }
            let mut data = next_acc.0.to_vec();
            data.extend_from_slice(msg_bytes);
            let key = db_key(RLP_DELAYED_MESSAGE_PREFIX, *seqnum);
            batch.put(&key, &data)?;
            next_acc = hash_after(next_acc, msg_bytes);
            pos += 1;
        }
        self.set_delayed_count_reorg_and_write_batch(batch.as_mut(), first_pos, pos, true)?;
        batch.write()?;
        Ok(())
    }
    pub fn delete_batch_metadata_starting_at(&self, start_index: u64) -> anyhow::Result<()> {
        let mut cache = self.batch_meta_cache.lock().unwrap();
        let mut iter = self.db.new_iterator(SEQUENCER_BATCH_META_PREFIX, &uint64_to_key(start_index));
        let mut batch = self.db.new_batch();
        while iter.next() {
            let key = iter.key();
            batch.delete(key)?;
            let prefix_len = SEQUENCER_BATCH_META_PREFIX.len();
            if key.len() >= prefix_len + 8 {
                let mut idx_bytes = [0u8; 8];
                idx_bytes.copy_from_slice(&key[prefix_len..prefix_len + 8]);
                let idx = u64::from_be_bytes(idx_bytes);
                cache.pop(&idx);
            }
        }
        if let Some(err) = iter.error() {
            return Err(err);
        }
        iter.release();
        batch.write()?;
        Ok(())
    }

    pub fn find_inbox_batch_containing_message(&self, pos: u64) -> anyhow::Result<(u64, bool)> {
        let batch_count = self.get_batch_count()?;
        if batch_count == 0 {
            return Ok((0, false));
        }
        let mut low = 0u64;
        let mut high = batch_count - 1;
        let last_count = self.get_batch_message_count(high)?;
        if last_count <= pos {
            return Ok((0, false));
        }
        loop {
            let mid = (low + high) / 2;
            let count = self.get_batch_message_count(mid)?;
            if count < pos {
                low = mid + 1;
            } else if count == pos {
                return Ok((mid + 1, true));
            } else if count == pos + 1 || mid == low {
                return Ok((mid, true));
            } else {
                high = mid;
            }
            if high == low {
                return Ok((high, true));
            }
        }
    }

    pub fn legacy_get_delayed_message_and_accumulator(&self, seqnum: u64) -> anyhow::Result<(L1IncomingMessage, B256)> {
        let key = db_key(LEGACY_DELAYED_MESSAGE_PREFIX, seqnum);
        let data = self.db.get(&key)?;
        if data.len() < 32 {
            anyhow::bail!("delayed message legacy entry missing accumulator");
        }
        let acc = B256::from_slice(&data[..32]);
        let msg = parse_incoming_l1_message_legacy(&data[32..])?;
        Ok((msg, acc))
    }

    pub fn get_delayed_message_accumulator_and_parent_chain_block_number(&self, seqnum: u64) -> anyhow::Result<(L1IncomingMessage, B256, u64)> {
        let key = db_key(RLP_DELAYED_MESSAGE_PREFIX, seqnum);
        if self.db.has(&key)? {
            let data = self.db.get(&key)?;
            if data.len() < 32 {
                anyhow::bail!("delayed message new entry missing accumulator");
            }
            let acc = B256::from_slice(&data[..32]);
            let mut bytes = &data[32..];
            let msg: L1IncomingMessage = L1IncomingMessage::decode(&mut bytes)?;
            let pkey = db_key(PARENT_CHAIN_BLOCK_NUMBER_PREFIX, seqnum);
            let parent_block = if self.db.has(&pkey)? {
                let v = self.db.get(&pkey)?;
                if v.len() != 8 {
                    anyhow::bail!("invalid parent chain block number encoding");
                }
                u64::from_be_bytes(v.try_into().unwrap())
            } else {
                msg.header.block_number
            };
            Ok((msg, acc, parent_block))
        } else {
            let (msg, acc) = self.legacy_get_delayed_message_and_accumulator(seqnum)?;
            Ok((msg, acc, 0))
        }
    }

    pub fn get_delayed_message(&self, seqnum: u64) -> anyhow::Result<L1IncomingMessage> {
        let (msg, _, _) = self.get_delayed_message_accumulator_and_parent_chain_block_number(seqnum)?;
        Ok(msg)
    }

    pub fn get_delayed_message_bytes(&self, seqnum: u64) -> anyhow::Result<Vec<u8>> {
        let msg = self.get_delayed_message(seqnum)?;
        serialize_incoming_l1_message_legacy(&msg)
    }


}
