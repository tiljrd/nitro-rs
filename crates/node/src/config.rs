use clap::Parser;

#[derive(Debug, Clone, Parser)]
pub struct NodeArgs {
    #[arg(long, env = "NITRO_NETWORK")]
    pub network: Option<String>,

    #[arg(long, default_value_t = 0)]
    pub sync_till_block: u64,

    #[arg(long, default_value_t = false)]
    pub sequencer: bool,
}
