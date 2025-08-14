use std::sync::{Arc, Mutex};

use nitro_batch_poster::poster::SegmentSource;
use nitro_streamer::streamer::TransactionStreamer;

pub struct NodeSegmentSource<D: nitro_inbox::db::Database> {
    streamer: Arc<TransactionStreamer<D>>,
    last_posted_msg_idx: Mutex<u64>,
    max_bytes: usize,
}

impl<D: nitro_inbox::db::Database> NodeSegmentSource<D> {
    pub fn new(streamer: Arc<TransactionStreamer<D>>, max_bytes: usize) -> Self {
        Self {
            streamer,
            last_posted_msg_idx: Mutex::new(0),
            max_bytes,
        }
    }
}

impl<D> SegmentSource for NodeSegmentSource<D>
where
    D: nitro_inbox::db::Database + Send + Sync + 'static,
{
    fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    fn next_segments(&self, max_bytes: usize) -> Vec<Vec<u8>> {
        let budget = if max_bytes == 0 { self.max_bytes } else { max_bytes };
        let mut used = 0usize;
        let mut segments: Vec<Vec<u8>> = Vec::new();

        let start_idx = {
            let guard = self.last_posted_msg_idx.lock().unwrap();
            *guard + 1
        };

        let Ok(head_idx) = self.streamer.get_head_message_index() else {
            return segments;
        };

        if head_idx < start_idx {
            return segments;
        }

        let mut last_taken = start_idx.saturating_sub(1);
        for idx in start_idx..=head_idx {
            let Ok(msg) = self.streamer.get_message_with_metadata_and_block_info(idx) else {
                break;
            };
            let l2msg = msg.message_with_meta.message.l2msg;
            let seg_len = l2msg.len();
            if used + seg_len > budget {
                break;
            }
            used += seg_len;
            segments.push(l2msg);
            last_taken = idx;
        }

        if last_taken >= start_idx {
            let mut guard = self.last_posted_msg_idx.lock().unwrap();
            *guard = last_taken;
        }

        segments
    }
}
