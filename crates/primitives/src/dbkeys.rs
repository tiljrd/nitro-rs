pub const MESSAGE_PREFIX: &[u8] = b"m";
pub const BLOCK_HASH_INPUT_FEED_PREFIX: &[u8] = b"b";
pub const BLOCK_METADATA_INPUT_FEED_PREFIX: &[u8] = b"t";
pub const MISSING_BLOCK_METADATA_INPUT_FEED_PREFIX: &[u8] = b"x";
pub const MESSAGE_RESULT_PREFIX: &[u8] = b"r";
pub const LEGACY_DELAYED_MESSAGE_PREFIX: &[u8] = b"d";
pub const RLP_DELAYED_MESSAGE_PREFIX: &[u8] = b"e";
pub const PARENT_CHAIN_BLOCK_NUMBER_PREFIX: &[u8] = b"p";
pub const SEQUENCER_BATCH_META_PREFIX: &[u8] = b"s";
pub const DELAYED_SEQUENCED_PREFIX: &[u8] = b"a";
pub const MEL_STATE_PREFIX: &[u8] = b"l";
pub const MEL_DELAYED_MESSAGE_PREFIX: &[u8] = b"y";

pub const MESSAGE_COUNT_KEY: &[u8] = b"_messageCount";
pub const LAST_PRUNED_MESSAGE_KEY: &[u8] = b"_lastPrunedMessageKey";
pub const LAST_PRUNED_DELAYED_MESSAGE_KEY: &[u8] = b"_lastPrunedDelayedMessageKey";
pub const DELAYED_MESSAGE_COUNT_KEY: &[u8] = b"_delayedMessageCount";
pub const SEQUENCER_BATCH_COUNT_KEY: &[u8] = b"_sequencerBatchCount";
pub const DB_SCHEMA_VERSION_KEY: &[u8] = b"_schemaVersion";
pub const HEAD_MEL_STATE_BLOCK_NUM_KEY: &[u8] = b"_headMelStateBlockNum";

pub const CURRENT_DB_SCHEMA_VERSION: u64 = 1;

pub fn uint64_to_key(x: u64) -> [u8; 8] {
    x.to_be_bytes()
}

pub fn db_key(prefix: &[u8], index: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(prefix.len() + 8);
    v.extend_from_slice(prefix);
    v.extend_from_slice(&uint64_to_key(index));
    v
}
