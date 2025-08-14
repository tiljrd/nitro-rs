use std::sync::{Arc, Mutex};

use nitro_batch_poster::poster::SegmentSource;
use nitro_inbox::streamer::Streamer as _;
use nitro_primitives::message::MessageWithMetadataAndBlockInfo;

pub struct NodeSegmentSource<S> {
    streamer: Arc<S>,
    last_posted_msg_idx: Mutex<u64>,
    max_bytes: usize,
}

impl<S> NodeSegmentSource<S> {
    pub fn new(streamer: Arc<S>, max_bytes: usize) -> Self {
        Self {
            streamer,
            last_posted_msg_idx: Mutex::new(0),
            max_bytes,
        }
    }
}

impl<S> SegmentSource for NodeSegmentSource<S>
where
    S: nitro_inbox::streamer::Streamer + Send + Sync + 'static,
{
    fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    fn next_segments(&self, max_bytes: usize) -> Vec<Vec<u8>> {
        let budget = if max_bytes == 0 { self.max_bytes } else { max_bytes };
        let mut used = 0usize;
        let mut segments: Vec<Vec<u8>> = Vec::new();

        let mut idx = {
            let mut guard = self.last_posted_msg_idx.lock().unwrap();
            let v = *guard;
            v
        };


        let _ = used;
        let _ = idx;
        segments
    }
}
