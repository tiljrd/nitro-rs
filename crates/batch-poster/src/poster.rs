use anyhow::Result;
use async_trait::async_trait;
use tokio::time::{interval, Duration};
use tracing::info;
use alloy_primitives::{Address, B256, U256};
use arb_alloy_util::l1_pricing::L1PricingState;
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::BlockNumberOrTag;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{sol, SolCall};
use std::str::FromStr;
use alloy_rpc_types::TransactionRequest;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentChainBound {
    Latest,
    Safe,
    Finalized,
    Ignore,
}

impl ParentChainBound {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "safe" => ParentChainBound::Safe,
            "finalized" => ParentChainBound::Finalized,
            "ignore" => ParentChainBound::Ignore,
            _ => ParentChainBound::Latest,
        }
    }
}

struct L1Bounds {
    min_ts: u64,
    max_ts: u64,
    min_block: u64,
    max_block: u64,
}
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

    pub async fn build_batch_bytes(&self) -> Result<Option<Vec<u8>>> {
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

        let bounds = self.resolve_l1_bounds().await?;

        let segments: Vec<Vec<u8>> = Vec::new();

        let mut out: Vec<u8> = Vec::with_capacity(40 + 128);
        out.extend_from_slice(&bounds.min_ts.to_be_bytes());
        out.extend_from_slice(&bounds.max_ts.to_be_bytes());
        out.extend_from_slice(&bounds.min_block.to_be_bytes());
        out.extend_from_slice(&bounds.max_block.to_be_bytes());
        out.extend_from_slice(&after_delayed.to_be_bytes());

        let mut concatenated: Vec<u8> = Vec::new();
        for seg in segments {
            let enc = alloy_rlp::encode(&seg);
            concatenated.extend_from_slice(&enc);
        }
        let mut payload: Vec<u8> = Vec::new();
        payload.push(0x01);
        let mut comp = Vec::new();
        {
            let mut params = brotli::enc::BrotliEncoderParams::default();
            params.quality = 6;
            brotli::BrotliCompress(&mut concatenated.as_slice(), &mut comp, &params)?;
        }
        payload.extend_from_slice(&comp);

        out.extend_from_slice(&payload);
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
    async fn resolve_l1_bounds(&self) -> Result<L1Bounds> {
        let bound = ParentChainBound::parse(&self.cfg.parent_chain_bound);
        if matches!(bound, ParentChainBound::Ignore) {
            return Ok(L1Bounds { min_ts: 0, max_ts: u64::MAX, min_block: 0, max_block: u64::MAX });
        }
        let provider = ProviderBuilder::new().connect_http(self.cfg.l1_rpc_url.parse()?);

        let tag = match bound {
            ParentChainBound::Latest => BlockNumberOrTag::Latest,
            ParentChainBound::Safe => BlockNumberOrTag::Safe,
            ParentChainBound::Finalized => BlockNumberOrTag::Finalized,
            ParentChainBound::Ignore => BlockNumberOrTag::Latest,
        };
        let block = match provider.get_block_by_number(tag).await {
            Ok(b) => b,
            Err(_) => return Ok(L1Bounds { min_ts: 0, max_ts: u64::MAX, min_block: 0, max_block: u64::MAX }),
        };
        let Some(block) = block else {
            return Ok(L1Bounds { min_ts: 0, max_ts: u64::MAX, min_block: 0, max_block: u64::MAX });
        };

        let max_block = block.number();
        let max_ts = block.header.inner.timestamp;

        Ok(L1Bounds { min_ts: 0, max_ts, min_block: 0, max_block })
    }
    async fn fetch_l1_base_fee_wei(&self) -> Option<u128> {
        let provider = ProviderBuilder::new().connect_http(self.cfg.l1_rpc_url.parse().ok()?);
        let block = provider.get_block_by_number(BlockNumberOrTag::Latest).await.ok()??;
        let fee = block.header.inner.base_fee_per_gas?;
        Some(fee as u128)
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
                function addSequencerL2BatchFromOrigin(
                    uint256 sequenceNumber,
                    bytes data,
                    uint256 afterDelayedMessagesRead,
                    address gasRefunder,
                    uint256 prevMessageCount,
                    uint256 newMessageCount
                ) external;
            }
        }

        let sequence_number = U256::ZERO;
        let after_delayed = {
            let guard = self.last_after_delayed.lock().unwrap();
            U256::from(guard.unwrap_or(0u64))
        };
        let gas_refunder: Address = Address::ZERO;
        let prev_message_count = U256::ZERO;
        let new_message_count = U256::ZERO;

        let calldata = ISequencerInbox::addSequencerL2BatchFromOriginCall {
            sequenceNumber: sequence_number,
            data: data.to_vec().into(),
            afterDelayedMessagesRead: after_delayed,
            gasRefunder: gas_refunder.into(),
            prevMessageCount: prev_message_count,
            newMessageCount: new_message_count,
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
                let base_fee = self.fetch_l1_base_fee_wei().await.unwrap_or(0);
                let _ = self.estimate_l1_cost(bytes.len(), base_fee);
                let _ = self.post_to_l1(&bytes).await?;
            }
        }
    }
}
