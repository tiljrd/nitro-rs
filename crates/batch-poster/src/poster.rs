use anyhow::Result;
use async_trait::async_trait;
use tokio::time::{interval, Duration};
use tracing::info;
use alloy_primitives::{Address, B256, U256};
use arb_alloy_util::l1_pricing::L1PricingState;

#[derive(Clone, Default)]
pub struct BatchPosterConfig {
    pub enabled: bool,
    pub use_4844: bool,
    pub l1_rpc_url: String,
    pub sequencer_inbox: Address,
    pub parent_chain_bound: String,
    pub poster_private_key_hex: Option<String>,
}

pub struct BatchPoster {
    cfg: BatchPosterConfig,
}

impl BatchPoster {
    pub fn new(cfg: BatchPosterConfig) -> Self {
        Self { cfg }
    }

    async fn build_batch_bytes(&self) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    fn brotli_compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut params = brotli::enc::BrotliEncoderParams::default();
        params.quality = 6;
        brotli::BrotliCompress(&mut &*data, &mut out, &params)?;
        Ok(out)
    }

    fn estimate_l1_cost(&self, brotli_len: usize, base_fee: u128) -> (u128, u128) {
        let pricing = L1PricingState { l1_base_fee_wei: base_fee };
        pricing.poster_data_cost_estimate_from_len(brotli_len as u64)
    }

    async fn post_to_l1(&self, _data: &[u8]) -> Result<B256> {
        Ok(B256::ZERO)
    }
}

#[async_trait]
pub trait PosterService {
    async fn start(&self) -> Result<()>;
}

#[async_trait]
impl PosterService for BatchPoster {
    async fn start(&self) -> Result<()> {
        if !self.cfg.enabled {
            return Ok(());
        }
        info!("starting batch poster");
        let mut tick = interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            if let Some(bytes) = self.build_batch_bytes().await? {
                let comp = self.brotli_compress(&bytes)?;
                let _ = self.estimate_l1_cost(comp.len(), 0);
                let _ = self.post_to_l1(&comp).await?;
            }
        }
    }
}
