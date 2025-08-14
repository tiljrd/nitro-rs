use alloy_rlp::Decodable;
use alloy_primitives::{B256, Address};
use nitro_primitives::message::{MessageWithMetadata, MessageWithMetadataAndBlockInfo};
use nitro_primitives::l1::{L1IncomingMessage, L1IncomingMessageHeader};
use std::io::Read;

const BATCH_SEGMENT_KIND_L2_MESSAGE: u8 = 0;
const BATCH_SEGMENT_KIND_L2_MESSAGE_BROTLI: u8 = 1;
const BATCH_SEGMENT_KIND_DELAYED_MESSAGES: u8 = 2;
const BATCH_SEGMENT_KIND_ADVANCE_TIMESTAMP: u8 = 3;
const BATCH_SEGMENT_KIND_ADVANCE_L1_BLOCK_NUMBER: u8 = 4;

pub struct SequencerMessage {
    pub min_timestamp: u64,
    pub max_timestamp: u64,
    pub min_l1_block: u64,
    pub max_l1_block: u64,
    pub after_delayed_messages: u64,
    pub segments: Vec<Vec<u8>>,
}

pub trait InboxBackend {
    fn peek_sequencer_inbox(&mut self) -> anyhow::Result<(Vec<u8>, Option<B256>)>;
    fn get_sequencer_inbox_position(&self) -> u64;
    fn advance_sequencer_inbox(&mut self);

    fn get_position_within_message(&self) -> u64;
    fn set_position_within_message(&mut self, pos: u64);

    fn read_delayed_inbox(&self, seqnum: u64) -> anyhow::Result<L1IncomingMessage>;
}

pub fn parse_sequencer_message(batch_num: u64, _batch_block_hash: Option<B256>, data: &[u8]) -> anyhow::Result<SequencerMessage> {
    if data.len() < 40 {
        anyhow::bail!("sequencer message missing L1 header");
    }
    let mut off = 0usize;
    let mut u64be = |d: &[u8], o: &mut usize| -> u64 {
        let v = u64::from_be_bytes(d[*o..*o+8].try_into().unwrap());
        *o += 8;
        v
    };
    let min_timestamp = u64be(data, &mut off);
    let max_timestamp = u64be(data, &mut off);
    let min_l1_block = u64be(data, &mut off);
    let max_l1_block = u64be(data, &mut off);
    let after_delayed_messages = u64be(data, &mut off);
    let mut payload = &data[off..];

    let mut segments: Vec<Vec<u8>> = Vec::new();
    if !payload.is_empty() {
        let first = payload[0];
        if first == 0x01 { // heuristic: brotli flag per Nitro, value checked via daprovider
            let mut decompressed = Vec::new();
            let mut reader = brotli::Decompressor::new(&payload[1..], 4096);
            reader.read_to_end(&mut decompressed)?;
            let mut cur = decompressed.as_slice();
            while !cur.is_empty() {
                match <Vec<u8> as Decodable>::decode(&mut cur) {
                    Ok(seg) => {
                        segments.push(seg);
                        if segments.len() > 100 * 1024 { break; }
                    }
                    Err(_) => break,
                }
            }
        } else {
        }
    }

    Ok(SequencerMessage {
        min_timestamp,
        max_timestamp,
        min_l1_block,
        max_l1_block,
        after_delayed_messages,
        segments,
    })
}

pub struct InboxMultiplexer<B: InboxBackend> {
    backend: B,
    delayed_messages_read: u64,
    cached_msg: Option<SequencerMessage>,
    cached_batch_hash: Option<B256>,
    cached_segment_num: u64,
    cached_segment_timestamp: u64,
    cached_segment_block_number: u64,
    cached_submessage_number: u64,
}

impl<B: InboxBackend> InboxMultiplexer<B> {
    pub fn new(backend: B, delayed_messages_read: u64) -> Self {
        Self {
            backend,
            delayed_messages_read,
            cached_msg: None,
            cached_batch_hash: None,
            cached_segment_num: 0,
            cached_segment_timestamp: 0,
            cached_segment_block_number: 0,
            cached_submessage_number: 0,
        }
    }

    fn advance_seq_msg(&mut self) {
        if let Some(seq) = &self.cached_msg {
            self.delayed_messages_read = seq.after_delayed_messages;
        }
        self.backend.set_position_within_message(0);
        self.backend.advance_sequencer_inbox();
        self.cached_msg = None;
        self.cached_batch_hash = None;
        self.cached_segment_num = 0;
        self.cached_segment_timestamp = 0;
        self.cached_segment_block_number = 0;
        self.cached_submessage_number = 0;
    }

    fn advance_submsg(&mut self) {
        let prev = self.backend.get_position_within_message();
        self.backend.set_position_within_message(prev + 1);
    }

    fn is_cached_segment_last(&self) -> bool {
        if let Some(seq) = &self.cached_msg {
            if self.delayed_messages_read < seq.after_delayed_messages {
                return false;
            }
            for seg in seq.segments.iter().skip(self.cached_segment_num as usize + 1) {
                if seg.is_empty() { continue; }
                let kind = seg[0];
                if kind == BATCH_SEGMENT_KIND_L2_MESSAGE || kind == BATCH_SEGMENT_KIND_L2_MESSAGE_BROTLI {
                    return false;
                }
                if kind == BATCH_SEGMENT_KIND_DELAYED_MESSAGES {
                    return false;
                }
            }
        }
        true
    }

