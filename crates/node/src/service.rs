use anyhow::Result;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;

use alloy_primitives::Address;

use crate::config::NodeArgs;

pub struct NitroNode {
    pub args: NodeArgs,
}

impl NitroNode {
    pub fn new(args: NodeArgs) -> Self {
        Self { args }
    }

    pub async fn start(self) -> Result<()> {
        info!("starting nitro-rs node; args: {:?}", self.args);

        let db_path = std::env::var("NITRO_DB_PATH").unwrap_or_else(|_| "./nitro-db".to_string());
        let db = Arc::new(nitro_db_sled::SledDb::open(&db_path)?);

        let streamer = Arc::new(nitro_streamer::streamer::TransactionStreamer::new(db.clone())) as Arc<dyn nitro_inbox::streamer::Streamer>;

        let tracker = Arc::new(nitro_inbox::tracker::InboxTracker::new(db.clone(), streamer.clone()));
        tracker.initialize()?;


        let l1_rpc = std::env::var("NITRO_L1_RPC").unwrap_or_else(|_| "http://localhost:8545".to_string());
        let header_reader = Arc::new(inbox_bridge::header_reader::HttpHeaderReader::new_http(&l1_rpc, 1000).await?);

        let delayed_bridge_addr_str = std::env::var("NITRO_DELAYED_BRIDGE")?;
        let sequencer_inbox_addr_str = std::env::var("NITRO_SEQUENCER_INBOX")?;
        let delayed_bridge_addr = Address::from_str(delayed_bridge_addr_str.trim())?;
        let sequencer_inbox_addr = Address::from_str(sequencer_inbox_addr_str.trim())?;

        let delayed_bridge = Arc::new(inbox_bridge::eth_delayed::EthDelayedBridge::new_http(
            &l1_rpc,
            delayed_bridge_addr,
        ).await?);

        let sequencer_inbox = Arc::new(inbox_bridge::eth_sequencer::EthSequencerInbox::new_http(
            &l1_rpc,
            sequencer_inbox_addr,
        ).await?);

        let reader_config: nitro_inbox_reader::reader::InboxReaderConfigFetcher = Arc::new(|| nitro_inbox_reader::reader::InboxReaderConfig::default());

        let first_msg_block: u64 = std::env::var("NITRO_FIRST_MESSAGE_BLOCK")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let inbox_reader = nitro_inbox_reader::reader::InboxReader::new(
            tracker.clone(),
            delayed_bridge.clone(),
            sequencer_inbox.clone(),
            header_reader.clone(),
            first_msg_block,
            reader_config.clone(),
        );

        let streamer = nitro_streamer::streamer::TransactionStreamer::new(db.clone());

        let reader_task = tokio::spawn(async move {
            let _ = inbox_reader.start().await;
        });

        let streamer_task = tokio::spawn(async move {
            let _ = streamer.start().await;
        });

        let _ = tokio::join!(reader_task, streamer_task);

        Ok(())
    }
}
