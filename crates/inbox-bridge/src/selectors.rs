pub const SIG_BATCH_COUNT: &str = "batchCount()";
pub const SIG_INBOX_ACCS: &str = "inboxAccs(uint256)";

pub const SIG_DELAYED_COUNT: &str = "delayedMessageCount()";
pub const SIG_DELAYED_INBOX_ACCS: &str = "delayedInboxAccs(uint256)";
pub const SIG_SEND_L2_FROM_ORIGIN: &str = "sendL2MessageFromOrigin(bytes)";

pub const EVT_SEQUENCER_BATCH_DELIVERED: &str =
    "SequencerBatchDelivered(uint256,bytes32,bytes32,bytes32,uint256,tuple(uint64,uint64,uint64,uint64),uint8)";
pub const EVT_SEQUENCER_BATCH_DATA: &str = "SequencerBatchData(uint256,bytes)";

pub const EVT_MESSAGE_DELIVERED: &str =
    "MessageDelivered(uint256,bytes32,address,uint8,address,bytes32,uint256,uint64)";
pub const EVT_INBOX_MESSAGE_DELIVERED: &str = "InboxMessageDelivered(uint256,bytes)";
pub const EVT_INBOX_MESSAGE_FROM_ORIGIN: &str = "InboxMessageDeliveredFromOrigin(uint256)";
