use anyhow::Result;
use tracing::info;

pub struct BatchPoster;

impl BatchPoster {
    pub fn new() -> Self { Self }
    pub async fn start(&self) -> Result<()> {
        info!("starting batch poster");
        Ok(())
    }
}
