use alloy_primitives::{Address, B256};
use anyhow::Result;
use serde_json::json;

use crate::rpc::RpcClient;

pub async fn proxy_admin_address(rpc: &RpcClient, proxy: Address) -> Result<Address> {
    let slot_admin = B256::from_slice(&hex::decode("b53127684a568b3173ae13b9f8a6016e243e63b6e8ee1178d6a717850b5d6103").unwrap());
    let params = json!([
        format!("{:#x}", proxy),
        format!("{:#066x}", slot_admin), // 0x + 64 hex chars
        "latest"
    ]);
    let storage: String = rpc.call("eth_getStorageAt", params).await?;
    let bytes = hex::decode(storage.trim_start_matches("0x"))?;
    if bytes.len() < 32 {
        anyhow::bail!("short storage for proxy admin slot")
    }
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&bytes[12..32]);
    Ok(Address::from(addr))
}

pub async fn safe_from_for_proxy(rpc: &RpcClient, proxy: Address) -> Result<String> {
    let admin = proxy_admin_address(rpc, proxy).await.unwrap_or(Address::ZERO);
    let mut raw = [0u8; 20];
    raw[19] = 1;
    let mut candidate = Address::from(raw);
    if candidate == admin {
        raw[19] = 2;
        candidate = Address::from(raw);
    }
    Ok(format!("{:#x}", candidate))
}
