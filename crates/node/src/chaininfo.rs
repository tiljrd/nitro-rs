use serde::Deserialize;
use std::borrow::Cow;

#[derive(Debug, Deserialize)]
pub struct Chains(pub Vec<ChainInfo>);

#[derive(Debug, Deserialize)]
pub struct ChainInfo {
    #[serde(rename = "chain-id")]
    pub chain_id: Option<u64>,
    #[serde(rename = "chain-name")]
    pub chain_name: Option<String>,
    #[serde(rename = "chain-config")]
    pub chain_config: Option<serde_json::Value>,
    #[serde(rename = "rollup")]
    pub rollup: Option<RollupAddresses>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RollupAddresses {
    pub bridge: String,
    pub inbox: String,
    #[serde(rename = "sequencer-inbox")]
    pub sequencer_inbox: String,
    pub rollup: String,
    #[serde(rename = "deployed-at")]
    pub deployed_at: Option<u64>,
}

pub fn load_embedded() -> anyhow::Result<Chains> {
    let json: Cow<'static, str> = Cow::from(include_str!("chaininfo/arbitrum_chain_info.json"));
    let chains: Vec<ChainInfo> = serde_json::from_str(&json)?;
    Ok(Chains(chains))
}

impl Chains {
    pub fn select_by_chain_id(&self, chain_id: u64) -> Option<&ChainInfo> {
        self.0.iter().find(|c| c.chain_id == Some(chain_id))
    }
    pub fn select_by_name(&self, name: &str) -> Option<&ChainInfo> {
        self.0.iter().find(|c| c.chain_name.as_deref() == Some(name))
    }
}
