use alloy_primitives::B256;
use alloy_rlp::{Decodable, Decoder, Encodable, Header, RlpDecodable, RlpEncodable};
use crate::l1::L1IncomingMessage;

#[derive(Clone, Debug, RlpEncodable, RlpDecodable)]
pub struct MessageWithMetadata {
    pub message: L1IncomingMessage,
    pub delayed_messages_read: u64,
}

#[derive(Clone, Debug)]
pub struct MessageWithMetadataAndBlockInfo {
    pub message_with_meta: MessageWithMetadata,
    pub block_hash: Option<B256>,
    pub block_metadata: Option<Vec<u8>>,
}

#[derive(Clone, Debug, RlpEncodable, RlpDecodable)]
pub struct BlockHashDbValue {
    pub block_hash: Option<B256>,
}
#[derive(Clone, Debug, RlpEncodable, RlpDecodable)]
pub struct MessageResult {
    pub block_hash: B256,
    pub send_root: B256,
}
