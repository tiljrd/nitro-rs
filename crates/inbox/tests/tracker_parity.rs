use std::sync::Arc;
use tempfile::tempdir;

use alloy_primitives::{Address, B256};
use nitro_db_sled::SledDb;
use nitro_inbox::streamer::Streamer;
use nitro_inbox::tracker::InboxTracker;
use nitro_primitives::l1::{
    serialize_incoming_l1_message_legacy, L1IncomingMessage, L1IncomingMessageHeader,
};
use inbox_bridge::types::{SequencerInboxBatch, TimeBounds};

struct StubStreamer;
impl Streamer for StubStreamer {
    fn reorg_at_and_end_batch(&self, _first_msg_idx_reorged: u64) -> anyhow::Result<()> {
        Ok(())
    }
    fn add_messages_and_end_batch(
        &self,
        _first_msg_idx: u64,
        _new_messages: &[nitro_primitives::message::MessageWithMetadataAndBlockInfo],
        _track_block_metadata_from: Option<u64>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    fn add_confirmed_messages_and_end_batch(
        &self,
        _first_msg_idx: u64,
        _new_messages: &[nitro_primitives::message::MessageWithMetadataAndBlockInfo],
        _track_block_metadata_from: Option<u64>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

fn l1_msg(ts: u64, l1_block: u64, payload: &[u8]) -> L1IncomingMessage {
    L1IncomingMessage {
        header: L1IncomingMessageHeader {
            kind: 3,
            poster: Address::ZERO,
            block_number: l1_block,
            timestamp: ts,
            request_id: Some(alloy_primitives::B256::ZERO),
            l1_base_fee: alloy_primitives::U256::ZERO,
        },
        l2msg: payload.to_vec(),
        batch_gas_cost: None,
    }
}

#[test]
fn tracker_adds_delayed_and_batches_and_tracks_counts() {
    let dir = tempdir().unwrap();
    let db = Arc::new(SledDb::open(dir.path().to_str().unwrap()).unwrap());
    let streamer = Arc::new(StubStreamer);
    let tracker = InboxTracker::new(db.clone(), streamer);
    tracker.initialize().unwrap();

    let d0 = l1_msg(10, 100, b"hello");
    let d1 = l1_msg(11, 101, b"world");
    let d0_ser = serialize_incoming_l1_message_legacy(&d0).unwrap();
    let d1_ser = serialize_incoming_l1_message_legacy(&d1).unwrap();

    tracker
        .add_delayed_messages(&[(0, B256::ZERO, d0_ser.clone())], None)
        .unwrap();
    let prev_acc = tracker.get_delayed_acc(0).unwrap();
    tracker
        .add_delayed_messages(&[(1, prev_acc, d1_ser.clone())], None)
        .unwrap();

    assert_eq!(tracker.get_delayed_count().unwrap(), 2);
    let _ = tracker.get_delayed_acc(0).unwrap();
    let _ = tracker.get_delayed_acc(1).unwrap();

    let batch = SequencerInboxBatch {
        sequence_number: 0,
        before_inbox_acc: B256::ZERO,
        after_inbox_acc: B256::from_slice(&[1u8; 32]),
        after_message_count: 0,
        after_delayed_count: 2,
        after_delayed_acc: tracker.get_delayed_acc(1).unwrap(),
        time_bounds: TimeBounds {
            min_timestamp: 0,
            max_timestamp: u64::MAX,
            min_block_number: 0,
            max_block_number: u64::MAX,
        },
        data_location: 0,
        bridge_address: Address::ZERO,
        parent_chain_block_number: 200,
        block_hash: B256::ZERO,
        serialized: vec![],
    };

    tracker.add_sequencer_batches(&[batch]).unwrap();

    assert_eq!(tracker.get_batch_count().unwrap(), 1);
    let meta = tracker.get_batch_metadata(0).unwrap();
    assert_eq!(meta.message_count, 1);
    assert_eq!(meta.delayed_message_count, 2);
    assert_eq!(meta.parent_chain_block, 200);
}

#[test]
fn tracker_reorg_delayed_rolls_back_and_deletes_future_mappings() {
    let dir = tempdir().unwrap();
    let db = Arc::new(SledDb::open(dir.path().to_str().unwrap()).unwrap());
    let streamer = Arc::new(StubStreamer);
    let tracker = InboxTracker::new(db.clone(), streamer);
    tracker.initialize().unwrap();

    for i in 0..3 {
        let dm = l1_msg(20 + i, 300 + i, &[i as u8]);
        let ser = serialize_incoming_l1_message_legacy(&dm).unwrap();
        tracker
            .add_delayed_messages(&[(i, B256::ZERO, ser)], None)
            .unwrap();
    }
    assert_eq!(tracker.get_delayed_count().unwrap(), 3);

    let batch = SequencerInboxBatch {
        sequence_number: 0,
        before_inbox_acc: B256::ZERO,
        after_inbox_acc: B256::from_slice(&[2u8; 32]),
        after_message_count: 0,
        after_delayed_count: 3,
        after_delayed_acc: tracker.get_delayed_acc(2).unwrap(),
        time_bounds: TimeBounds {
            min_timestamp: 0,
            max_timestamp: u64::MAX,
            min_block_number: 0,
            max_block_number: u64::MAX,
        },
        data_location: 0,
        bridge_address: Address::ZERO,
        parent_chain_block_number: 400,
        block_hash: B256::ZERO,
        serialized: vec![],
    };
    tracker.add_sequencer_batches(&[batch]).unwrap();
    assert_eq!(tracker.get_batch_count().unwrap(), 1);

    tracker.reorg_delayed_to(1).unwrap();
    assert_eq!(tracker.get_delayed_count().unwrap(), 1);

    assert!(tracker.get_delayed_acc(1).is_err());

    assert_eq!(tracker.get_batch_count().unwrap(), 0);
}
