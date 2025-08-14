use anyhow::Result;
use async_trait::async_trait;
use tokio::time::{interval, Duration};

pub struct Validator;

impl Validator {
    pub fn new() -> Self { Self }
}

#[async_trait]
pub trait ValidatorService {
    async fn start(&self) -> Result<()>;
}

#[async_trait]
impl ValidatorService for Validator {
    async fn start(&self) -> Result<()> {
        let mut tick = interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
        }
    }
}
