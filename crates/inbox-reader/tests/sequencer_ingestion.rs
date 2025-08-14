use std::sync::Arc;

use alloy_primitives::{Address, B256, U256};
use async_trait::async_trait;
use nitro_db_sled::SledDb;
use nitro_inbox::tracker::InboxTracker;
use nitro_inbox::streamer::Streamer as TxStreamer;
use inbox_bridge::traits::{DelayedBridge, SequencerInbox, L1Header, L1HeaderReader};
use inbox_bridge::types::{DelayedInboxMessage, SequencerInboxBatch, TimeBounds};
use nitro_inbox_reader::reader::{InboxReader, InboxReaderConfig};
use nitro_primitives::l1::{L1IncomingMessage, L1IncomingMessageHeader};
use tokio::sync::mpsc::{self, Receiver};

struct StubStreamer;
impl TxStreamer for StubStreamer {
    fn reorg_at_and_end_batch(&self, _first_msg_idx_reorged: u64) -> anyhow::Result<()> { Ok(()) }
    fn add_messages_and_end_batch(
        &self,
        _first_msg_idx: u64,
        _new_messages: &[nitro_primitives::message::MessageWithMetadataAndBlockInfo],
        _track_block_metadata_from: Option<u64>,
    ) -> anyhow::Result<()> { Ok(()) }
    fn add_confirmed_messages_and_end_batch(
        &self,
        _first_msg_idx: u64,
        _new_messages: &[nitro_primitives::message::MessageWithMetadataAndBlockInfo],
        _track_block_metadata_from: Option<u64>,
    ) -> anyhow::Result<()> { Ok(()) }
}

struct MockDelayed {
    msgs: Vec<DelayedInboxMessage>,
}
impl MockDelayed {
    fn new() -> Self {
        let base_block = 10;
        let msg0 = L1IncomingMessage {
            header: L1IncomingMessageHeader {
                kind: 3,
                poster: Address::ZERO,
                block_number: base_block,
                timestamp: 1000,
                request_id: Some(B256::ZERO),
                l1_base_fee: U256::ZERO,
            },
            l2msg: vec![0x01],
            batch_gas_cost: None,
        };
        let d0 = DelayedInboxMessage {
            seq_num: 0,
            block_hash: B256::from_slice(&[0x11; 32]),
            before_inbox_acc: B256::ZERO,
            message: msg0,
            parent_chain_block_number: base_block,
        };
        let msg1 = L1IncomingMessage {
            header: L1IncomingMessageHeader {
                kind: 3,
                poster: Address::ZERO,
                block_number: base_block,
                timestamp: 1001,
                request_id: Some(B256::ZERO),
                l1_base_fee: U256::ZERO,
            },
            l2msg: vec![0x02],
            batch_gas_cost: None,
        };
        use nitro_primitives::accumulator::hash_after;
        use nitro_primitives::l1::serialize_incoming_l1_message_legacy;
        let d0_ser = serialize_incoming_l1_message_legacy(&d0.message).unwrap();
        let acc1 = hash_after(B256::ZERO, &d0_ser);
        let d1 = DelayedInboxMessage {
            seq_num: 1,
            block_hash: B256::from_slice(&[0x22; 32]),
            before_inbox_acc: acc1,
            message: msg1,
            parent_chain_block_number: base_block,
        };
        Self { msgs: vec![d0, d1] }
    }
}
#[async_trait]
impl DelayedBridge for MockDelayed {
    async fn get_message_count(&self, _block_number: u64) -> anyhow::Result<u64> {
        Ok(self.msgs.len() as u64)
    }
    async fn get_accumulator(&self, seq_num: u64, _block_number: u64, _block_hash: B256) -> anyhow::Result<B256> {
        if seq_num == 0 {
            Ok(self.msgs[0].after_inbox_acc())
        } else if seq_num == 1 {
            Ok(self.msgs[1].after_inbox_acc())
        } else {
            anyhow::bail!("out of range")
        }
    }
    async fn lookup_messages_in_range<F>(
        &self,
        _from_block: u64,
        _to_block: u64,
        _batch_fetcher: F,
    ) -> anyhow::Result<Vec<DelayedInboxMessage>>
    where
        F: Fn(u64) -> anyhow::Result<Vec<u8>> + Send + Sync,
    {
        Ok(self.msgs.clone())
    }
}

fn build_serialized_batch(after_delayed: u64, l2_payload: Vec<u8>) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&0u64.to_be_bytes()); // min ts
    out.extend_from_slice(&u64::MAX.to_be_bytes()); // max ts
    out.extend_from_slice(&0u64.to_be_bytes()); // min l1 block
    out.extend_from_slice(&u64::MAX.to_be_bytes()); // max l1 block
    out.extend_from_slice(&after_delayed.to_be_bytes()); // after_delayed_messages
    let mut seg: Vec<u8> = Vec::with_capacity(1 + l2_payload.len());
    seg.push(0x00);
    seg.extend_from_slice(&l2_payload);
    let seg_rlp = alloy_rlp::encode(&seg);
    out.extend_from_slice(&seg_rlp);
    out
}

