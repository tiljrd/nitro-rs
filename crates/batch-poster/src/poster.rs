use anyhow::Result;
use async_trait::async_trait;
use tokio::time::{interval, Duration};
use tracing::info;
use alloy_primitives::{Address, B256, U256};
use arb_alloy_util::l1_pricing::L1PricingState;
use alloy_provider::{Provider, ProviderBuilder};
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{sol, SolCall};
use std::str::FromStr;
use alloy_rpc_types::TransactionRequest;
use std::sync::{Arc, Mutex};

type DelayedCountFn = Arc<dyn Fn() -> u64 + Send + Sync>;

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
    delayed_count_fn: Option<DelayedCountFn>,
    last_after_delayed: Mutex<Option<u64>>,
}

impl BatchPoster {
    pub fn new(cfg: BatchPosterConfig) -> Self {
        Self { cfg, delayed_count_fn: None, last_after_delayed: Mutex::new(None) }
    }

    pub fn with_delayed_count_fn(mut self, f: DelayedCountFn) -> Self {
        self.delayed_count_fn = Some(f);
        self
    }

    async fn build_batch_bytes(&self) -> Result<Option<Vec<u8>>> {
        let after_delayed = match &self.delayed_count_fn {
            Some(f) => (f)(),
            None => 0,
        };
        {
            let mut guard = self.last_after_delayed.lock().unwrap();
            if guard.as_ref() == Some(&after_delayed) {
                return Ok(None);
            }
            *guard = Some(after_delayed);
        }
        let mut out: Vec<u8> = Vec::with_capacity(40 + 64);
        out.extend_from_slice(&0u64.to_be_bytes());
        out.extend_from_slice(&u64::MAX.to_be_bytes());
        out.extend_from_slice(&0u64.to_be_bytes());
        out.extend_from_slice(&u64::MAX.to_be_bytes());
        out.extend_from_slice(&after_delayed.to_be_bytes());
        Ok(Some(out))
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

    async fn post_to_l1(&self, data: &[u8]) -> Result<B256> {
        let key_hex = match &self.cfg.poster_private_key_hex {
            Some(k) => k.clone(),
            None => return Ok(B256::ZERO),
        };
        let signer = PrivateKeySigner::from_str(&key_hex)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.cfg.l1_rpc_url.parse()?);

        sol! {
            interface ISequencerInbox {
                function addSequencerL2BatchFromOrigin(uint256, uint256, bytes) external;
            }
        }

        let sequence_number = U256::ZERO;
        let gas_refunder = U256::ZERO;
        let calldata = ISequencerInbox::addSequencerL2BatchFromOriginCall {
            _0: sequence_number,
            _1: gas_refunder,
            _2: data.to_vec(),
        }
        .abi_encode();

        let mut tx = TransactionRequest::default();
        tx.to = Some(self.cfg.sequencer_inbox.into());
        tx.input = calldata.into();
        tx.value = Some(U256::ZERO);

        let pending = provider.send_transaction(tx).await?;
        let tx_hash = pending.tx_hash();
        Ok(*tx_hash)
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
