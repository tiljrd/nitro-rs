use std::collections::BTreeMap;

use alloy_primitives::{keccak256, Address, Bytes, B256, U256};
use crate::chaininfo::ChainInfo;

pub struct GenesisBootstrap;

const ARBOS_ADDR: Address = alloy_primitives::address!("0xA4B05FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF");
const L2_GAS_LIMIT: u64 = 1u64 << 50;

const VERSION_OFFSET: u64 = 0;
const UPGRADE_VERSION_OFFSET: u64 = 1;
const UPGRADE_TIMESTAMP_OFFSET: u64 = 2;
const NETWORK_FEE_ACCOUNT_OFFSET: u64 = 3;
const CHAIN_ID_OFFSET: u64 = 4;
const GENESIS_BLOCK_NUM_OFFSET: u64 = 5;
const INFRA_FEE_ACCOUNT_OFFSET: u64 = 6;
const BROTLI_LEVEL_OFFSET: u64 = 7;
const NATIVE_TOKEN_ENABLED_FROM_TIME_OFFSET: u64 = 8;

const L1_PRICING_SUBSPACE: u8 = 0;
const L2_PRICING_SUBSPACE: u8 = 1;
const RETRYABLES_SUBSPACE: u8 = 2;
const ADDRESS_TABLE_SUBSPACE: u8 = 3;
const CHAIN_OWNER_SUBSPACE: u8 = 4;
const SEND_MERKLE_SUBSPACE: u8 = 5;
const BLOCKHASHES_SUBSPACE: u8 = 6;
const CHAIN_CONFIG_SUBSPACE: u8 = 7;

const L2_SPEED_LIMIT_PER_SECOND_OFFSET: u64 = 0;
const L2_PER_BLOCK_GAS_LIMIT_OFFSET: u64 = 1;
const L2_BASE_FEE_WEI_OFFSET: u64 = 2;
const L2_MIN_BASE_FEE_WEI_OFFSET: u64 = 3;
const L2_GAS_BACKLOG_OFFSET: u64 = 4;
const L2_PRICING_INERTIA_OFFSET: u64 = 5;
const L2_BACKLOG_TOLERANCE_OFFSET: u64 = 6;

const INITIAL_SPEED_LIMIT_PER_SECOND_V0: u64 = 1_000_000;
const INITIAL_PER_BLOCK_GAS_LIMIT_V0: u64 = 20 * 1_000_000;
const INITIAL_MIN_BASE_FEE_WEI: u128 = 100_000_000; // 0.1 gwei
const INITIAL_BASE_FEE_WEI: u128 = INITIAL_MIN_BASE_FEE_WEI;
const INITIAL_PRICING_INERTIA: u64 = 102;
const INITIAL_BACKLOG_TOLERANCE: u64 = 10;

fn be_u256(val: U256) -> B256 {
    B256::from(val.to_be_bytes::<32>())
}

fn be_u64(val: u64) -> B256 {
    be_u256(U256::from(val))
}

fn map_slot(storage_key: &[u8], key: B256) -> B256 {
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(key.as_slice());
    let boundary = 31usize;
    let mut mapped = [0u8; 32];
    let hashed = keccak256([storage_key, &key_bytes[..boundary]].concat());
    mapped[..boundary].copy_from_slice(&hashed[..boundary]);
    mapped[boundary] = key_bytes[boundary];
    B256::from(mapped)
}

fn subspace(storage_key: &[u8], id: u8) -> Vec<u8> {
    keccak256([storage_key, &[id]].concat()).to_vec()
}

fn write_bytes(storage: &mut BTreeMap<B256, B256>, storage_key: &[u8], bytes: &[u8]) {
    storage.insert(map_slot(storage_key, be_u64(0)), be_u64(bytes.len() as u64));
    let mut offset = 1u64;
    let mut i = 0usize;
    while i + 32 <= bytes.len() {
        let word = B256::from_slice(&bytes[i..i + 32]);
        storage.insert(map_slot(storage_key, be_u64(offset)), word);
        offset += 1;
        i += 32;
    }
    if i < bytes.len() {
        let mut last = [0u8; 32];
        let rem = &bytes[i..];
        last[32 - rem.len()..].copy_from_slice(rem);
        storage.insert(map_slot(storage_key, be_u64(offset)), B256::from(last));
    }
}

