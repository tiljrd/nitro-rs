use crate::db::{Database};
use alloy_primitives::B256;
use nitro_primitives::dbkeys::*;
use nitro_primitives::accumulator::hash_after;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SnapSyncConfig {
    pub enabled: bool,
    pub delayed_count: u64,
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
        let mut dec = alloy_rlp::Decoder::new(&data);
        Ok(u64::decode(&mut dec)?)
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
        let mut dec = alloy_rlp::Decoder::new(&data);
        let acc: B256 = B256::decode(&mut dec)?;
        let msg_count: u64 = u64::decode(&mut dec)?;
        let delayed_count: u64 = u64::decode(&mut dec)?;
        let parent_block: u64 = u64::decode(&mut dec)?;
        let meta = BatchMetadata {
            accumulator: acc,
            message_count: msg_count,
            delayed_message_count: delayed_count,
            parent_chain_block: parent_block,
        };
        self.batch_meta_cache.lock().unwrap().push(seqnum, meta.clone());
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
        let mut dec = alloy_rlp::Decoder::new(&data);
        Ok(u64::decode(&mut dec)?)
    }

    pub fn set_delayed_count_reorg_and_write_batch(
        &self,
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
        let mut batch = self.db.new_batch();

        crate::util::delete_starting_at(
            self.db.as_ref(),
            batch.as_mut(),
            RLP_DELAYED_MESSAGE_PREFIX,
            &uint64_to_key(new_delayed_count),
        )?;
        crate::util::delete_starting_at(
            self.db.as_ref(),
            batch.as_mut(),
            PARENT_CHAIN_BLOCK_NUMBER_PREFIX,
            &uint64_to_key(new_delayed_count),
        )?;
        crate::util::delete_starting_at(
            self.db.as_ref(),
            batch.as_mut(),
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
            let mut dec = alloy_rlp::Decoder::new(val);
            let batch_seq_num: u64 = u64::decode(&mut dec)?;
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

        batch.write()?;
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
        let enc = alloy_rlp::encode(&pos);
        batch.put(DELAYED_MESSAGE_COUNT_KEY, &enc)?;
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
                cache.remove(&idx);
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
}

trait RlpDecodeExt: alloy_rlp::Decodable {}
impl RlpDecodeExt for u64 {}
impl RlpDecodeExt for B256 {}
