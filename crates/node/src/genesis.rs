use alloy_primitives::{Address, B256, U256};
use crate::chaininfo::ChainInfo;

pub struct GenesisBootstrap;

impl GenesisBootstrap {
    pub async fn build_spec_from_init_message<HB, HR>(
        _chaininfo: &ChainInfo,
        delayed_bridge: &HB,
        header_reader: &HR,
        deployed_at: u64,
    ) -> anyhow::Result<Option<reth_chainspec::ChainSpec>>
    where
        HB: inbox_bridge::traits::DelayedBridge + Send + Sync + ?Sized,
        HR: inbox_bridge::traits::L1HeaderReader + Send + Sync + ?Sized,
    {
        let from_block = deployed_at;
        let latest = header_reader.latest_finalized_block_nr().await.unwrap_or(from_block + 10_000);
        let to_block = std::cmp::min(latest, from_block.saturating_add(9_999));
        let fetcher = |_bn: u64| -> anyhow::Result<Vec<u8>> { Ok(Vec::new()) };
        let msgs =
            delayed_bridge.lookup_messages_in_range(from_block, to_block, fetcher).await?;
        let init = msgs.into_iter().find(|m| m.message.header.kind == 11u8).map(|m| m.message);
        let Some(init_msg) = init else { return Ok(None); };
        let parsed = nitro_primitives::l1::parse_init_message(&init_msg)?;

        let chain_id = parsed.chain_id_u64().unwrap_or(421_614u64);

        let mut genesis = alloy_genesis::Genesis::default();
        genesis.nonce = 1;
        genesis.difficulty = U256::from(1u64);
        genesis.timestamp = 0;
        genesis.gas_limit = 1u64 << 50;
        genesis.base_fee_per_gas = Some(parsed.initial_l1_base_fee.to::<u128>());
        genesis.mix_hash = B256::ZERO;
        genesis.coinbase = Address::ZERO;
        genesis.config.chain_id = chain_id.into();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0);

        let mut spec = reth_chainspec::ChainSpec::from_genesis(genesis);
        spec.chain = alloy_chains::Chain::from(chain_id);
        Ok(Some(spec))
    }
}
