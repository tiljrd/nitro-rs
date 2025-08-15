use alloy_primitives::{Address, B256, U256};
use nitro_primitives::l1::{L1IncomingMessage, serialize_incoming_l1_message_legacy};
use nitro_primitives::accumulator::hash_after;

#[derive(Clone, Debug)]
pub struct DelayedInboxMessage {
    pub seq_num: u64,
    pub block_hash: B256,
    pub before_inbox_acc: B256,
    pub message: L1IncomingMessage,
    pub parent_chain_block_number: u64,
}

impl DelayedInboxMessage {
    pub fn after_inbox_acc(&self) -> B256 {
        let ser = serialize_incoming_l1_message_legacy(&self.message)
            .expect("serialize_incoming_l1_message_legacy");
        hash_after(self.before_inbox_acc, &ser)
    }
}

#[derive(Clone, Debug)]
pub struct TimeBounds {
    pub min_timestamp: u64,
    pub max_timestamp: u64,
    pub min_block_number: u64,
    pub max_block_number: u64,
}

#[derive(Clone, Debug)]
pub struct SequencerInboxBatch {
    pub sequence_number: u64,
    pub before_inbox_acc: B256,
    pub after_inbox_acc: B256,
    pub after_message_count: u64,
    pub after_delayed_count: u64,
    pub after_delayed_acc: B256,
    pub time_bounds: TimeBounds,
    pub data_location: u8,
    pub bridge_address: Address,
    pub parent_chain_block_number: u64,
    pub block_hash: B256,
    pub tx_hash: B256,
    pub serialized: Vec<u8>,
}
