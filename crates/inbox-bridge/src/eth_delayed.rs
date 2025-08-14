use crate::traits::DelayedBridge;
use crate::types::DelayedInboxMessage;
use alloy_primitives::{B256, Address};
use alloy_provider::{Provider, ProviderBuilder};
use async_trait::async_trait;
use std::sync::Arc;

pub struct EthDelayedBridge {
    provider: Arc<dyn Provider>,
    contract: Address,
}

impl EthDelayedBridge {
    pub async fn new_http(rpc_url: &str, contract: Address) -> anyhow::Result<Self> {
        let provider = ProviderBuilder::default().on_http(rpc_url.parse()?)?;
        Ok(Self { provider: Arc::new(provider), contract })
    }
}

#[async_trait]
impl DelayedBridge for EthDelayedBridge {
    async fn get_message_count(&self, _block_number: u64) -> anyhow::Result<u64> {
        Err(anyhow::anyhow!("unimplemented"))
    }

    async fn get_accumulator(&self, _seq_num: u64, _block_number: u64, _block_hash: B256) -> anyhow::Result<B256> {
        Err(anyhow::anyhow!("unimplemented"))
    }

    async fn lookup_messages_in_range<F>(
        &self,
        _from_block: u64,
        _to_block: u64,
        _batch_fetcher: F,
    ) -> anyhow::Result<Vec<DelayedInboxMessage>>
    where
        F: Fn(u64) -> anyhow::Result<Vec<u8>> + Send + Sync,
    {
        Err(anyhow::anyhow!("unimplemented"))
    }
}
