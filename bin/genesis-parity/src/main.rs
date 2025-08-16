use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize)]
struct RpcBlock {
    #[serde(rename = "parentHash")]
    parent_hash: String,
    #[serde(rename = "hash")]
    hash: Option<String>,
    #[serde(rename = "baseFeePerGas")]
    base_fee_per_gas: Option<String>,
    #[serde(rename = "timestamp")]
    timestamp: String,
    #[serde(rename = "stateRoot")]
    state_root: String,
    #[serde(rename = "gasLimit")]
    gas_limit: String,
    #[serde(rename = "extraData")]
    extra_data: String,
    #[serde(rename = "nonce")]
    nonce: Option<String>,
    #[serde(rename = "mixHash")]
    mix_hash: Option<String>,
}

#[derive(Deserialize)]
struct Chains(Vec<ChainInfo>);

#[derive(Deserialize)]
struct ChainInfo {
    #[serde(rename = "chain-id")]
    chain_id: Option<u64>,
    #[serde(rename = "chain-name")]
    chain_name: Option<String>,
    #[serde(rename = "chain-config")]
    chain_config: Option<serde_json::Value>,
}

fn parse_hex_quantity_u256(s: Option<&str>) -> alloy_primitives::U256 {
    let s = s.unwrap_or("0x0");
    let mut h = s.strip_prefix("0x").unwrap_or(s).to_string();
    if h.is_empty() {
        return alloy_primitives::U256::ZERO;
    }
    if h.len() % 2 == 1 {
        h.insert(0, '0');
    }
    let bytes = alloy_primitives::hex::decode(&h).unwrap_or_default();
    alloy_primitives::U256::from_be_slice(&bytes)
}

fn parse_hex_quantity_u64(s: &str) -> u64 {
    u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0)
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let l2_rpc = args.next().unwrap_or_else(|| {
        "https://arb-sepolia.g.alchemy.com/v2/lC2HDPB2Vs7-p-UPkgKD-VqFulU5elyk".to_string()
    });
    let chain_id: u64 = 421_614;

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getBlockByNumber",
        "params": ["0x0", false]
    });
    let resp = client.post(&l2_rpc).json(&body).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("RPC error: {}", resp.status());
    }
    let v: serde_json::Value = resp.json().await?;
    let blk: RpcBlock = serde_json::from_value(v.get("result").cloned().ok_or_else(|| anyhow::anyhow!("no result"))?)?;

    if blk.parent_hash != "0x0000000000000000000000000000000000000000000000000000000000000000" {
        anyhow::bail!("Genesis parentHash not zero: {}", blk.parent_hash);
    }

    let json: &str = include_str!("../../../crates/node/src/chaininfo/arbitrum_chain_info.json");
    let chains: Chains = serde_json::from_str(json)?;
    let entry = chains.0.iter().find(|c| c.chain_id == Some(chain_id)).ok_or_else(|| anyhow::anyhow!("chain-id {} not found in embedded chaininfo", chain_id))?;
    let chain_cfg_str = entry.chain_config.as_ref().map(|v| v.to_string());

    let spec = reth_arbitrum_chainspec::sepolia_baked_genesis_from_header(
        chain_id,
        blk.base_fee_per_gas.as_deref().unwrap_or("0x0"),
        &blk.timestamp,
        &blk.state_root,
        &blk.gas_limit,
        &blk.extra_data,
        chain_cfg_str.as_deref(),
    )?;

    let g = spec.genesis();

    let mut ok = true;
    if g.nonce != 1 {
        eprintln!("Mismatch nonce: got {}, expected 1", g.nonce);
        ok = false;
    }
    if g.mix_hash != alloy_primitives::B256::ZERO {
        eprintln!("Mismatch mixHash: got {:?}", g.mix_hash);
        ok = false;
    }
    let rpc_ts = parse_hex_quantity_u64(&blk.timestamp);
    if g.timestamp != rpc_ts {
        eprintln!("Mismatch timestamp: got {}, expected {}", g.timestamp, rpc_ts);
        ok = false;
    }
    let rpc_gas_limit = parse_hex_quantity_u64(&blk.gas_limit);
    if g.gas_limit != rpc_gas_limit {
        eprintln!("Mismatch gasLimit: got {}, expected {}", g.gas_limit, rpc_gas_limit);
        ok = false;
    }
    let rpc_base_fee = parse_hex_quantity_u256(blk.base_fee_per_gas.as_deref());
    if g.base_fee_per_gas.map(alloy_primitives::U256::from) != Some(rpc_base_fee) {
        eprintln!("Mismatch baseFeePerGas: got {:?}, expected {:?}", g.base_fee_per_gas, rpc_base_fee);
        ok = false;
    }
    let rpc_extra = alloy_primitives::hex::decode(blk.extra_data.trim_start_matches("0x")).unwrap_or_default();
    if g.extra_data.as_ref() != rpc_extra.as_slice() {
        eprintln!("Mismatch extraData len={} expected_len={}", g.extra_data.len(), rpc_extra.len());
        ok = false;
    }

    if ok {
        println!("Genesis header parity OK against {}", l2_rpc);
        Ok(())
    } else {
        anyhow::bail!("Genesis header parity FAILED")
    }
}
