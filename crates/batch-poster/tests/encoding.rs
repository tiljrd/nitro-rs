use nitro_batch_poster::poster::{BatchPoster, BatchPosterConfig};
use alloy_primitives::Address;

#[tokio::test]
async fn builds_header_and_brotli_prefixed_payload() {
    let cfg = BatchPosterConfig {
        enabled: true,
        use_4844: false,
        l1_rpc_url: "http://localhost:8545".to_string(),
        sequencer_inbox: Address::ZERO,
        parent_chain_bound: "latest".to_string(),
        poster_private_key_hex: None,
    };

    let poster = BatchPoster::new(cfg).with_delayed_count_fn(std::sync::Arc::new(|| 0u64));
    let bytes = poster.build_batch_bytes().await.unwrap().expect("should produce batch bytes");

    assert!(bytes.len() >= 41);
    assert_eq!(&bytes[0..8], &0u64.to_be_bytes());
    assert_eq!(&bytes[8..16], &u64::MAX.to_be_bytes());
    assert_eq!(&bytes[16..24], &0u64.to_be_bytes());
    assert_eq!(&bytes[24..32], &u64::MAX.to_be_bytes());
    assert_eq!(&bytes[32..40], &0u64.to_be_bytes());

    assert_eq!(bytes[40], 0x01, "brotli-compressed payload must be prefixed with 0x01");
}
