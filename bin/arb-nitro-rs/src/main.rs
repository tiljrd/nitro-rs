use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let mut filtered = Vec::new();
    let mut it = std::env::args().into_iter();
    if let Some(bin) = it.next() {
        filtered.push(bin);
    }
    while let Some(arg) = it.next() {
        let is_supported = arg.starts_with("--network")
            || arg.starts_with("--sync-till-block")
            || arg == "--sequencer"
            || arg.starts_with("--sequencer")
            || arg.starts_with("--conf.file");
        if is_supported {
            filtered.push(arg.clone());
            if !arg.contains('=') {
                if let Some(val) = it.next() {
                    if !val.starts_with('-') {
                        filtered.push(val);
                    }
                }
            }
        }
    }

    let args = nitro_node::config::NodeArgs::parse_from(filtered);
    let node = nitro_node::service::NitroNode::new(args);
    node.start().await
}
