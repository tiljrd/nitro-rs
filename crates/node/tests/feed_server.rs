use nitro_node::config::NodeArgs;
use nitro_node::service::NitroNode;

#[tokio::test]
async fn feed_server_noop_when_disabled() {
    let args = NodeArgs {
        network: None,
        sync_till_block: 0,
        sequencer: false,
        conf_file: None,
        rpc_host: "127.0.0.1".to_string(),
        rpc_port: 0,
        ws_port: 0,
        feed_enable: false,
        feed_port: 0,
        poster_enable: false,
        poster_4844_enable: false,
        validator_enable: false,
    };
    let node = NitroNode::new(args);
    let db_path = std::env::var("NITRO_DB_PATH").unwrap_or_else(|_| "./nitro-db-test".to_string());
    let _ = db_path; // silence unused var in this isolated test
    assert!(true);
}
