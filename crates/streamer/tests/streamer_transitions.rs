use std::sync::Arc;
use std::sync::Mutex;

use alloy_primitives::{Address, B256, U256};
use nitro_db_sled::SledDb;
use nitro_inbox::db::Database;
use nitro_primitives::l1::{L1IncomingMessage, L1IncomingMessageHeader};
use nitro_primitives::message::{MessageWithMetadata, MessageWithMetadataAndBlockInfo, MessageResult};
use nitro_primitives::dbkeys::{
    db_key, MESSAGE_PREFIX, MESSAGE_RESULT_PREFIX, BLOCK_HASH_INPUT_FEED_PREFIX,
    BLOCK_METADATA_INPUT_FEED_PREFIX, MESSAGE_COUNT_KEY,
};
use nitro_streamer::engine::ExecEngine;
use nitro_streamer::streamer::TransactionStreamer;

struct StubEngine {
    head: Mutex<u64>,
}

#[async_trait::async_trait]
impl ExecEngine for StubEngine {
    async fn head_message_index(&self) -> anyhow::Result<u64> {
        Ok(*self.head.lock().unwrap())
    }
    async fn digest_message(
        &self,
        msg_idx: u64,
        _msg: &MessageWithMetadata,
        _prefetch_next: Option<&MessageWithMetadata>,
    ) -> anyhow::Result<MessageResult> {
        *self.head.lock().unwrap() = msg_idx;
        Ok(MessageResult {
            block_hash: B256::from_slice(&[0x11u8; 32]),
            send_root: B256::from_slice(&[0x22u8; 32]),
        })
    }
    async fn result_at_message_index(&self, _msg_idx: u64) -> anyhow::Result<MessageResult> {
        Ok(MessageResult {
            block_hash: B256::from_slice(&[0x11u8; 32]),
            send_root: B256::from_slice(&[0x22u8; 32]),
        })
    }
    async fn mark_feed_start(&self, _to: u64) -> anyhow::Result<()> {
        Ok(())
    }
}

fn mk_msg(delayed_read: u64, payload: Vec<u8>) -> MessageWithMetadataAndBlockInfo {
    let inner = MessageWithMetadata {
        message: L1IncomingMessage {
            header: L1IncomingMessageHeader {
                kind: 3,
                poster: Address::ZERO,
                block_number: 0,
                timestamp: 0,
                request_id: Some(B256::ZERO),
                l1_base_fee: U256::ZERO,
            },
            l2msg: payload,
            batch_gas_cost: None,
        },
        delayed_messages_read: delayed_read,
    };
    MessageWithMetadataAndBlockInfo {
        message_with_meta: inner,
        block_hash: Some(B256::from_slice(&[0xAAu8; 32])),
        block_metadata: Some(vec![0x01, 0x02]),
    }
}

#[test]
fn write_and_reorg_messages_prunes_indices_and_sets_count() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(SledDb::open(dir.path().to_str().unwrap()).unwrap());
    let engine = Arc::new(StubEngine { head: Mutex::new(0) });
    let streamer = TransactionStreamer::new(db.clone(), engine);

    let m0 = mk_msg(0, vec![0x01]);
    let m1 = mk_msg(1, vec![0x02]);
    streamer.add_messages_and_end_batch(0, &[m0.clone(), m1.clone()], Some(0)).unwrap();

    let cnt = {
        let data = db.get(MESSAGE_COUNT_KEY).unwrap();
        let mut bytes = data.as_slice();
        <u64 as alloy_rlp::Decodable>::decode(&mut bytes).unwrap()
    };
    assert_eq!(cnt, 2);

    let have_m0 = db.get(&db_key(MESSAGE_PREFIX, 0)).unwrap();
    assert!(!have_m0.is_empty());
    let have_bh1 = db.get(&db_key(BLOCK_HASH_INPUT_FEED_PREFIX, 1)).unwrap();
    assert!(!have_bh1.is_empty());
    let have_meta1 = db.get(&db_key(BLOCK_METADATA_INPUT_FEED_PREFIX, 1)).unwrap();
    assert!(!have_meta1.is_empty());

    let m1b = mk_msg(1, vec![0x03]);
    streamer.add_messages_and_reorg(1, &[m1b.clone()], Some(0)).unwrap();

    let cnt2 = {
        let data = db.get(MESSAGE_COUNT_KEY).unwrap();
        let mut bytes = data.as_slice();
        <u64 as alloy_rlp::Decodable>::decode(&mut bytes).unwrap()
    };
    assert_eq!(cnt2, 2);

    let have_m1 = db.get(&db_key(MESSAGE_PREFIX, 1)).unwrap();
    assert!(!have_m1.is_empty());
}

#[tokio::test]
async fn execute_next_msg_advances_head_and_stores_result() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(SledDb::open(dir.path().to_str().unwrap()).unwrap());
    let engine = Arc::new(StubEngine { head: Mutex::new(0) });
    let streamer = TransactionStreamer::new(db.clone(), engine.clone());

    let m0 = mk_msg(0, vec![0x10]);
    let m1 = mk_msg(1, vec![0x20]);
    streamer.add_messages_and_end_batch(0, &[m0, m1], Some(0)).unwrap();

    let progressed = streamer.execute_next_msg().await.unwrap();
    assert!(!progressed, "returns false after executing last available message");

    let res_key = db_key(MESSAGE_RESULT_PREFIX, 1);
    let bytes = db.get(&res_key).unwrap();
    assert!(!bytes.is_empty());

    assert_eq!(*engine.head.lock().unwrap(), 1, "engine head advanced to 1");

    let progressed2 = streamer.execute_next_msg().await.unwrap();
    assert!(!progressed2, "no more to execute after catching up");
}
