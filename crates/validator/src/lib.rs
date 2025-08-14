use anyhow::Result;
use async_trait::async_trait;
use tokio::time::{interval, Duration};
use tracing::info;

#[derive(Clone, Default)]
pub struct ValidatorConfig {
    pub enabled: bool,
}

pub struct Validator {
    pub cfg: ValidatorConfig,
}

impl Validator {
    pub fn new() -> Self { Self { cfg: ValidatorConfig::default() } }
    pub fn with_config(mut self, cfg: ValidatorConfig) -> Self { self.cfg = cfg; self }
}

#[async_trait]
pub trait ValidatorService {
    async fn start(&self) -> Result<()>;
}

#[async_trait]
impl ValidatorService for Validator {
    async fn start(&self) -> Result<()> {
        if !self.cfg.enabled {
            return Ok(());
        }
        info!("starting validator");
        let mut tick = interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
        }
    }
}
