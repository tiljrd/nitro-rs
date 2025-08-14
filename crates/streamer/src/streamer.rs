use anyhow::Result;
use tracing::info;

pub struct TransactionStreamer;

impl TransactionStreamer {
    pub fn new() -> Self { Self }
    pub async fn start(&self) -> Result<()> {
        info!("starting transaction streamer");
        Ok(())
    }
}
