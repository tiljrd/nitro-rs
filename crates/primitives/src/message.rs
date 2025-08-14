use alloy_primitives::B256;
use alloy_rlp::{Decodable, Encodable, Header};
use crate::l1::L1IncomingMessage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageWithMetadata {
    pub message: L1IncomingMessage,
    pub delayed_messages_read: u64,
}

impl Encodable for MessageWithMetadata {
    fn length(&self) -> usize {
        let mut payload_len = 0;
        payload_len += self.message.length();
        payload_len += self.delayed_messages_read.length();
        let header = Header { list: true, payload_length: payload_len };
        header.length() + payload_len
    }

    fn encode(&self, out: &mut dyn bytes::BufMut) {
        let payload_len = self.message.length() + self.delayed_messages_read.length();
        let header = Header { list: true, payload_length: payload_len };
        header.encode(out);
        self.message.encode(out);
        self.delayed_messages_read.encode(out);
    }
}

impl Decodable for MessageWithMetadata {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let header = Header::decode(buf)?;
        if !header.list {
            return Err(alloy_rlp::Error::Custom("expected list for MessageWithMetadata"));
        }
        let mut bytes = &buf[..header.payload_length];
        let message = <L1IncomingMessage as Decodable>::decode(&mut bytes)?;
        let delayed_messages_read = <u64 as Decodable>::decode(&mut bytes)?;
        *buf = &buf[header.payload_length..];
        Ok(Self { message, delayed_messages_read })
    }
}

#[derive(Clone, Debug)]
pub struct MessageWithMetadataAndBlockInfo {
    pub message_with_meta: MessageWithMetadata,
    pub block_hash: Option<B256>,
    pub block_metadata: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct BlockHashDbValue {
    pub block_hash: Option<B256>,
}

impl Encodable for BlockHashDbValue {
    fn length(&self) -> usize {
        match &self.block_hash {
            Some(h) => h.length(),
            None => 1,
        }
    }

    fn encode(&self, out: &mut dyn bytes::BufMut) {
        match &self.block_hash {
            Some(h) => h.encode(out),
            None => out.put_u8(0x80),
        }
    }
}

impl Decodable for BlockHashDbValue {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        if buf.first().copied() == Some(0x80) {
            *buf = &buf[1..];
            return Ok(Self { block_hash: None });
        }
        let h = <B256 as Decodable>::decode(buf)?;
        Ok(Self { block_hash: Some(h) })
    }
}

#[derive(Clone, Debug)]
pub struct MessageResult {
    pub block_hash: B256,
    pub send_root: B256,
}

impl Encodable for MessageResult {
    fn length(&self) -> usize {
        let payload_len = self.block_hash.length() + self.send_root.length();
        let header = Header { list: true, payload_length: payload_len };
        header.length() + payload_len
    }

    fn encode(&self, out: &mut dyn bytes::BufMut) {
        let payload_len = self.block_hash.length() + self.send_root.length();
        let header = Header { list: true, payload_length: payload_len };
        header.encode(out);
        self.block_hash.encode(out);
        self.send_root.encode(out);
    }
}

impl Decodable for MessageResult {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let header = Header::decode(buf)?;
        if !header.list {
            return Err(alloy_rlp::Error::Custom("expected list for MessageResult"));
        }
        let mut bytes = &buf[..header.payload_length];
        let block_hash = <B256 as Decodable>::decode(&mut bytes)?;
        let send_root = <B256 as Decodable>::decode(&mut bytes)?;
        *buf = &buf[header.payload_length..];
        Ok(Self { block_hash, send_root })
    }
}
