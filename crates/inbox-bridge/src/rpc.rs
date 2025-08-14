use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct RpcClient {
    url: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct RpcReq<'a, T> {
    jsonrpc: &'a str,
    id: u64,
    method: &'a str,
    params: T,
}

#[derive(Deserialize)]
struct RpcResp<T> {
    jsonrpc: String,
    id: u64,
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

impl RpcClient {
    pub fn new(url: String) -> Self {
        Self { url, http: reqwest::Client::new() }
    }

    pub async fn call<T: for<'de> serde::Deserialize<'de>, P: serde::Serialize>(&self, method: &str, params: P) -> Result<T> {
        let req = RpcReq { jsonrpc: "2.0", id: 1, method, params };
        let resp = self.http.post(&self.url).json(&req).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("rpc http error {}: {}", status, text);
        }
        let parsed: RpcResp<T> = serde_json::from_str(&text)?;
        if let Some(err) = parsed.error {
            anyhow::bail!("rpc error {}: {}", err.code, err.message);
        }
        parsed.result.ok_or_else(|| anyhow::anyhow!("missing result"))
    }
}
