use anyhow::Result;
use async_trait::async_trait;
use inbox_bridge::traits::{DelayedBridge, SequencerInbox, L1HeaderReader};
use nitro_inbox::tracker::InboxTracker;
use nitro_primitives::l1::serialize_incoming_l1_message_legacy;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};

use tokio::sync::watch;
use tracing::info;

#[derive(Clone)]
pub struct InboxReaderConfig {
    pub delay_blocks: u64,
    pub check_delay_ms: u64,
    pub min_blocks_to_read: u64,
    pub default_blocks_to_read: u64,
    pub target_messages_read: u64,
    pub max_blocks_to_read: u64,
    pub read_mode: String,
}

impl Default for InboxReaderConfig {
    fn default() -> Self {
        Self {
            delay_blocks: 0,
            check_delay_ms: 60_000,
            min_blocks_to_read: 1,
            default_blocks_to_read: 100,
            target_messages_read: 500,
            max_blocks_to_read: 2000,
            read_mode: "latest".to_string(),
        }
    }
}

pub type InboxReaderConfigFetcher = Arc<dyn Fn() -> InboxReaderConfig + Send + Sync>;

pub struct InboxReader<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> {
    tracker: Arc<InboxTracker<D>>,
    delayed_bridge: Arc<B1>,
    sequencer_inbox: Arc<B2>,
    l1_reader: Arc<dyn L1HeaderReader>,
    first_message_block: u64,
    config: InboxReaderConfigFetcher,
    caught_up: AtomicBool,
    caught_up_tx: watch::Sender<bool>,
    caught_up_rx: watch::Receiver<bool>,
    last_seen_batch_count: AtomicU64,
    last_read_batch_count: AtomicU64,
}

impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    pub fn new(
        tracker: Arc<InboxTracker<D>>,
        delayed_bridge: Arc<B1>,
        sequencer_inbox: Arc<B2>,
        l1_reader: Arc<dyn L1HeaderReader>,
        first_message_block: u64,
        config: InboxReaderConfigFetcher,
    ) -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            tracker,
            delayed_bridge,
            sequencer_inbox,
            l1_reader,
            first_message_block,
            config,
            caught_up: AtomicBool::new(false),
            caught_up_tx: tx,
            caught_up_rx: rx,
            last_seen_batch_count: AtomicU64::new(0),
            last_read_batch_count: AtomicU64::new(0),
        }
    }

    pub fn caught_up_channel(&self) -> watch::Receiver<bool> {
        self.caught_up_rx.clone()
    }

    pub async fn start(&self) -> Result<()> {
        let read_mode = (self.config)().read_mode.clone();
        let mut from = self.get_next_block_to_read().await?;
        let (mut headers_rx, _unsubscribe) = self.l1_reader.subscribe().await;
        let mut blocks_to_fetch = {
            let cfg = (self.config)();
            cfg.default_blocks_to_read
        };
        info!("inbox_reader: starting loop with read_mode={}", read_mode);
        let mut seen_batch_count: u64 = 0;
        loop {
            let cfg = (self.config)();
            let mut current_height: u64 = 0;
            if read_mode != "latest" {
                let block_num = if read_mode == "safe" {
                    self.l1_reader.latest_safe_block_nr().await?
                } else {
                    self.l1_reader.latest_finalized_block_nr().await?
                };
                if block_num == 0 {
                    return Err(anyhow::anyhow!("unable to fetch latest {} block", read_mode));
                }
                current_height = block_num;
                if from > current_height + 1 {
                    from = current_height;
                }
                while current_height <= from {
                    tokio::select! {
                        v = headers_rx.recv() => {
                            if v.is_none() {
                                return Ok(());
                            }
                            let block_num2 = if read_mode == "safe" {
                                self.l1_reader.latest_safe_block_nr().await?
                            } else {
                                self.l1_reader.latest_finalized_block_nr().await?
                            };
                            if block_num2 == 0 {
                                return Err(anyhow::anyhow!("unable to fetch latest {} block", read_mode));
                            }
                            current_height = block_num2;
                        }
                    }
                }
            } else {
                info!("inbox_reader: latest header number={}", current_height);
                let latest = self.l1_reader.last_header().await?;
                current_height = latest.number;
                let needed_block_advance = cfg.delay_blocks + cfg.min_blocks_to_read.saturating_sub(1);
                let needed_block_height = from.saturating_add(needed_block_advance);
                let mut delay = tokio::time::interval(std::time::Duration::from_millis(cfg.check_delay_ms));
                delay.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    if current_height >= needed_block_height {
                        break;
                    }
                    tokio::select! {
                        v = headers_rx.recv() => {
                            if v.is_none() {
                                return Ok(());
                            }
                            if let Some(h) = v {
                                current_height = h.number;
                            }
                        }
                        _ = delay.tick() => {
                            break;
                        }
                    }
                }
                info!("inbox_reader: adjusted current_height after delay={} => {}", cfg.delay_blocks, current_height);
                if cfg.delay_blocks > 0 {
                    if current_height >= cfg.delay_blocks {
                        current_height -= cfg.delay_blocks;
                    } else {
                        current_height = 0;
                    }
                    if current_height < self.first_message_block {
                        current_height = self.first_message_block;
                    }
                }
                if from > current_height {
                    from = current_height;
                }
            }
            info!("inbox_reader: compute window from={} current_height={} blocks_to_fetch={}", from, current_height, blocks_to_fetch);

            let mut reorging_delayed = false;
            let mut reorging_sequencer = false;
            let mut missing_delayed = false;
            let mut missing_sequencer = false;

            {
                info!("inbox_reader: querying delayed.get_message_count at l1_height={}", current_height);
                let checking_delayed_count = match self.delayed_bridge.get_message_count(current_height).await {
                    Ok(v) => v,
                    Err(e) => {
                        info!("inbox_reader: delayed.get_message_count error: {}", e);
                        0
                    }
                };
                let mut checking_delayed_count = checking_delayed_count;
                let our_latest_delayed = self.tracker.get_delayed_count()?;
                if our_latest_delayed < checking_delayed_count {
                    checking_delayed_count = our_latest_delayed;
                    missing_delayed = true;
                } else if our_latest_delayed > checking_delayed_count {
                    self.tracker.reorg_delayed_to(checking_delayed_count)?;
                }
                if checking_delayed_count > 0 {
                    let checking_delayed_seq = checking_delayed_count - 1;
                    info!("inbox_reader: querying delayed.get_accumulator seq={} l1_height={}", checking_delayed_seq, current_height);
                    let l1_delayed_acc = match self.delayed_bridge.get_accumulator(checking_delayed_seq, current_height, alloy_primitives::B256::ZERO).await {
                        Ok(v) => v,
                        Err(e) => {
                            info!("inbox_reader: delayed.get_accumulator error: {}", e);
                            alloy_primitives::B256::ZERO
                        }
                    };
                    let db_delayed_acc = self.tracker.get_delayed_acc(checking_delayed_seq)?;
                    if db_delayed_acc != l1_delayed_acc {
                        reorging_delayed = true;
                    }
                }
                info!("inbox_reader: delayed our_latest={} l1_count={} reorging={} missing={}", our_latest_delayed, checking_delayed_count, reorging_delayed, missing_delayed);
            }

            let mut checking_batch_count: u64 = 0;
            info!("inbox_reader: querying sequencer.get_batch_count at l1_height={}", current_height);
            let seen_batch_res = self.sequencer_inbox.get_batch_count(current_height).await;
            match seen_batch_res {
                Ok(cnt) => {
                    seen_batch_count = cnt;
                    let our_latest_batch = self.tracker.get_batch_count()?;
                    if our_latest_batch < seen_batch_count {
                        missing_sequencer = true;
                    }
                    checking_batch_count = our_latest_batch.min(seen_batch_count);
                    if checking_batch_count > 0 {
                        let checking_batch_seq = checking_batch_count - 1;
                        info!("inbox_reader: querying sequencer.get_accumulator seq={} l1_height={}", checking_batch_seq, current_height);
                        let l1_batch_acc = match self.sequencer_inbox.get_accumulator(checking_batch_seq, current_height).await {
                            Ok(v) => v,
                            Err(e) => {
                                info!("inbox_reader: sequencer.get_accumulator error: {}", e);
                                alloy_primitives::B256::ZERO
                            }
                        };
                        let db_batch_acc = self.tracker.get_batch_acc(checking_batch_seq)?;
                        if db_batch_acc != l1_batch_acc {
                            reorging_sequencer = true;
                            info!("inbox_reader: sequencer reorg detected at seq={}", checking_batch_seq);
                        }
                    }
                    info!("inbox_reader: sequencer our_latest={} l1_seen={} checking={}", our_latest_batch, seen_batch_count, checking_batch_count);
                }
                Err(e) => {
                    seen_batch_count = self.tracker.get_batch_count()?;
                    checking_batch_count = seen_batch_count;
                    info!("inbox_reader: sequencer get_batch_count error: {}; using db count {}", e, seen_batch_count);
                    missing_sequencer = true;
                }
            }

            if !missing_delayed && !reorging_delayed && !missing_sequencer && !reorging_sequencer {
                from = current_height.saturating_add(1);
                blocks_to_fetch = cfg.default_blocks_to_read;
                info!("inbox_reader: nothing missing; advancing from -> {}", from);
                self.last_read_batch_count.store(checking_batch_count, Ordering::Relaxed);
                self.last_seen_batch_count.store(seen_batch_count, Ordering::Relaxed);
                if !self.caught_up.load(Ordering::Relaxed) && read_mode == "latest" {
                    self.caught_up.store(true, Ordering::Relaxed);
                    let _ = self.caught_up_tx.send(true);
                }
                continue;
            }

            let to_block = from
                .saturating_add(blocks_to_fetch)
                .min(current_height);
            info!("inbox_reader: to_block computed {}", to_block);
            let mut fetched_any = false;

            let mut delayed_len: u64 = 0;
            let mut batches_len: u64 = 0;

            if missing_delayed || reorging_delayed {
                let delayed = self
                    .delayed_bridge
                    .lookup_messages_in_range(from, to_block, |_b| Ok(Vec::new()))
                    .await?;
                if !delayed.is_empty() {
                    let mut tuples: Vec<(u64, alloy_primitives::B256, Vec<u8>)> = Vec::with_capacity(delayed.len());
                    for m in &delayed {
                        let bytes = serialize_incoming_l1_message_legacy(&m.message)?;
                        tuples.push((m.seq_num, m.before_inbox_acc, bytes));
                    }
                    self.tracker.add_delayed_messages(&tuples, None)?;
                    delayed_len = delayed.len() as u64;
                    info!("inbox_reader: fetched {} delayed messages", delayed.len());
                    fetched_any = true;
                }
            }

            if missing_sequencer || reorging_sequencer {
                let mut batches = self
                    .sequencer_inbox
                    .lookup_batches_in_range(from, to_block)
                    .await
                    .unwrap_or_default();
                if !batches.is_empty() {
                    for b in batches.iter_mut() {
                        let (bytes, block_hash) = {
                            let (data, blk_hash, _seen) = self
                                .sequencer_inbox
                                .get_sequencer_message_bytes_in_block(b.parent_chain_block_number, b.sequence_number)
                                .await?;
                            (data, blk_hash)
                        };
                        b.serialized = bytes;
                        if b.block_hash == alloy_primitives::B256::ZERO {
                            b.block_hash = block_hash;
                        }
                    }
                    self.tracker.add_sequencer_batches_and_stream(&batches)?;
                    batches_len = batches.len() as u64;
                    info!("inbox_reader: fetched {} sequencer batches", batches.len());
                    fetched_any = true;
                    seen_batch_count = seen_batch_count.max(batches.last().unwrap().sequence_number + 1);
                }
            }

            self.last_read_batch_count.store(checking_batch_count, Ordering::Relaxed);
            self.last_seen_batch_count.store(seen_batch_count, Ordering::Relaxed);

            let have_messages: u64 = delayed_len + batches_len;
            if have_messages <= (cfg.target_messages_read / 2) {
                blocks_to_fetch = blocks_to_fetch.saturating_add((blocks_to_fetch + 4) / 5);
            } else if have_messages >= (cfg.target_messages_read.saturating_mul(3) / 2) {
                blocks_to_fetch = blocks_to_fetch.saturating_sub((blocks_to_fetch + 4) / 5);
            }
            if blocks_to_fetch < 1 {
                blocks_to_fetch = 1;
            } else if blocks_to_fetch > cfg.max_blocks_to_read {
                blocks_to_fetch = cfg.max_blocks_to_read;
            }

            if reorging_delayed || reorging_sequencer {
                let prev = self.get_prev_block_for_reorg(from, blocks_to_fetch)?;
                from = prev;
                blocks_to_fetch = cfg.min_blocks_to_read;
            } else {
                from = to_block.saturating_add(1);
            }
        }
        #[allow(unreachable_code)]
        {
            let _ = _unsubscribe;
            Ok(())
        }
    }
}
impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    fn recent_parent_chain_block_to_msg(&self, parent_chain_block: u64) -> Result<u64> {
        let mut batch = self.tracker.get_batch_count()?;
        loop {
            if batch == 0 {
                return Ok(0);
            }
            batch -= 1;
            let meta = self.tracker.get_batch_metadata(batch)?;
            if meta.parent_chain_block <= parent_chain_block {
                return Ok(meta.message_count);
            }
        }
    }

    pub async fn get_safe_msg_count(&self) -> Result<u64> {
        let l1block = self.l1_reader.latest_safe_block_nr().await?;
        self.recent_parent_chain_block_to_msg(l1block)
    }

    pub async fn get_finalized_msg_count(&self) -> Result<u64> {
        let l1block = self.l1_reader.latest_finalized_block_nr().await?;
        self.recent_parent_chain_block_to_msg(l1block)
    }
}
impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    pub async fn get_sequencer_message_bytes(&self, seq_num: u64) -> Result<(Vec<u8>, alloy_primitives::B256)> {
        let metadata = self.tracker.get_batch_metadata(seq_num)?;
        let block_num = metadata.parent_chain_block;
        let (data, block_hash, _seen) = self
            .sequencer_inbox
            .get_sequencer_message_bytes_in_block(block_num, seq_num)
            .await?;
        Ok((data, block_hash))
    }
}



impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    async fn get_next_block_to_read(&self) -> Result<u64> {
        let delayed_count = self.tracker.get_delayed_count()?;
        if delayed_count == 0 {
            return Ok(self.first_message_block);
        }
        let (_, _, parent_block) = self
            .tracker
            .get_delayed_message_accumulator_and_parent_chain_block_number(delayed_count - 1)?;
        let msg_block = parent_block.max(self.first_message_block);
        Ok(msg_block)
    }
}

impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    fn get_prev_block_for_reorg(&self, from: u64, max_blocks_backwards: u64) -> Result<u64> {
        if from <= self.first_message_block {
            anyhow::bail!("can't get older messages");
        }
        let new_from = from.saturating_sub(max_blocks_backwards).max(self.first_message_block);
        Ok(new_from)
    }
}
impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    pub fn get_last_read_batch_count(&self) -> u64 {
        self.last_read_batch_count.load(Ordering::Relaxed)
    }

    pub fn get_last_seen_batch_count(&self) -> u64 {
        self.last_seen_batch_count.load(Ordering::Relaxed)
    }

    pub fn get_delay_blocks(&self) -> u64 {
        (self.config)().delay_blocks
    }
}
