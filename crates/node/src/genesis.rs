use std::sync::Arc;
use alloy_primitives::{Address, B256, U256};
use crate::chaininfo::ChainInfo;
use nitro_inbox::db::Database;

pub struct GenesisBootstrap;

impl GenesisBootstrap {
    pub fn should_seed_genesis<DB: Database>(db: &DB) -> anyhow::Result<bool> {
        let have_head = db.has(&[b'h', b'c'].concat())?;
        Ok(!have_head)
    }

    pub async fn build_spec_from_init_message<HB, HR>(
        chaininfo: &ChainInfo,
        delayed_bridge: &Arc<HB>,
        header_reader: &Arc<HR>,
        deployed_at: u64,
    ) -> anyhow::Result<Option<reth_chainspec::ChainSpec>>
    where
        HB: inbox_bridge::traits::DelayedBridge + Send + Sync + ?Sized,
        HR: inbox_bridge::traits::L1HeaderReader + Send + Sync + ?Sized,
    {
        let from_block = deployed_at;
        let to_block = header_reader.latest_finalized_block_nr().await.unwrap_or(from_block + 10_000);
        let fetcher = |_bn: u64| -> anyhow::Result<Vec<u8>> { Ok(Vec::new()) };
        let msgs = delayed_bridge.lookup_messages_in_range(from_block, to_block, fetcher).await?;
        let init = msgs
            .into_iter()
            .find(|m| m.message.header.kind == 11u8)
            .map(|m| m.message);
        let Some(init_msg) = init else { return Ok(None); };
        let parsed = nitro_primitives::l1::parse_init_message(&init_msg)?;

        let chain_id = parsed.chain_id_u64().unwrap_or(421_614u64);

        let mut genesis = alloy_genesis::Genesis::default();
        genesis.nonce = 1;
        genesis.difficulty = 1;
        genesis.timestamp = 0;
        genesis.gas_limit = 1u64 << 50;
        genesis.base_fee_per_gas = Some(parsed.initial_l1_base_fee.to::<u64>());
        genesis.mix_hash = B256::ZERO;
        genesis.coinbase = Address::ZERO;
        genesis.alloc = alloy_genesis::Alloc::default();

        let hardforks = reth_ethereum_forks::EthereumHardfork::cancun().into();
        let header = reth_chainspec::make_genesis_header(&genesis, &hardforks);
        let sealed = reth_primitives_traits::SealedHeader::new(header, B256::ZERO);

        let spec = reth_chainspec::ChainSpec {
            chain: alloy_chains::Chain::from(chain_id),
            genesis_header: sealed,
            genesis,
            hardforks,
            ..Default::default()
        };
        Ok(Some(spec))
    }

    pub async fn seed_from_init_message<DB: Database, HB, SB, HR>(
        _db: &DB,
        _chaininfo: &ChainInfo,
        _delayed_bridge: &Arc<HB>,
        _sequencer_inbox: &Arc<SB>,
        _header_reader: &Arc<HR>,
        _deployed_at: u64,
    ) -> anyhow::Result<Option<B256>>
    where
        HB: inbox_bridge::traits::DelayedBridge + Send + Sync + ?Sized,
        SB: inbox_bridge::traits::SequencerInbox + Send + Sync + ?Sized,
        HR: inbox_bridge::traits::L1HeaderReader + Send + Sync + ?Sized,
    {
        Ok(None)
    }
}