struct MockSequencer {
    after_delayed_acc: B256,
    serialized: Vec<u8>,
}
#[async_trait]
impl SequencerInbox for MockSequencer {
    async fn get_batch_count(&self, _block_number: u64) -> anyhow::Result<u64> {
        Ok(1)
    }
    async fn get_accumulator(&self, _seq_num: u64, _block_number: u64) -> anyhow::Result<B256> {
        Ok(B256::from_slice(&[0xAB; 32]))
    }
    async fn lookup_batches_in_range(&self, _from_block: u64, _to_block: u64) -> anyhow::Result<Vec<SequencerInboxBatch>> {
        Ok(vec![SequencerInboxBatch {
            sequence_number: 0,
            before_inbox_acc: B256::ZERO,
            after_inbox_acc: B256::from_slice(&[0xAB; 32]),
            after_message_count: 1,
            after_delayed_count: 2,
            after_delayed_acc: self.after_delayed_acc,
            time_bounds: TimeBounds {
                min_timestamp: 0,
                max_timestamp: u64::MAX,
                min_block_number: 0,
                max_block_number: u64::MAX,
            },
            data_location: 0,
            bridge_address: Address::ZERO,
            parent_chain_block_number: 10,
            block_hash: B256::from_slice(&[0xEE; 32]),
            serialized: self.serialized.clone(),
        }])
    }
    async fn get_sequencer_message_bytes_in_block(
        &self,
        _block_number: u64,
        _seq_num: u64,
    ) -> anyhow::Result<(Vec<u8>, B256, Vec<u64>)> {
        Ok((self.serialized.clone(), B256::from_slice(&[0xEE; 32]), vec![]))
    }
}

struct MockHeaderReader {
    number: u64,
}
#[async_trait]
impl L1HeaderReader for MockHeaderReader {
    async fn last_header(&self) -> anyhow::Result<L1Header> {
        Ok(L1Header { number: self.number })
    }
    async fn latest_safe_block_nr(&self) -> anyhow::Result<u64> {
        Ok(self.number)
    }
    async fn latest_finalized_block_nr(&self) -> anyhow::Result<u64> {
        Ok(self.number)
    }
    async fn subscribe(&self) -> (Receiver<L1Header>, Box<dyn FnOnce() + Send>) {
        let (_tx, rx) = mpsc::channel(1);
        let unsub = Box::new(|| {});
        (rx, unsub)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reader_ingests_one_batch_and_updates_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(SledDb::open(dir.path().to_str().unwrap()).unwrap());
    let tracker = Arc::new(InboxTracker::new(db.clone(), Arc::new(StubStreamer)));
    tracker.initialize().unwrap();

    let delayed = Arc::new(MockDelayed::new());
    use nitro_primitives::accumulator::hash_after;
    use nitro_primitives::l1::serialize_incoming_l1_message_legacy;
    let d0_ser = serialize_incoming_l1_message_legacy(&delayed.msgs[0].message).unwrap();
    let acc1 = hash_after(alloy_primitives::B256::ZERO, &d0_ser);
    let d1_ser = serialize_incoming_l1_message_legacy(&delayed.msgs[1].message).unwrap();
    let after_delayed_acc = hash_after(acc1, &d1_ser);
    let serialized = build_serialized_batch(2, vec![0xCA, 0xFE]);
    let sequencer = Arc::new(MockSequencer { after_delayed_acc, serialized });
    let headers = Arc::new(MockHeaderReader { number: 20 });

    let cfg_fetcher = Arc::new(|| InboxReaderConfig {
        delay_blocks: 0,
        check_delay_ms: 10,
        min_blocks_to_read: 1,
        default_blocks_to_read: 1,
        target_messages_read: 10,
        max_blocks_to_read: 10,
        read_mode: "latest".to_string(),
    });

    let reader = InboxReader::new(
        tracker.clone(),
        delayed,
        sequencer,
        headers,
        10,
        cfg_fetcher,
    );

    let res = tokio::time::timeout(std::time::Duration::from_millis(1500), reader.start()).await;
    match res {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("reader.start error: {e:?}"),
        Err(_) => {}
    }

    assert_eq!(tracker.get_delayed_count().unwrap(), 2, "delayed count");
    assert_eq!(tracker.get_batch_count().unwrap(), 1, "batch count");
    let meta = tracker.get_batch_metadata(0).unwrap();
    assert_eq!(meta.message_count, 1, "batch message_count");
    assert_eq!(meta.delayed_message_count, 2, "batch delayed_message_count");
}
