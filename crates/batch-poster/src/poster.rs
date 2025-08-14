use anyhow::Result;
use async_trait::async_trait;
use tokio::time::{interval, Duration};
use tracing::info;

pub struct BatchPoster;

impl BatchPoster {
    pub fn new() -> Self { Self }
}

#[async_trait]
pub trait PosterService {
    async fn start(&self) -> Result<()>;
}

#[async_trait]
impl PosterService for BatchPoster {
    async fn start(&self) -> Result<()> {
        info!("starting batch poster");
        let mut tick = interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
        }
    }
}
