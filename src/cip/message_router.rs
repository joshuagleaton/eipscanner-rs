use bytes::{Buf, BufMut};

use super::{EPath, GeneralStatusCode, SegmentSize, ServiceCode};
use crate::codec::BufExt;
use crate::eip::CommonPacketItem;
use crate::{Error, Result};

/// Encodes a message router request: service, path size in words, path, data.
pub fn encode_request(
    service: ServiceCode,
    path: &EPath,
    data: &[u8],
    size: SegmentSize,
) -> Vec<u8> {
    let path = path.encode(size);
    let mut out = Vec::with_capacity(2 + path.len() + data.len());
    out.put_u8(service.0);
    out.put_u8((path.len() / 2) as u8);
    out.put_slice(&path);
    out.put_slice(data);
    out
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessageRouterResponse {
    /// Reply service code; the request's code with bit 0x80 set.
    pub service: ServiceCode,
    pub general_status: GeneralStatusCode,
    pub additional_status: Vec<u16>,
    pub data: Vec<u8>,
    /// Common packet items after the unconnected data item, e.g. the socket
    /// address items in a Forward Open reply.
    pub additional_packet_items: Vec<CommonPacketItem>,
}

impl MessageRouterResponse {
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(Error::InvalidPacket(
                "message router response must have at least 4 bytes".into(),
            ));
        }
        let mut buf = data;
        let service = ServiceCode(buf.get_u8());
        buf.get_u8();
        let general_status = GeneralStatusCode(buf.get_u8());
        let additional_size = buf.get_u8() as usize;
        if additional_size * 2 > buf.remaining() {
            return Err(Error::InvalidPacket(
                "additional status has wrong size".into(),
            ));
        }
        let additional_status = (0..additional_size).map(|_| buf.get_u16_le()).collect();
        let data = buf.try_get_vec(buf.remaining())?;
        Ok(Self {
            service,
            general_status,
            additional_status,
            data,
            additional_packet_items: Vec::new(),
        })
    }

    pub fn is_success(&self) -> bool {
        self.general_status == GeneralStatusCode::SUCCESS
    }

    /// Returns `self` on success, or an [`Error::Cip`] carrying the status codes.
    pub fn check(self, context: impl Into<String>) -> Result<Self> {
        if self.is_success() {
            Ok(self)
        } else {
            Err(Error::Cip {
                context: context.into(),
                status: self.general_status,
                additional: self.additional_status,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_get_attribute_single() {
        let req = encode_request(
            ServiceCode::GET_ATTRIBUTE_SINGLE,
            &EPath::attribute(1, 1, 1),
            &[],
            SegmentSize::Bits16,
        );
        assert_eq!(req, [0x0E, 6, 0x21, 0, 1, 0, 0x25, 0, 1, 0, 0x31, 0, 1, 0]);
    }

    #[test]
    fn decode_with_additional_status() {
        let resp = MessageRouterResponse::decode(&[0x8E, 0, 0x1F, 1, 0x34, 0x12, 0xAA]).unwrap();
        assert_eq!(resp.service, ServiceCode(0x8E));
        assert_eq!(resp.general_status, GeneralStatusCode::VENDOR_SPECIFIC);
        assert_eq!(resp.additional_status, [0x1234]);
        assert_eq!(resp.data, [0xAA]);
        assert!(matches!(resp.check("read"), Err(Error::Cip { .. })));
    }

    #[test]
    fn decode_errors() {
        assert!(MessageRouterResponse::decode(&[0, 0, 0]).is_err());
        assert!(MessageRouterResponse::decode(&[0, 0, 0, 2, 0, 1]).is_err());
    }
}
