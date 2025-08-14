use anyhow::Result;
use tracing::info;

use crate::config::NodeArgs;

pub struct NitroNode {
    pub args: NodeArgs,
}

impl NitroNode {
    pub fn new(args: NodeArgs) -> Self {
        Self { args }
    }

    pub async fn start(self) -> Result<()> {
        info!("starting nitro-rs node; args: {:?}", self.args);
        Ok(())
    }
}
