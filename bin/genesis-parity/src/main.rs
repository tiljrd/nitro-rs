use anyhow::Result;
use serde::Deserialize;

use alloy_primitives::{self as ap, Address, B256, Bytes, U256, hex, keccak256};
use std::str::FromStr;

use std::collections::BTreeMap;

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

    fn extract_chain_config_raw_for(chain_id: u64) -> Option<String> {
        let text: &str = include_str!("../../../crates/node/src/chaininfo/arbitrum_chain_info.json");
        let bytes = text.as_bytes();
        let key_cfg = br#""chain-config""#;
        let mut pos = 0usize;

        fn find_json_number(buf: &[u8], key: &[u8]) -> Option<u64> {
            let mut i = 0usize;
            while i + key.len() < buf.len() {
                if &buf[i..i + key.len()] == key {
                    let mut j = i + key.len();
                    while j < buf.len() && (buf[j] == b' ' || buf[j] == b'\t' || buf[j] == b'\r' || buf[j] == b'\n' || buf[j] == b':' ) { j += 1; }
                    let mut val: u64 = 0;
                    let mut any = false;
                    while j < buf.len() {
                        let c = buf[j];
                        if c >= b'0' && c <= b'9' {
                            any = true;
                            val = val.saturating_mul(10).saturating_add((c - b'0') as u64);
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if any { return Some(val); }
                    return None;
                }
                i += 1;
            }
            None
        }

        while let Some(cfg_pos_rel) = text[pos..].find(core::str::from_utf8(key_cfg).ok()?) {
            let cfg_pos = pos + cfg_pos_rel;
            let colon = text[cfg_pos..].find(':')? + cfg_pos;
            let mut i = colon + 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() { i += 1; }
            if i >= bytes.len() || bytes[i] != b'{' { pos = cfg_pos + key_cfg.len(); continue; }
            let start_obj = i;
            let mut depth: i32 = 0;
            let mut j = i;
            while j < bytes.len() {
                let c = bytes[j];
                if c == b'{' { depth += 1; }
                if c == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        let obj_bytes = &bytes[start_obj..=j];
                        if let Some(val) = find_json_number(obj_bytes, br#""chainId""#) {
                            if val == chain_id {
                                let obj_str = &text[start_obj..=j];
                                return Some(obj_str.to_string());
                            }
                        }
                        break;
                    }
                }
                j += 1;
            }
            pos = j.saturating_add(1);
        }
        None
    }
    let chain_cfg_str = extract_chain_config_raw_for(chain_id);

    let arbos_addr = Address::from_str("0xA4B05FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF")?;
    let root_key: Vec<u8> = Vec::new();
    let l1_space = subspace(&root_key, 0u8);
    let l1_price_slot = map_slot(&l1_space, be_u64(7));
    let l1_price_slot_hex = format!("0x{}", hex::encode(l1_price_slot.as_slice()));
    let addr_hex = format!("{arbos_addr:#x}");
    let req = |method: &str, params: serde_json::Value| serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": method, "params": params
    });
    let resp_ppu = client.post(&l2_rpc)
        .json(&req("eth_getStorageAt", serde_json::json!([addr_hex, l1_price_slot_hex, "0x0"])))
        .send().await?;
    let ppu_val: String = resp_ppu.json::<serde_json::Value>().await?
        .get("result").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();

    let spec = reth_arbitrum_chainspec::sepolia_baked_genesis_from_header(
        chain_id,
        blk.base_fee_per_gas.as_deref().unwrap_or("0x0"),
        &blk.timestamp,
        &blk.state_root,
        &blk.gas_limit,
        &blk.extra_data,
        chain_cfg_str.as_deref(),
        Some(ppu_val.as_str()),
    )?;
    let g = spec.genesis();

    println!("Our computed stateRoot = {:?}", spec.genesis_header().state_root);

    let chain_cfg_bytes = chain_cfg_str.as_deref().map(|s| Bytes::from(s.as_bytes().to_vec()));
    let initial_l1_price = {
        let mut h = ppu_val.trim_start_matches("0x").to_string();
        if h.is_empty() { U256::ZERO } else {
            if h.len() % 2 == 1 { h.insert(0, '0'); }
            let bytes = hex::decode(&h).unwrap_or_default();
            U256::from_be_slice(&bytes)
        }
    };
    let our_storage: BTreeMap<B256, B256> = reth_arbitrum_chainspec::build_full_arbos_storage(
        chain_id,
        chain_cfg_bytes,
        initial_l1_price,
    );

    let addr_hex = format!("{arbos_addr:#x}");
    let mut mismatches = 0usize;

    let client_slot = client.clone();
    let l2_rpc_slot = l2_rpc.clone();
    let addr_hex_slot = addr_hex.clone();
    let fetch_slot = |slot: B256| {
        let client = client_slot.clone();
        let l2_rpc = l2_rpc_slot.clone();
        let addr_hex = addr_hex_slot.clone();
        async move {
            let slot_hex = format!("0x{}", hex::encode(slot.as_slice()));
            let body = serde_json::json!({
                "jsonrpc":"2.0","id":1,"method":"eth_getStorageAt",
                "params":[addr_hex, slot_hex, "0x0"]
            });
            let r = client.post(&l2_rpc).json(&body).send().await?;
            let v: serde_json::Value = r.json().await?;
            let s = v.get("result").and_then(|x| x.as_str()).unwrap_or("0x0").to_string();
            let mut h = s.trim_start_matches("0x").to_string();
            if h.len() % 2 == 1 { h.insert(0,'0'); }
            let bytes = hex::decode(&h).unwrap_or_default();
            let mut w = [0u8; 32];
            if bytes.len() <= 32 {
                w[32 - bytes.len()..].copy_from_slice(&bytes);
            }
            let got = B256::from(w);
            Ok::<B256, anyhow::Error>(got)
        }
    };

    let cc_space = subspace(&root_key, 7u8);
    let cc_len_slot = map_slot(&cc_space, be_u64(0));
    {
        let got = fetch_slot(cc_len_slot).await?;
        let expected = our_storage.get(&cc_len_slot).copied().unwrap_or(B256::ZERO);
        if got != expected {
            eprintln!("Mismatch CHAIN_CONFIG length @{} got={} expected={}",
                format!("0x{}", hex::encode(cc_len_slot.as_slice())),
                format!("0x{}", hex::encode(got.as_slice())),
                format!("0x{}", hex::encode(expected.as_slice()))
            );
            mismatches += 1;
        }
    }

    let mut total = 0usize;
    let mut first_n = 0usize;
    for (slot, expected) in our_storage.iter() {
        total += 1;
        let got = fetch_slot(*slot).await?;
        if got != *expected {
            if first_n < 50 {
                eprintln!(
                    "Mismatch slot={} got={} expected={}",
                    format!("0x{}", hex::encode(slot.as_slice())),
                    format!("0x{}", hex::encode(got.as_slice())),
                    format!("0x{}", hex::encode(expected.as_slice()))
                );
                first_n += 1;
            }
            mismatches += 1;
        }
    }
    println!("Checked {} ArbOS slots, {} mismatches", total, mismatches);

    let l1_space = subspace(&root_key, 0u8);
    let l1_price_slot = map_slot(&l1_space, be_u64(7));
    {
        let got = fetch_slot(l1_price_slot).await?;
        let expected = our_storage.get(&l1_price_slot).copied().unwrap_or(B256::ZERO);
        if got != expected {
            eprintln!("Mismatch L1 price-per-unit @{} got={} expected={}",
                format!("0x{}", hex::encode(l1_price_slot.as_slice())),
                format!("0x{}", hex::encode(got.as_slice())),
                format!("0x{}", hex::encode(expected.as_slice()))
            );
            mismatches += 1;
        }
    }

    for (slot, expected) in our_storage.iter() {
        let got = fetch_slot(*slot).await?;
        if got != *expected {
            eprintln!("Mismatch slot {} got={} expected={}",
                format!("0x{}", hex::encode(slot.as_slice())),
                format!("0x{}", hex::encode(got.as_slice())),
                format!("0x{}", hex::encode(expected.as_slice()))
            );
            mismatches += 1;
        }
    }


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
    let ours = format!("{:?}", spec.genesis_hash());
    let rpc_hash = blk.hash.clone().unwrap_or_default();
    if ours != rpc_hash {
        eprintln!("Mismatch genesis hash: ours={} rpc={}", ours, rpc_hash);
        ok = false;
    }
    let rpc_extra = alloy_primitives::hex::decode(blk.extra_data.trim_start_matches("0x")).unwrap_or_default();
    if g.extra_data.as_ref() != rpc_extra.as_slice() {
        eprintln!("Mismatch extraData len={} expected_len={}", g.extra_data.len(), rpc_extra.len());
        ok = false;
    }

    let arbos_addr = Address::from_str("0xA4B05FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF")?;
    let root_key: Vec<u8> = Vec::new();
    let l1_space = subspace(&root_key, 0u8);
    let l1_price_slot = map_slot(&l1_space, be_u64(7));
    let cc_space = subspace(&root_key, 7u8);
    let cc_len_slot = map_slot(&cc_space, be_u64(0));

    let l1_price_slot_hex = format!("0x{}", hex::encode(l1_price_slot.as_slice()));
    let cc_len_slot_hex = format!("0x{}", hex::encode(cc_len_slot.as_slice()));
    let addr_hex = format!("{arbos_addr:#x}");

    let req = |method: &str, params: serde_json::Value| serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": method, "params": params
    });

    let resp_ppu = client.post(&l2_rpc)
        .json(&req("eth_getStorageAt", serde_json::json!([addr_hex, l1_price_slot_hex, "0x0"])))
        .send().await?;
    let ppu_val: String = resp_ppu.json::<serde_json::Value>().await?
        .get("result").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();

    let resp_ccl = client.post(&l2_rpc)
        .json(&req("eth_getStorageAt", serde_json::json!([addr_hex, cc_len_slot_hex, "0x0"])))
        .send().await?;
    let ccl_val: String = resp_ccl.json::<serde_json::Value>().await?
        .get("result").and_then(|v| v.as_str()).unwrap_or("0x0").to_string();

    println!("RPC L1 price-per-unit @slot {} = {}", l1_price_slot_hex, ppu_val);
    let our_cc_compact_len = chain_cfg_str.as_ref().and_then(|s| {
        serde_json::from_str::<serde_json::Value>(s).ok().and_then(|v| serde_json::to_vec(&v).ok()).map(|b| b.len())
    }).unwrap_or_else(|| chain_cfg_str.as_ref().map(|s| s.len()).unwrap_or(0));
    println!("RPC CHAIN_CONFIG length @slot {} = {} (our compacted bytes len = {})",
        cc_len_slot_hex, ccl_val, our_cc_compact_len
    );

    println!("RPC stateRoot = {}", blk.state_root);
    if ok && mismatches == 0 {
        println!("Genesis header + ArbOS storage parity OK against {}", l2_rpc);
        Ok(())
    } else {
        anyhow::bail!("Genesis parity FAILED: header_ok={} storage_mismatches={}", ok, mismatches)
    }
}
