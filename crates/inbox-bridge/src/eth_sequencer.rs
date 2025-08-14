use crate::traits::SequencerInbox;
use crate::types::SequencerInboxBatch;
use alloy_primitives::{B256, Address};
use alloy_provider::{Provider, ProviderBuilder};
use async_trait::async_trait;
use std::sync::Arc;

pub struct EthSequencerInbox {
    provider: Arc<dyn Provider>,
    contract: Address,
}

impl EthSequencerInbox {
    pub async fn new_http(rpc_url: &str, contract: Address) -> anyhow::Result<Self> {
        let provider = ProviderBuilder::default().on_http(rpc_url.parse()?)?;
        Ok(Self { provider: Arc::new(provider), contract })
    }
}

#[async_trait]
impl SequencerInbox for EthSequencerInbox {
    async fn get_batch_count(&self, _block_number: u64) -> anyhow::Result<u64> {
        Err(anyhow::anyhow!("unimplemented"))
    }

    async fn get_accumulator(&self, _seq_num: u64, _block_number: u64) -> anyhow::Result<B256> {
        Err(anyhow::anyhow!("unimplemented"))
    }

    async fn lookup_batches_in_range(&self, _from_block: u64, _to_block: u64) -> anyhow::Result<Vec<SequencerInboxBatch>> {
        Err(anyhow::anyhow!("unimplemented"))
    }

    async fn get_sequencer_message_bytes_in_block(
        &self,
        _block_number: u64,
        _seq_num: u64,
    ) -> anyhow::Result<(Vec<u8>, B256, Vec<u64>)> {
        Err(anyhow::anyhow!("unimplemented"))
    }
}
