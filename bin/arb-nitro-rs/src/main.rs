use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let args_vec: Vec<String> = std::env::args().collect();
    if args_vec.iter().any(|a| a == "-h" || a == "--help" || a == "-V" || a == "--version") {
        let _ = nitro_node::config::NodeArgs::parse();
        return Ok(());
    }

    let mut filtered = Vec::new();
    let mut it = args_vec.into_iter();
    if let Some(bin) = it.next() {
        filtered.push(bin);
    }
    while let Some(arg) = it.next() {
        let is_supported = arg.starts_with("--network")
            || arg.starts_with("--sync-till-block")
            || arg == "--sequencer"
            || arg.starts_with("--sequencer")
            || arg.starts_with("--conf.file")
            || arg.starts_with("--rpc-host")
            || arg.starts_with("--rpc-port")
            || arg.starts_with("--ws-port")
            || arg.starts_with("--feed-enable")
            || arg.starts_with("--feed-port")
            || arg.starts_with("--poster-enable")
            || arg.starts_with("--poster-4844-enable")
            || arg.starts_with("--validator-enable");
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
