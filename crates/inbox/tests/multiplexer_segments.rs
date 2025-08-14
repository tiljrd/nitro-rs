use alloy_rlp::encode;
use nitro_inbox::multiplexer::parse_sequencer_message;
use brotli::CompressorWriter;
use std::io::Write;

fn header(min_ts: u64, max_ts: u64, min_l1: u64, max_l1: u64, after_delayed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(40);
    out.extend_from_slice(&min_ts.to_be_bytes());
    out.extend_from_slice(&max_ts.to_be_bytes());
    out.extend_from_slice(&min_l1.to_be_bytes());
    out.extend_from_slice(&max_l1.to_be_bytes());
    out.extend_from_slice(&after_delayed.to_be_bytes());
    out
}

#[test]
fn parses_uncompressed_rlp_segments() {
    let mut data = header(10, 20, 100, 200, 0);
    let seg1 = {
        let mut v = Vec::new();
        v.push(0u8);
        v.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        v
    };
    let seg2 = {
        let mut v = Vec::new();
        v.push(0u8);
        v.extend_from_slice(&[0x01, 0x02]);
        v
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(&encode(&seg1));
    payload.extend_from_slice(&encode(&seg2));
    data.extend_from_slice(&payload);

    let parsed = parse_sequencer_message(0, None, &data).expect("parse ok");
    assert_eq!(parsed.min_timestamp, 10);
    assert_eq!(parsed.max_timestamp, 20);
    assert_eq!(parsed.min_l1_block, 100);
    assert_eq!(parsed.max_l1_block, 200);
    assert_eq!(parsed.after_delayed_messages, 0);
    assert_eq!(parsed.segments.len(), 2);
    assert_eq!(parsed.segments[0], seg1);
    assert_eq!(parsed.segments[1], seg2);
}

#[test]
fn parses_brotli_compressed_segments() {
    let mut data = header(1, 2, 3, 4, 0);

    let seg = {
        let mut v = Vec::new();
        v.push(0u8);
        v.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        v
    };
    let mut rlp_concat = Vec::new();
    rlp_concat.extend_from_slice(&encode(&seg));

    let mut compressed = Vec::new();
    {
        let mut w = CompressorWriter::new(&mut compressed, 4096, 5, 22);
        w.write_all(&rlp_concat).unwrap();
        w.flush().unwrap();
    }

    let mut payload = Vec::with_capacity(1 + compressed.len());
    payload.push(0x01);
    payload.extend_from_slice(&compressed);
    data.extend_from_slice(&payload);

    let parsed = parse_sequencer_message(0, None, &data).expect("parse ok");
    assert_eq!(parsed.segments.len(), 1);
    assert_eq!(parsed.segments[0], seg);
}
