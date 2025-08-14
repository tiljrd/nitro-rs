use anyhow::Result;
use async_trait::async_trait;
use inbox_bridge::traits::{DelayedBridge, SequencerInbox, L1HeaderReader};
use nitro_inbox::tracker::InboxTracker;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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
    caught_up: bool,
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
            caught_up: false,
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
        info!("inbox reader starting");
        Ok(())
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

        let l1block = self.l1_reader.latest_finalized_block_nr().await?;
        self.recent_parent_chain_block_to_msg(l1block)
    }
}


impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    async fn get_next_block_to_read(&self) -> Result<u64> {
        let delayed_count = self.tracker.get_delayed_count()?;
        if delayed_count == 0 {
            return Ok(self.first_message_block);
        }
        let (_, parent_block) = self
impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    fn get_prev_block_for_reorg(&self, from: u64, max_blocks_backwards: u64) -> Result<u64> {
        if from <= self.first_message_block {
            anyhow::bail!("can't get older messages");
        }
        let new_from = from.saturating_sub(max_blocks_backwards).max(self.first_message_block);
        Ok(new_from)
    }
}

            .tracker
            .get_delayed_message_accumulator_and_parent_chain_block_number(delayed_count - 1)?;
        let msg_block = parent_block.max(self.first_message_block);
        Ok(msg_block)
    }
}
