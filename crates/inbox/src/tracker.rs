use alloy_primitives::B256;

#[derive(Debug, Clone, Default)]
pub struct InboxTracker {
    last_batch_count: u64,
    last_delayed_count: u64,
}

impl InboxTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_batch_count(&self) -> u64 { self.last_batch_count }
    pub fn get_delayed_count(&self) -> u64 { self.last_delayed_count }

    pub fn set_batch_accumulator(&mut self, _seqnum: u64, _acc: B256) {}
    pub fn set_delayed_accumulator(&mut self, _seqnum: u64, _acc: B256) {}

    pub fn reorg_delayed_to(&mut self, count: u64) { self.last_delayed_count = count; }
}
