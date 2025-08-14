use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let args = nitro_node::config::NodeArgs::parse();
    let node = nitro_node::service::NitroNode::new(args);
    node.start().await
}
