use anyhow::Result;
use alloy_primitives::U256;
use async_trait::async_trait;
use inbox_bridge::traits::{DelayedBridge, SequencerInbox};
use nitro_inbox::tracker::InboxTracker;
use std::sync::Arc;
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
    first_message_block: U256,
    config: InboxReaderConfigFetcher,
    caught_up_tx: watch::Sender<bool>,
    caught_up_rx: watch::Receiver<bool>,
}

impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    pub fn new(
        tracker: Arc<InboxTracker<D>>,
        delayed_bridge: Arc<B1>,
        sequencer_inbox: Arc<B2>,
        first_message_block: U256,
        config: InboxReaderConfigFetcher,
    ) -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            tracker,
            delayed_bridge,
            sequencer_inbox,
            first_message_block,
            config,
            caught_up_tx: tx,
            caught_up_rx: rx,
        }
    }

    pub fn caught_up_channel(&self) -> watch::Receiver<bool> {
        self.caught_up_rx.clone()
    }

    pub async fn start(&self) -> Result<()> {
        info!("inbox reader starting");
        Ok(())
impl<B1: DelayedBridge, B2: SequencerInbox, D: nitro_inbox::db::Database> InboxReader<B1, B2, D> {
    async fn get_next_block_to_read(&self) -> Result<u64> {
        let delayed_count = self.tracker.get_delayed_count()?;
        if delayed_count == 0 {
            return Ok(self.first_message_block.to());
        }
        let (_, parent_block) = self.tracker.get_delayed_message_accumulator_and_parent_chain_block_number(delayed_count - 1)?;
        Ok(parent_block)
    }
}

    }
}
