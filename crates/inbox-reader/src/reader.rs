use anyhow::Result;
use tracing::info;

pub struct InboxReader;

impl InboxReader {
    pub fn new() -> Self { Self }
    pub async fn start(&self) -> Result<()> {
        info!("starting inbox reader");
        Ok(())
    }
}
