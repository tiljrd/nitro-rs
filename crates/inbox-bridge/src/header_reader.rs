use crate::traits::{L1Header, L1HeaderReader};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::BlockNumberOrTag;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver};
use tokio::sync::Notify;
use tokio::time::{self, Duration};

pub struct HttpHeaderReader {
    provider: Arc<dyn Provider>,
    poll_interval_ms: u64,
    stop: Arc<Notify>,
}

impl HttpHeaderReader {
    pub async fn new_http(rpc_url: &str, poll_interval_ms: u64) -> Result<Self> {
        let provider = Arc::new(ProviderBuilder::default().connect(rpc_url).await?);
        Ok(Self { provider, poll_interval_ms, stop: Arc::new(Notify::new()) })
    }

    async fn block_number_tag(&self, tag: BlockNumberOrTag) -> Result<u64> {
        let block = self
            .provider
            .get_block_by_number(tag)
            .header_only()
            .await?
            .ok_or_else(|| anyhow::anyhow!("no block for tag"))?;
        Ok(block.header.number)
    }
}

#[async_trait]
impl L1HeaderReader for HttpHeaderReader {
    async fn last_header(&self) -> Result<L1Header> {
        let n = self.provider.get_block_number().await?;
        Ok(L1Header { number: n })
    }

    async fn latest_safe_block_nr(&self) -> Result<u64> {
        self.block_number_tag(BlockNumberOrTag::Safe).await
    }

    async fn latest_finalized_block_nr(&self) -> Result<u64> {
        self.block_number_tag(BlockNumberOrTag::Finalized).await
    }

    async fn subscribe(&self) -> (Receiver<L1Header>, Box<dyn FnOnce() + Send>) {
        let (tx, rx) = mpsc::channel(64);
        let provider = self.provider.clone();
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
                match provider.get_block_number().await {
                    Ok(n) => {
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
