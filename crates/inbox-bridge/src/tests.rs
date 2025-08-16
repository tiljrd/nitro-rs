use alloy_primitives::{keccak256, B256};

use crate::selectors::{
    EVT_SEQ_BATCH_DATA, EVT_SEQ_BATCH_DELIVERED, SIG_DELAYED_COUNT,
    SIG_DELAYED_INBOX_ACCS, SIG_INBOX_ACCS, SIG_BATCH_COUNT, SIG_SEND_L2_FROM_ORIGIN,
};

fn selector_hex(sig: &str) -> String {
    let h = keccak256(sig.as_bytes());
    format!("{:02x}{:02x}{:02x}{:02x}", h[0], h[1], h[2], h[3])
}

fn topic_hex(evt: &str) -> String {
    let h = keccak256(evt.as_bytes());
    format!("{:#x}", B256::from_slice(&h))
}

#[test]
fn test_function_selectors_match_expected() {
    assert_eq!(selector_hex(SIG_BATCH_COUNT), "06f13056");
    assert_eq!(selector_hex(SIG_INBOX_ACCS), "d9dd67ab");
    assert_eq!(selector_hex(SIG_DELAYED_COUNT), "eca067ad");
    assert_eq!(selector_hex(SIG_DELAYED_INBOX_ACCS), "d5719dc2");
    assert_eq!(selector_hex(SIG_SEND_L2_FROM_ORIGIN), "b680a2f8");
}

#[test]
fn test_sequencer_event_topics_are_stable() {
    assert_eq!(
        topic_hex(EVT_SEQ_BATCH_DELIVERED),
        "0x7394f4a19a13c7b92b5bb71033245305946ef78452f7b4986ac1390b5df4ebd7"
    );
    assert_eq!(
        topic_hex(EVT_SEQ_BATCH_DATA),
        "0xff64905f73a67fb594e0f940a8075a860db489ad991e032f48c81123eb52d60b"
    );
}