fn build_minimal_arbos_storage(
    chain_id: u64,
    chain_config_bytes: Option<Bytes>,
    initial_l1_base_fee: U256,
) -> BTreeMap<B256, B256> {
    let mut storage = BTreeMap::<B256, B256>::new();
    let root_key: Vec<u8> = Vec::new();

    storage.insert(map_slot(&root_key, be_u64(VERSION_OFFSET)), be_u64(1));
    storage.insert(map_slot(&root_key, be_u64(UPGRADE_VERSION_OFFSET)), be_u64(0));
    storage.insert(map_slot(&root_key, be_u64(UPGRADE_TIMESTAMP_OFFSET)), be_u64(0));
    storage.insert(map_slot(&root_key, be_u64(NETWORK_FEE_ACCOUNT_OFFSET)), B256::ZERO);
    storage.insert(map_slot(&root_key, be_u64(CHAIN_ID_OFFSET)), be_u256(U256::from(chain_id)));
    storage.insert(map_slot(&root_key, be_u64(GENESIS_BLOCK_NUM_OFFSET)), be_u64(0));
    storage.insert(map_slot(&root_key, be_u64(INFRA_FEE_ACCOUNT_OFFSET)), B256::ZERO);
    storage.insert(map_slot(&root_key, be_u64(BROTLI_LEVEL_OFFSET)), be_u64(0));
    storage.insert(map_slot(&root_key, be_u64(NATIVE_TOKEN_ENABLED_FROM_TIME_OFFSET)), be_u64(0));

    if let Some(cfg) = chain_config_bytes {
        let cc_space = subspace(&root_key, CHAIN_CONFIG_SUBSPACE);
        write_bytes(&mut storage, &cc_space, &cfg);
    }

    let l2_space = subspace(&root_key, L2_PRICING_SUBSPACE);
    storage.insert(
        map_slot(&l2_space, be_u64(L2_SPEED_LIMIT_PER_SECOND_OFFSET)),
        be_u64(INITIAL_SPEED_LIMIT_PER_SECOND_V0),
    );
    storage.insert(
        map_slot(&l2_space, be_u64(L2_PER_BLOCK_GAS_LIMIT_OFFSET)),
        be_u64(INITIAL_PER_BLOCK_GAS_LIMIT_V0),
    );
    storage.insert(
        map_slot(&l2_space, be_u64(L2_BASE_FEE_WEI_OFFSET)),
        be_u256(U256::from(INITIAL_BASE_FEE_WEI)),
    );
    storage.insert(
        map_slot(&l2_space, be_u64(L2_MIN_BASE_FEE_WEI_OFFSET)),
        be_u256(U256::from(INITIAL_MIN_BASE_FEE_WEI)),
    );
    storage.insert(map_slot(&l2_space, be_u64(L2_GAS_BACKLOG_OFFSET)), be_u64(0));
    storage.insert(
        map_slot(&l2_space, be_u64(L2_PRICING_INERTIA_OFFSET)),
        be_u64(INITIAL_PRICING_INERTIA),
    );
    storage.insert(
        map_slot(&l2_space, be_u64(L2_BACKLOG_TOLERANCE_OFFSET)),
        be_u64(INITIAL_BACKLOG_TOLERANCE),
    );

    let l1_space = subspace(&root_key, L1_PRICING_SUBSPACE);
    let PRICE_PER_UNIT_OFFSET: u64 = 7;
    storage.insert(
        map_slot(&l1_space, be_u64(PRICE_PER_UNIT_OFFSET)),
        be_u256(initial_l1_base_fee),
    );

    storage
}

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
        let latest = if let Ok(n) = header_reader.latest_safe_block_nr().await {
            n
        } else if let Ok(n) = header_reader.latest_finalized_block_nr().await {
            n
        } else {
            from_block + 100_000
        };
        tracing::info!(target: "genesis_bootstrap", "received deployed_at param={}", deployed_at);

        tracing::info!(target: "genesis_bootstrap", "init scan configured: deployed_at={} latest={}", from_block, latest);
        let fetcher = |_bn: u64| -> anyhow::Result<Vec<u8>> { Ok(Vec::new()) };

        let mut init_msg_opt: Option<nitro_primitives::l1::L1IncomingMessage> = None;
        let mut start = from_block;
        while start <= latest {
            let end = start.saturating_add(9_999).min(latest);
            tracing::info!(target: "genesis_bootstrap", "scanning for init message: from={} to={}", start, end);
            let msgs = delayed_bridge.lookup_messages_in_range(start, end, fetcher).await?;
            tracing::info!(target: "genesis_bootstrap", "window {}-{} yielded {} delayed messages", start, end, msgs.len());
            if !msgs.is_empty() {
                let kinds: Vec<u8> = msgs.iter().map(|m| m.message.header.kind).collect();
                tracing::info!(target: "genesis_bootstrap", "found {} delayed messages in window; kinds={:?}", msgs.len(), kinds);
                if let Some(found) = msgs.into_iter().find(|m| m.message.header.kind == 11u8) {
                    tracing::info!(target: "genesis_bootstrap", "found init message (kind=11) in window {}-{}", start, end);
                    init_msg_opt = Some(found.message);
                    break;
                }
            }
            if end == latest {
                break;
            }
            start = end.saturating_add(1);
        }

        let Some(init_msg) = init_msg_opt else { return Ok(None); };
        let parsed = nitro_primitives::l1::parse_init_message(&init_msg)?;

        let chain_id = parsed.chain_id_u64().unwrap_or(421_614u64);

        let mut genesis = alloy_genesis::Genesis::default();
        genesis.nonce = 1;
        genesis.difficulty = U256::from(1u64);
        genesis.timestamp = 0;
        genesis.gas_limit = L2_GAS_LIMIT;
        genesis.base_fee_per_gas = Some(INITIAL_BASE_FEE_WEI as u128);
        genesis.mix_hash = B256::ZERO;
        genesis.coinbase = Address::ZERO;
        genesis.config.chain_id = chain_id.into();
        genesis.config.london_block = Some(0);
        genesis.config.cancun_time = Some(0);

        let alloc_storage = build_minimal_arbos_storage(
            chain_id,
            parsed.chain_config_json.clone().map(Bytes::from),
            parsed.initial_l1_base_fee,
        );
        if !alloc_storage.is_empty() {
            let mut acct = alloy_genesis::GenesisAccount::default()
                .with_nonce(Some(1))
                .with_balance(U256::ZERO)
                .with_code(None)
                .with_storage(Some(alloc_storage.into_iter().collect()));
            let mut map = BTreeMap::new();
            map.insert(ARBOS_ADDR, acct);
            genesis.alloc = map;
        }

        let mut spec = reth_chainspec::ChainSpec::from_genesis(genesis);
        spec.chain = alloy_chains::Chain::from(chain_id);
        Ok(Some(spec))
    }
}