    pub fn pop(&mut self) -> anyhow::Result<Option<MessageWithMetadataAndBlockInfo>> {
        if self.cached_msg.is_none() {
            let (bytes, batch_hash) = self.backend.peek_sequencer_inbox()?;
            let seqnum = self.backend.get_sequencer_inbox_position();
            let parsed = parse_sequencer_message(seqnum, batch_hash, &bytes)?;
            self.cached_batch_hash = batch_hash;
            self.cached_msg = Some(parsed);
        }
        let (msg, err) = self.get_next_msg();
        if self.is_cached_segment_last() {
            self.advance_seq_msg();
        } else {
            self.advance_submsg();
        }
        if let Some(err) = err {
            return Err(err);
        }
        Ok(msg)
    }

    fn get_next_msg(&mut self) -> (Option<MessageWithMetadataAndBlockInfo>, Option<anyhow::Error>) {
        let target_submessage = self.backend.get_position_within_message();
        let mut segment_num = self.cached_segment_num;
        let mut timestamp = self.cached_segment_timestamp;
        let mut block_number = self.cached_segment_block_number;
        let mut submessage_number = self.cached_submessage_number;

        let seq = match &self.cached_msg {
            Some(s) => s,
            None => return (None, None),
        };

        while (segment_num as usize) < seq.segments.len() {
            let seg = &seq.segments[segment_num as usize];
            if seg.is_empty() { segment_num += 1; continue; }
            let kind = seg[0];
            if kind == BATCH_SEGMENT_KIND_ADVANCE_TIMESTAMP || kind == BATCH_SEGMENT_KIND_ADVANCE_L1_BLOCK_NUMBER {
                let mut cur = &seg[1..];
                if let Ok(v) = <u64 as Decodable>::decode(&mut cur) {
                    if kind == BATCH_SEGMENT_KIND_ADVANCE_TIMESTAMP { timestamp = timestamp.saturating_add(v); }
                    else { block_number = block_number.saturating_add(v); }
                }
                segment_num += 1;
            } else if submessage_number < target_submessage {
                segment_num += 1;
                submessage_number += 1;
            } else {
                break;
            }
        }

        self.cached_segment_num = segment_num;
        self.cached_segment_timestamp = timestamp;
        self.cached_segment_block_number = block_number;
        self.cached_submessage_number = submessage_number;

        let mut seg = if (segment_num as usize) < seq.segments.len() {
            seq.segments[segment_num as usize].clone()
        } else {
            vec![BATCH_SEGMENT_KIND_DELAYED_MESSAGES]
        };

        if seg.is_empty() {
            return (None, None);
        }
        let kind = seg[0];
        let rest = &seg[1..];

        if kind == BATCH_SEGMENT_KIND_L2_MESSAGE || kind == BATCH_SEGMENT_KIND_L2_MESSAGE_BROTLI {
            let mut payload = rest.to_vec();
            if kind == BATCH_SEGMENT_KIND_L2_MESSAGE_BROTLI {
                let mut decompressed = Vec::new();
                let mut reader = brotli::Decompressor::new(&payload[..], 4096);
                if let Err(e) = reader.read_to_end(&mut decompressed) {
                    return (None, Some(anyhow::anyhow!(e)));
                }
                payload = decompressed;
            }
            let l1_header = L1IncomingMessageHeader {
                kind: 3,
                poster: Address::ZERO, // BatchPosterAddress; filled by execution hooks
                block_number: block_number.clamp(seq.min_l1_block, seq.max_l1_block),
                timestamp: timestamp.clamp(seq.min_timestamp, seq.max_timestamp),
                request_id: None,
                l1_base_fee: alloy_primitives::U256::ZERO,
            };
            let msg = L1IncomingMessage { header: l1_header, l2msg: payload, batch_gas_cost: None };
            let with_meta = MessageWithMetadata { message: msg, delayed_messages_read: self.delayed_messages_read };
            let out = MessageWithMetadataAndBlockInfo {
                message_with_meta: with_meta,
                block_hash: self.cached_batch_hash,
                block_metadata: None,
            };
            return (Some(out), None);
        } else if kind == BATCH_SEGMENT_KIND_DELAYED_MESSAGES {
            if self.delayed_messages_read >= seq.after_delayed_messages {
                let invalid = L1IncomingMessage {
                    header: L1IncomingMessageHeader {
                        kind: 0xff, // Invalid
                        poster: Address::ZERO,
                        block_number: seq.max_l1_block,
                        timestamp: seq.max_timestamp,
                        request_id: None,
                        l1_base_fee: alloy_primitives::U256::ZERO,
                    },
                    l2msg: vec![],
                    batch_gas_cost: None,
                };
                let with_meta = MessageWithMetadata { message: invalid, delayed_messages_read: seq.after_delayed_messages };
                let out = MessageWithMetadataAndBlockInfo {
                    message_with_meta: with_meta,
                    block_hash: self.cached_batch_hash,
                    block_metadata: None,
                };
                return (Some(out), None);
            } else {
                match self.backend.read_delayed_inbox(self.delayed_messages_read) {
                    Ok(delayed) => {
                        self.delayed_messages_read += 1;
                        let with_meta = MessageWithMetadata { message: delayed, delayed_messages_read: self.delayed_messages_read };
                        let out = MessageWithMetadataAndBlockInfo {
                            message_with_meta: with_meta,
                            block_hash: None,
                            block_metadata: None,
                        };
                        return (Some(out), None);
                    }
                    Err(e) => return (None, Some(e)),
                }
            }
        }
        (None, None)
    }
}
