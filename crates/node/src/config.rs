use clap::Parser;

#[derive(Debug, Clone, Parser)]
pub struct NodeArgs {
    #[arg(long = "l1-rpc-url", env = "NITRO_L1_RPC")]
    pub l1_rpc_url: Option<String>,

    #[arg(long = "sequencer-inbox", env = "NITRO_SEQUENCER_INBOX")]
    pub sequencer_inbox: Option<String>,

    #[arg(long = "delayed-bridge", env = "NITRO_DELAYED_BRIDGE")]
    pub delayed_bridge: Option<String>,

    #[arg(long = "first-message-block", env = "NITRO_FIRST_MESSAGE_BLOCK")]
    pub first_message_block: Option<u64>,

    #[arg(long, env = "NITRO_CHAININFO_FILE")]
    pub chaininfo_file: Option<String>,

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

    #[arg(long = "db-path", env = "NITRO_DB_PATH", default_value = "./nitro-db")]
    pub db_path: String,

    #[arg(long = "beacon-url", env = "NITRO_BEACON_URL")]
    pub beacon_url: Option<String>,

    #[arg(long = "secondary-beacon-url", env = "NITRO_BEACON_URL_SECONDARY")]
    pub secondary_beacon_url: Option<String>,

    #[arg(long = "beacon-authorization", env = "NITRO_BEACON_AUTH")]
    pub beacon_authorization: Option<String>,

    #[arg(long = "beacon-blob-directory", env = "NITRO_BEACON_BLOB_DIR")]
    pub beacon_blob_directory: Option<String>,
}
