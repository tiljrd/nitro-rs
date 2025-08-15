use alloy_primitives::{Address, B256, U256};
use alloy_rlp::{Decodable, Encodable, Header};
use anyhow::anyhow;
use std::io::{Cursor, Read};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct L1IncomingMessageHeader {
    pub kind: u8,
    pub poster: Address,
    pub block_number: u64,
    pub timestamp: u64,
    pub request_id: Option<B256>,
    pub l1_base_fee: U256,
}

impl L1IncomingMessageHeader {
    pub fn seq_num(&self) -> anyhow::Result<u64> {
        let Some(req) = self.request_id else { return Err(anyhow!("no requestId")) };
        let n = U256::from_be_bytes(req.0);
        if n > U256::from(u64::MAX) {
            return Err(anyhow!("bad requestId"));
        }
        Ok(n.to::<u64>())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct L1IncomingMessage {
    pub header: L1IncomingMessageHeader,
    pub l2msg: Vec<u8>,
    pub batch_gas_cost: Option<u64>,
}

impl Encodable for L1IncomingMessageHeader {
    fn length(&self) -> usize {
        let mut payload_len = 0;
        payload_len += self.kind.length();
        payload_len += self.poster.length();
        payload_len += self.block_number.length();
        payload_len += self.timestamp.length();
        if let Some(req) = self.request_id {
            payload_len += req.length();
        } else {
            payload_len += 1;
        }
        payload_len += self.l1_base_fee.length();
        let header = Header { list: true, payload_length: payload_len };
        header.length() + payload_len
    }

    fn encode(&self, out: &mut dyn bytes::BufMut) {
        let content_len = self.kind.length()
            + self.poster.length()
            + self.block_number.length()
            + self.timestamp.length()
            + if let Some(req) = self.request_id { req.length() } else { 1 }
            + self.l1_base_fee.length();
        let header = Header { list: true, payload_length: content_len };
        header.encode(out);

        self.kind.encode(out);
        self.poster.encode(out);
        self.block_number.encode(out);
        self.timestamp.encode(out);
        if let Some(req) = self.request_id {
            req.encode(out);
        } else {
            out.put_u8(0xc0);
        }
        self.l1_base_fee.encode(out);
    }
}

impl Decodable for L1IncomingMessageHeader {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let payload = alloy_rlp::Header::decode(buf)?;
        if !payload.list {
            return Err(alloy_rlp::Error::Custom("expected list for L1IncomingMessageHeader"));
        }
        let mut bytes = &buf[..payload.payload_length];
        let kind = <u8 as Decodable>::decode(&mut bytes)?;
        let poster = <Address as Decodable>::decode(&mut bytes)?;
        let block_number = <u64 as Decodable>::decode(&mut bytes)?;
        let timestamp = <u64 as Decodable>::decode(&mut bytes)?;
        let req_peek = *bytes.get(0).ok_or(alloy_rlp::Error::InputTooShort)?;
        let request_id = if req_peek == 0xc0 {
            bytes = &bytes[1..];
            None
        } else {
            Some(<B256 as Decodable>::decode(&mut bytes)?)
        };
        let l1_base_fee = <U256 as Decodable>::decode(&mut bytes)?;
        *buf = &buf[payload.payload_length..];
        Ok(Self { kind, poster, block_number, timestamp, request_id, l1_base_fee })
    }
}

impl Encodable for L1IncomingMessage {
    fn length(&self) -> usize {
        let mut payload_len = 0;
        payload_len += self.header.length();
        payload_len += self.l2msg.length();
        if let Some(g) = self.batch_gas_cost {
            payload_len += g.length();
        }
        let header = Header { list: true, payload_length: payload_len };
        header.length() + payload_len
    }

