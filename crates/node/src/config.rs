use clap::Parser;

#[derive(Debug, Clone, Parser)]
pub struct NodeArgs {
    #[arg(long, env = "NITRO_NETWORK")]
    pub network: Option<String>,

    #[arg(long, default_value_t = 0)]
    pub sync_till_block: u64,

    #[arg(long, default_value_t = false)]
    pub sequencer: bool,

    #[arg(long = "conf.file")]
    pub conf_file: Option<String>,

    #[arg(long, env = "NITRO_RPC_HOST", default_value = "0.0.0.0")]
    pub rpc_host: String,

    #[arg(long, env = "NITRO_RPC_PORT", default_value_t = 8547)]
    pub rpc_port: u16,

    #[arg(long, env = "NITRO_WS_PORT", default_value_t = 8548)]
    pub ws_port: u16,

    #[arg(long, env = "NITRO_FEED_ENABLE", default_value_t = true)]
    pub feed_enable: bool,

    #[arg(long, env = "NITRO_FEED_PORT", default_value_t = 9642)]
    pub feed_port: u16,

    #[arg(long, env = "NITRO_POSTER_ENABLE", default_value_t = false)]
    pub poster_enable: bool,

    #[arg(long, env = "NITRO_POSTER_4844_ENABLE", default_value_t = false)]
    pub poster_4844_enable: bool,

    #[arg(long, env = "NITRO_VALIDATOR_ENABLE", default_value_t = false)]
    pub validator_enable: bool,
}
