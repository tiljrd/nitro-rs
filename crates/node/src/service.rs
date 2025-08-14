use anyhow::Result;
use std::net::{SocketAddr, IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;
use tokio::io::AsyncWriteExt;
use nitro_batch_poster::poster::PosterService;
use nitro_validator::ValidatorService;

use alloy_primitives::Address;

use crate::config::NodeArgs;

use reth_node_core::node_config::NodeConfig;
use reth_node_core::args::RpcServerArgs;
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


    async fn start_feed_server(&self, enable: bool, port: u16, _tracker: Arc<nitro_inbox::tracker::InboxTracker<nitro_db_sled::SledDb>>) -> Result<()> {
        if !enable {
            return Ok(());
        }
        let addr: SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { continue };
                let _ = socket.writable().await;
                let _ = socket.try_write(b"READY\n");
                let _ = socket.set_nodelay(true);
                let mut buf = [0u8; 64];
                loop {
                    match socket.readable().await {
                        Ok(_) => {
                            match socket.try_read(&mut buf) {
                                Ok(0) => {
                                    let _ = socket.shutdown().await;
                                    break;
                                }
                                Ok(_) => {}
                                Err(_) => {
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        });
        Ok(())
    }

    pub async fn start(self) -> Result<()> {
        info!("starting nitro-rs node; args: {:?}", self.args);

        let db_path = std::env::var("NITRO_DB_PATH").unwrap_or_else(|_| "./nitro-db".to_string());
        let db = Arc::new(nitro_db_sled::SledDb::open(&db_path)?);

        let mut l1_rpc = std::env::var("NITRO_L1_RPC").unwrap_or_else(|_| "http://localhost:8545".to_string());
        let mut delayed_bridge_addr_opt: Option<Address> = None;
        let mut sequencer_inbox_addr_opt: Option<Address> = None;
        let mut poster_enable_cfg = self.args.poster_enable;
        let mut parent_chain_bound_cfg: Option<String> = None;
        let mut poster_privkey_cfg: Option<String> = std::env::var("NITRO_L1_POSTER_KEY").ok();
        let mut feed_enable_cfg = self.args.feed_enable;
        let mut feed_port_cfg = self.args.feed_port;
        let mut rpc_host_cfg = self.args.rpc_host.clone();
        let mut rpc_http_port_cfg = self.args.rpc_port;
        let mut rpc_ws_port_cfg = self.args.ws_port;

        if let Some(conf_path) = self.args.conf_file.clone() {
            if let Ok(text) = std::fs::read_to_string(&conf_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(url) = v.pointer("/parent-chain/connection/url").and_then(|x| x.as_str()) {
                        l1_rpc = url.to_string();
                    }
                    if let Some(b) = v.pointer("/node/batch-poster/enable").and_then(|x| x.as_bool()) {
                        poster_enable_cfg = b;
                    }
                    if let Some(s) = v.pointer("/node/batch-poster/l1-block-bound").and_then(|x| x.as_str()) {
                        parent_chain_bound_cfg = Some(s.to_string());
                    }
                    if let Some(k) = v.pointer("/node/batch-poster/parent-chain-wallet/private-key").and_then(|x| x.as_str()) {
                        poster_privkey_cfg = Some(k.to_string());
                    }
                    if let Some(b) = v.pointer("/node/feed/output/enable").and_then(|x| x.as_bool()) {
                        feed_enable_cfg = b;
                    }
                    if let Some(p) = v.pointer("/node/feed/output/port").and_then(|x| x.as_u64()) {
                        feed_port_cfg = p as u16;
                    }
                    if let Some(addr) = v.pointer("/http/addr").and_then(|x| x.as_str()) {
                        rpc_host_cfg = addr.to_string();
                    }
                    if let Some(addr) = v.pointer("/ws/addr").and_then(|x| x.as_str()) {
                        rpc_host_cfg = addr.to_string();
                    }
                    if let Some(p) = v.pointer("/http/port").and_then(|x| x.as_u64()) {
                        rpc_http_port_cfg = p as u16;
                    }
                    if let Some(p) = v.pointer("/ws/port").and_then(|x| x.as_u64()) {
                        rpc_ws_port_cfg = p as u16;
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

        let mut rpc = RpcServerArgs::default().with_http().with_ws();
        let http_ip: IpAddr = rpc_host_cfg.parse().unwrap_or(Ipv4Addr::UNSPECIFIED.into());
        rpc.http_addr = http_ip;
        rpc.http_port = rpc_http_port_cfg;
        rpc.ws_addr = http_ip;
        rpc.ws_port = rpc_ws_port_cfg;

        let arb_cfg = NodeConfig::test().with_rpc(rpc);
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

        let tracker = Arc::new(nitro_inbox::tracker::InboxTracker::new(db.clone(), streamer_trait.clone()));
        tracker.initialize()?;

        let first_msg_block: u64 = std::env::var("NITRO_FIRST_MESSAGE_BLOCK")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let _ = nitro_rpc::register_backend(tracker.clone(), streamer_impl.clone());

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

        let reader_task = tokio::spawn({
            let inbox_reader = inbox_reader;
            async move {
                let _ = inbox_reader.start().await;
            }
        });

        let feed_task = tokio::spawn({
            let this = self.clone_args();
            let tracker = tracker.clone();
            let enable = feed_enable_cfg;
            let port = feed_port_cfg;
            async move {
                let _ = this.start_feed_server(enable, port, tracker).await;
            }
        });


        let poster_task = if poster_enable_cfg {
            let poster_cfg = nitro_batch_poster::poster::BatchPosterConfig {
                enabled: true,
                use_4844: self.args.poster_4844_enable,
                l1_rpc_url: l1_rpc.clone(),
                sequencer_inbox: sequencer_inbox_addr,
                parent_chain_bound: parent_chain_bound_cfg.unwrap_or_else(|| "latest".to_string()),
                poster_private_key_hex: poster_privkey_cfg,
            };
            let delayed_fn = {
                let tracker = tracker.clone();
                std::sync::Arc::new(move || tracker.get_delayed_count().unwrap_or(0))
            };
            let poster = nitro_batch_poster::poster::BatchPoster::new(poster_cfg).with_delayed_count_fn(delayed_fn);
            Some(tokio::spawn(async move { let _ = poster.start().await; }))
        } else { None };

        let validator_task = if self.args.validator_enable {
            let validator = nitro_validator::Validator::new()
                .with_config(nitro_validator::ValidatorConfig { enabled: true });
            Some(tokio::spawn(async move { let _ = validator.start().await; }))
        } else { None };

        let _ = tokio::join!(reader_task, streamer_task, feed_task);
        if let Some(t) = poster_task { let _ = t.await; }
        if let Some(t) = validator_task { let _ = t.await; }

        Ok(())
    }

    fn clone_args(&self) -> Self {
        Self { args: self.args.clone() }
    }
}
