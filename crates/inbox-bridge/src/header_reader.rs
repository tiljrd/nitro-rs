use crate::rpc::RpcClient;
use crate::traits::{L1Header, L1HeaderReader};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver};
use tokio::sync::Notify;
use tokio::time::{self, Duration};

pub struct HttpHeaderReader {
    rpc: Arc<RpcClient>,
    poll_interval_ms: u64,
    stop: Arc<Notify>,
}

impl HttpHeaderReader {
    pub async fn new_http(rpc_url: &str, poll_interval_ms: u64) -> Result<Self> {
        let rpc = Arc::new(RpcClient::new(rpc_url.to_string()));
        Ok(Self { rpc, poll_interval_ms, stop: Arc::new(Notify::new()) })
    }

    async fn block_number_tag(&self, tag: &str) -> Result<u64> {
        let block: serde_json::Value = self.rpc.call("eth_getBlockByNumber", json!([tag, false])).await?;
        let num_hex = block.get("number").and_then(|v| v.as_str()).unwrap_or("0x0");
        let n = u64::from_str_radix(num_hex.trim_start_matches("0x"), 16).unwrap_or(0);
        Ok(n)
    }
}

#[async_trait]
impl L1HeaderReader for HttpHeaderReader {
    async fn last_header(&self) -> Result<L1Header> {
        let hex: String = self.rpc.call("eth_blockNumber", []).await?;
        let n = u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0);
        Ok(L1Header { number: n })
    }

    async fn latest_safe_block_nr(&self) -> Result<u64> {
        self.block_number_tag("safe").await
    }

    async fn latest_finalized_block_nr(&self) -> Result<u64> {
        self.block_number_tag("finalized").await
    }

    async fn subscribe(&self) -> (Receiver<L1Header>, Box<dyn FnOnce() + Send>) {
        let (tx, rx) = mpsc::channel(64);
        let rpc = self.rpc.clone();
        let stop = self.stop.clone();
        let interval = self.poll_interval_ms;
        let unsub = {
            let stop = self.stop.clone();
            Box::new(move || stop.notify_waiters()) as Box<dyn FnOnce() + Send>
        };
        tokio::spawn(async move {
            let mut ticker = time::interval(Duration::from_millis(interval));
            let mut last: u64 = 0;
            loop {
                tokio::select! {
                    _ = ticker.tick() => {},
                    _ = stop.notified() => break,
                }
                match rpc.call::<String, _>("eth_blockNumber", []).await {
                    Ok(hex) => {
                        let n = u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0);
                        if n > last {
                            last = n;
                            let _ = tx.send(L1Header{ number: n }).await;
                        }
                    }
                    Err(_) => {}
                }
            }
        });
        (rx, unsub)
    }
}
