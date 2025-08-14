use anyhow::Result;
use crate as nitro_inbox;
use nitro_primitives::message::MessageWithMetadataAndBlockInfo;

pub trait Streamer: Send + Sync {
    fn reorg_at_and_end_batch(&self, first_msg_idx_reorged: u64) -> Result<()>;
    fn add_messages_and_end_batch(
        &self,
        first_msg_idx: u64,
        new_messages: &[MessageWithMetadataAndBlockInfo],
        track_block_metadata_from: Option<u64>,
    ) -> Result<()>;
}
