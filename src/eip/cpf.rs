use bytes::{Buf, BufMut};

use crate::codec::BufExt;
use crate::{Error, Result};

wire_code! {
    CommonPacketItemId(u16) {
        NULL_ADDRESS = 0x0000,
        LIST_IDENTITY = 0x000C,
        CONNECTION_ADDRESS = 0x00A1,
        CONNECTED_TRANSPORT_PACKET = 0x00B1,
        UNCONNECTED_MESSAGE = 0x00B2,
        O2T_SOCKADDR_INFO = 0x8000,
        T2O_SOCKADDR_INFO = 0x8001,
        SEQUENCED_ADDRESS = 0x8002,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommonPacketItem {
    pub type_id: CommonPacketItemId,
    pub data: Vec<u8>,
}

impl CommonPacketItem {
    pub fn new(type_id: CommonPacketItemId, data: Vec<u8>) -> Self {
        Self { type_id, data }
    }

    pub fn null_address() -> Self {
        Self::default()
    }

    pub fn unconnected_data(data: Vec<u8>) -> Self {
        Self::new(CommonPacketItemId::UNCONNECTED_MESSAGE, data)
    }

    pub fn connected_data(data: Vec<u8>) -> Self {
        Self::new(CommonPacketItemId::CONNECTED_TRANSPORT_PACKET, data)
    }

    pub fn sequenced_address(connection_id: u32, sequence_number: u32) -> Self {
        let mut data = Vec::with_capacity(8);
        data.put_u32_le(connection_id);
        data.put_u32_le(sequence_number);
        Self::new(CommonPacketItemId::SEQUENCED_ADDRESS, data)
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        out.put_u16_le(self.type_id.0);
        out.put_u16_le(self.data.len() as u16);
        out.put_slice(&self.data);
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.data.len());
        self.encode_into(&mut out);
        out
    }
}

/// A common packet format item list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommonPacket {
    pub items: Vec<CommonPacketItem>,
}

impl CommonPacket {
    pub fn new(items: Vec<CommonPacketItem>) -> Self {
        Self { items }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.put_u16_le(self.items.len() as u16);
        for item in &self.items {
            item.encode_into(&mut out);
        }
        out
    }

    /// Decodes up to the item count; trailing bytes after the last item are ignored.
    pub fn decode(mut buf: &[u8]) -> Result<Self> {
        let count = buf.try_get_u16_le()?;
        let mut items = Vec::with_capacity(count as usize);
        for _ in 0..count {
            if buf.is_empty() {
                break;
            }
            let type_id = CommonPacketItemId(buf.try_get_u16_le()?);
            let len = buf.try_get_u16_le()? as usize;
            let data = buf
                .try_get_vec(len)
                .map_err(|_| Error::InvalidPacket("common packet item is truncated".into()))?;
            items.push(CommonPacketItem { type_id, data });
        }
        Ok(Self { items })
    }
}

/// An implicit (class 0/1) packet: sequenced address item plus connected data item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedPacket<'a> {
    pub connection_id: u32,
    pub sequence_number: u32,
    pub data: &'a [u8],
}

/// Decodes an implicit packet without allocating.
pub fn decode_connected_packet(mut buf: &[u8]) -> Result<ConnectedPacket<'_>> {
    let count = buf.try_get_u16_le()?;
    if count < 2 {
        return Err(Error::InvalidPacket(format!(
            "implicit packet has {count} items, expected 2"
        )));
    }
    let addr_type = CommonPacketItemId(buf.try_get_u16_le()?);
    let addr_len = buf.try_get_u16_le()?;
    if addr_type != CommonPacketItemId::SEQUENCED_ADDRESS || addr_len != 8 {
        return Err(Error::InvalidPacket(format!(
            "implicit packet address item is {addr_type} with length {addr_len}"
        )));
    }
    let connection_id = buf.try_get_u32_le()?;
    let sequence_number = buf.try_get_u32_le()?;
    let data_type = CommonPacketItemId(buf.try_get_u16_le()?);
    if data_type != CommonPacketItemId::CONNECTED_TRANSPORT_PACKET {
        return Err(Error::InvalidPacket(format!(
            "implicit packet data item is {data_type}"
        )));
    }
    let len = buf.try_get_u16_le()? as usize;
    if buf.len() < len {
        return Err(Error::Truncated {
            requested: len,
            available: buf.len(),
        });
    }
    Ok(ConnectedPacket {
        connection_id,
        sequence_number,
        data: &buf[..len],
    })
}

/// Encodes an implicit packet into `out` (cleared first) from payload parts, so
/// the caller can reuse one buffer for every send.
pub fn encode_connected_packet(
    out: &mut Vec<u8>,
    connection_id: u32,
    sequence_number: u32,
    parts: &[&[u8]],
) {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    out.clear();
    out.put_u16_le(2);
    out.put_u16_le(CommonPacketItemId::SEQUENCED_ADDRESS.0);
    out.put_u16_le(8);
    out.put_u32_le(connection_id);
    out.put_u32_le(sequence_number);
    out.put_u16_le(CommonPacketItemId::CONNECTED_TRANSPORT_PACKET.0);
    out.put_u16_le(len as u16);
    for part in parts {
        out.put_slice(part);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_encoding() {
        assert_eq!(
            CommonPacketItem::unconnected_data(vec![1, 2, 3, 4]).encode(),
            [0xB2, 0, 4, 0, 1, 2, 3, 4]
        );
        assert_eq!(CommonPacketItem::null_address().encode(), [0, 0, 0, 0]);
    }

    #[test]
    fn decode_packet() {
        let cp = CommonPacket::new(vec![
            CommonPacketItem::null_address(),
            CommonPacketItem::unconnected_data(vec![0, 2]),
        ]);
        let decoded = CommonPacket::decode(&cp.encode()).unwrap();
        assert_eq!(decoded.items[0].type_id, CommonPacketItemId::NULL_ADDRESS);
        assert_eq!(
            decoded.items[1].type_id,
            CommonPacketItemId::UNCONNECTED_MESSAGE
        );
        assert_eq!(decoded, cp);
    }

    #[test]
    fn decode_truncated_packet() {
        let cp = CommonPacket::new(vec![
            CommonPacketItem::unconnected_data(vec![]),
            CommonPacketItem::unconnected_data(vec![]),
        ]);
        let mut data = cp.encode();
        data.pop();
        assert!(CommonPacket::decode(&data).is_err());
    }

    #[test]
    fn connected_packet_round_trip() {
        let mut out = Vec::new();
        encode_connected_packet(&mut out, 0x11223344, 7, &[&[1, 0], &[0xAA, 0xBB]]);
        let generic = CommonPacket::new(vec![
            CommonPacketItem::sequenced_address(0x11223344, 7),
            CommonPacketItem::connected_data(vec![1, 0, 0xAA, 0xBB]),
        ]);
        assert_eq!(out, generic.encode());

        let pkt = decode_connected_packet(&out).unwrap();
        assert_eq!(pkt.connection_id, 0x11223344);
        assert_eq!(pkt.sequence_number, 7);
        assert_eq!(pkt.data, [1, 0, 0xAA, 0xBB]);
        assert!(decode_connected_packet(&out[..out.len() - 1]).is_err());
    }
}
