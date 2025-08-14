use anyhow::Result;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;

use alloy_primitives::Address;

use crate::config::NodeArgs;

use reth_node_core::node_config::NodeConfig;
use reth_node_builder::NodeBuilder;
use reth_arbitrum_node::{ArbNode, args::RollupArgs};
use reth_tasks::TaskManager;

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

        let arb_cfg = NodeConfig::test();
        let builder = NodeBuilder::new(arb_cfg);
        let task_manager = TaskManager::current();
        let task_executor = task_manager.executor();
        let arb_handle = builder
            .testing_node(task_executor)
            .node(ArbNode::new(RollupArgs::default()))
            .launch()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let beacon_handle = arb_handle.node.add_ons_handle.beacon_engine_handle.clone();
        let payload_handle = arb_handle.node.payload_builder_handle.clone();

        let exec = crate::engine_adapter::RethExecEngine::new_with_handles(db.clone(), beacon_handle, payload_handle);
        let streamer_impl = Arc::new(nitro_streamer::streamer::TransactionStreamer::new(db.clone(), exec));
        let streamer_trait = streamer_impl.clone() as Arc<dyn nitro_inbox::streamer::Streamer>;

        let tracker = Arc::new(nitro_inbox::tracker::InboxTracker::new(db.clone(), streamer_trait.clone()));
        tracker.initialize()?;

        let mut l1_rpc = std::env::var("NITRO_L1_RPC").unwrap_or_else(|_| "http://localhost:8545".to_string());
        let mut delayed_bridge_addr_opt: Option<Address> = None;
        let mut sequencer_inbox_addr_opt: Option<Address> = None;

        if let Some(conf_path) = self.args.conf_file.clone() {
            if let Ok(text) = std::fs::read_to_string(&conf_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(url) = v.pointer("/parent-chain/connection/url").and_then(|x| x.as_str()) {
                        l1_rpc = url.to_string();
                    }
                    let info_files = v.pointer("/chain/info-files").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                    if let Some(info_path_val) = info_files.get(0).and_then(|x| x.as_str()) {
                        if let Ok(info_text) = std::fs::read_to_string(info_path_val) {
                            if let Ok(info) = serde_json::from_str::<serde_json::Value>(&info_text) {
                                if let Some(s) = info.get("bridge").and_then(|x| x.as_str()) {
                                    if let Ok(addr) = Address::from_str(s) { delayed_bridge_addr_opt = Some(addr); }
                                }
                                if let Some(s) = info.get("sequencerInbox").and_then(|x| x.as_str()) {
                                    if let Ok(addr) = Address::from_str(s) { sequencer_inbox_addr_opt = Some(addr); }
                                }
                                if delayed_bridge_addr_opt.is_none() {
                                    if let Some(s) = info.pointer("/contracts/Bridge/address").and_then(|x| x.as_str()) {
                                        if let Ok(addr) = Address::from_str(s) { delayed_bridge_addr_opt = Some(addr); }
                                    }
                                }
                                if sequencer_inbox_addr_opt.is_none() {
                                    if let Some(s) = info.pointer("/contracts/SequencerInbox/address").and_then(|x| x.as_str()) {
                                        if let Ok(addr) = Address::from_str(s) { sequencer_inbox_addr_opt = Some(addr); }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let header_reader = Arc::new(inbox_bridge::header_reader::HttpHeaderReader::new_http(&l1_rpc, 1000).await?);

        let delayed_bridge_addr = if let Some(a) = delayed_bridge_addr_opt {
            a
        } else {
            let delayed_bridge_addr_str = std::env::var("NITRO_DELAYED_BRIDGE")?;
            Address::from_str(delayed_bridge_addr_str.trim())?
        };
        let sequencer_inbox_addr = if let Some(a) = sequencer_inbox_addr_opt {
            a
        } else {
            let sequencer_inbox_addr_str = std::env::var("NITRO_SEQUENCER_INBOX")?;
            Address::from_str(sequencer_inbox_addr_str.trim())?
        };

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

        let streamer_task = tokio::spawn({
            let streamer_impl = streamer_impl.clone();
            async move {
                let _ = streamer_impl.start().await;
            }
        });

        let reader_task = tokio::spawn(async move {
            let _ = inbox_reader.start().await;
        });

        let _ = tokio::join!(reader_task, streamer_task);

        Ok(())
    }
}
