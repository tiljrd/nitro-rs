use nitro_batch_poster::poster::{BatchPoster, BatchPosterConfig};
use alloy_primitives::Address;

#[tokio::test]
async fn ignore_parent_chain_bound_returns_unbounded() {
    let cfg = BatchPosterConfig {
        enabled: true,
        use_4844: false,
        l1_rpc_url: "http://localhost:8545".to_string(),
        sequencer_inbox: Address::ZERO,
        parent_chain_bound: "ignore".to_string(),
        poster_private_key_hex: None,
    };

    let poster = BatchPoster::new(cfg).with_delayed_count_fn(std::sync::Arc::new(|| 1u64));
    let bytes = poster.build_batch_bytes().await.unwrap();
    assert!(bytes.is_some());
}
