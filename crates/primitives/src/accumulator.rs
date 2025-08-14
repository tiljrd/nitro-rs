use alloy_primitives::{keccak256, B256};

pub fn hash_after(prev: B256, msg: &[u8]) -> B256 {
    let mut data = Vec::with_capacity(32 + msg.len());
    data.extend_from_slice(prev.as_slice());
    data.extend_from_slice(msg);
    B256::from(keccak256(&data))
}
