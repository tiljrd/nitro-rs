use alloy_primitives::{Address, B256, U256};
use nitro_primitives::l1::L1IncomingMessage;

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
        use alloy_primitives::keccak256;
        let header = &self.message.header;
        let h = keccak256(
            [
                &[header.kind][..],
                header.poster.as_slice(),
                &header.block_number.to_be_bytes(),
                &header.timestamp.to_be_bytes(),
                header.request_id.map(|b| b.0.to_vec()).unwrap_or_default().as_slice(),
                &header.l1_base_fee.to_be_bytes::<32>(),
                &keccak256(&self.message.l2msg).0[..],
            ]
            .concat(),
        );
        keccak256([self.before_inbox_acc.0.to_vec(), h.0.to_vec()].concat())
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
    pub serialized: Vec<u8>,
}