    fn encode(&self, out: &mut dyn bytes::BufMut) {
        let content_len = self.header.length() + self.l2msg.length() + self.batch_gas_cost.map(|g| g.length()).unwrap_or(0);
        let header = Header { list: true, payload_length: content_len };
        header.encode(out);
        self.header.encode(out);
        self.l2msg.encode(out);
        if let Some(g) = self.batch_gas_cost {
            g.encode(out);
        }
    }
}

impl Decodable for L1IncomingMessage {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let payload = alloy_rlp::Header::decode(buf)?;
        if !payload.list {
            return Err(alloy_rlp::Error::Custom("expected list for L1IncomingMessage"));
        }
        let mut bytes = &buf[..payload.payload_length];
        let header = <L1IncomingMessageHeader as Decodable>::decode(&mut bytes)?;
        let l2msg = <Vec<u8> as Decodable>::decode(&mut bytes)?;
        let batch_gas_cost = if !bytes.is_empty() {
            Some(<u64 as Decodable>::decode(&mut bytes)?)
        } else {
            None
        };
        *buf = &buf[payload.payload_length..];
        Ok(Self { header, l2msg, batch_gas_cost })
    }
}
use alloy_primitives::keccak256;

fn u64_min_be_bytes(n: u64) -> Vec<u8> {
    if n == 0 {
        return Vec::new();
    }
    let be = n.to_be_bytes();
    let mut i = 0usize;
    while i < be.len() && be[i] == 0 {
        i += 1;
    }
    be[i..].to_vec()
}

pub fn delayed_message_body_hash(msg: &L1IncomingMessage) -> B256 {
    let mut buf: Vec<u8> = Vec::new();
    buf.push(msg.header.kind);
    buf.extend_from_slice(msg.header.poster.as_slice());
    buf.extend_from_slice(&u64_min_be_bytes(msg.header.block_number));
    buf.extend_from_slice(&u64_min_be_bytes(msg.header.timestamp));
    if let Some(req) = msg.header.request_id {
        buf.extend_from_slice(req.as_slice());
    }
    let mut base_fee_be = msg.header.l1_base_fee.to_be_bytes_vec();
    while !base_fee_be.is_empty() && base_fee_be[0] == 0 {
        base_fee_be.remove(0);
    }
    buf.extend_from_slice(&base_fee_be);
    let l2_hash = keccak256(&msg.l2msg);
    buf.extend_from_slice(l2_hash.as_slice());
    B256::from(keccak256(&buf))
}



fn read_exact<const N: usize>(r: &mut Cursor<&[u8]>) -> anyhow::Result<[u8; N]> {
    let mut buf = [0u8; N];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_u64_be(r: &mut Cursor<&[u8]>) -> anyhow::Result<u64> {
    let b = read_exact::<8>(r)?;
    Ok(u64::from_be_bytes(b))
}

pub fn parse_incoming_l1_message_legacy(bytes: &[u8]) -> anyhow::Result<L1IncomingMessage> {
    let mut rdr = Cursor::new(bytes);
    let mut kind_buf = [0u8; 1];
    rdr.read_exact(&mut kind_buf)?;
    let kind = kind_buf[0];

    let addr32 = read_exact::<32>(&mut rdr)?;
    let poster = Address::from_slice(&addr32[12..]);

    let block_number = read_u64_be(&mut rdr)?;
    let timestamp = read_u64_be(&mut rdr)?;
    let req = read_exact::<32>(&mut rdr)?;
    let request_id = Some(B256::from_slice(&req));

    let basefee_hash = read_exact::<32>(&mut rdr)?;
    let l1_base_fee = U256::from_be_bytes(basefee_hash);

    let mut l2msg = Vec::new();
    rdr.read_to_end(&mut l2msg)?;

    Ok(L1IncomingMessage {
        header: L1IncomingMessageHeader {
            kind,
            poster,
            block_number,
            timestamp,
            request_id,
            l1_base_fee,
        },
        l2msg,
        batch_gas_cost: None,
    })
}

pub fn serialize_incoming_l1_message_legacy(msg: &L1IncomingMessage) -> anyhow::Result<Vec<u8>> {
    let Some(req) = msg.header.request_id else { return Err(anyhow!("cannot serialize without requestId")); };
    let mut out = Vec::with_capacity(1 + 32 + 8 + 8 + 32 + 32 + msg.l2msg.len());
    out.push(msg.header.kind);
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(msg.header.poster.as_slice());
    out.extend_from_slice(&msg.header.block_number.to_be_bytes());
    out.extend_from_slice(&msg.header.timestamp.to_be_bytes());
    out.extend_from_slice(req.as_slice());
    out.extend_from_slice(&msg.header.l1_base_fee.to_be_bytes::<32>());
    out.extend_from_slice(&msg.l2msg);
    Ok(out)
}
