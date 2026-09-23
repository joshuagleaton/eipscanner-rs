use bytes::{Buf, BufMut};

use super::{NetworkParams, TransportTrigger};
use crate::Result;
use crate::codec::BufExt;

/// A Forward Open request body, field for field. Most code builds a
/// [`ConnectionConfig`](super::ConnectionConfig) instead and lets
/// [`ConnectionManager`](crate::io::ConnectionManager) fill this in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForwardOpenRequest {
    pub priority_time_tick: u8,
    pub timeout_ticks: u8,
    pub o2t_connection_id: u32,
    pub t2o_connection_id: u32,
    pub connection_serial_number: u16,
    pub originator_vendor_id: u16,
    pub originator_serial_number: u32,
    pub connection_timeout_multiplier: u8,
    /// Microseconds.
    pub o2t_rpi: u32,
    pub o2t_params: NetworkParams,
    /// Microseconds.
    pub t2o_rpi: u32,
    pub t2o_params: NetworkParams,
    pub transport: TransportTrigger,
    /// Complete connection path, including any configuration data segment.
    pub path: Vec<u8>,
}

impl ForwardOpenRequest {
    /// Encodes a Forward Open (`large = false`, 16-bit network parameters) or
    /// Large Forward Open (`large = true`, 32-bit) request body.
    pub fn encode(&self, large: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(40 + self.path.len());
        out.put_u8(self.priority_time_tick);
        out.put_u8(self.timeout_ticks);
        out.put_u32_le(self.o2t_connection_id);
        out.put_u32_le(self.t2o_connection_id);
        out.put_u16_le(self.connection_serial_number);
        out.put_u16_le(self.originator_vendor_id);
        out.put_u32_le(self.originator_serial_number);
        out.put_u8(self.connection_timeout_multiplier);
        out.put_bytes(0, 3);
        out.put_u32_le(self.o2t_rpi);
        put_params(&mut out, &self.o2t_params, large);
        out.put_u32_le(self.t2o_rpi);
        put_params(&mut out, &self.t2o_params, large);
        out.put_u8(self.transport.encode());
        out.put_u8(self.path.len().div_ceil(2) as u8);
        out.put_slice(&self.path);
        out
    }
}

fn put_params(out: &mut Vec<u8>, params: &NetworkParams, large: bool) {
    if large {
        out.put_u32_le(params.encode(true));
    } else {
        out.put_u16_le(params.encode(false) as u16);
    }
}

/// Successful Forward Open reply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForwardOpenResponse {
    pub o2t_network_connection_id: u32,
    pub t2o_network_connection_id: u32,
    pub connection_serial_number: u16,
    pub originator_vendor_id: u16,
    pub originator_serial_number: u32,
    /// Actual packet interval, microseconds.
    pub o2t_api: u32,
    /// Actual packet interval, microseconds.
    pub t2o_api: u32,
    pub application_reply: Vec<u8>,
}

impl ForwardOpenResponse {
    pub fn decode(mut buf: &[u8]) -> Result<Self> {
        let o2t_network_connection_id = buf.try_get_u32_le()?;
        let t2o_network_connection_id = buf.try_get_u32_le()?;
        let connection_serial_number = buf.try_get_u16_le()?;
        let originator_vendor_id = buf.try_get_u16_le()?;
        let originator_serial_number = buf.try_get_u32_le()?;
        let o2t_api = buf.try_get_u32_le()?;
        let t2o_api = buf.try_get_u32_le()?;
        let reply_words = buf.try_get_u8()? as usize;
        buf.try_get_u8()?;
        let application_reply = buf.try_get_vec(reply_words * 2)?;
        Ok(Self {
            o2t_network_connection_id,
            t2o_network_connection_id,
            connection_serial_number,
            originator_vendor_id,
            originator_serial_number,
            o2t_api,
            t2o_api,
            application_reply,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ForwardOpenRequest {
        ForwardOpenRequest {
            priority_time_tick: 0x0A,
            timeout_ticks: 0x0E,
            o2t_connection_id: 0,
            t2o_connection_id: 0x1234_0001,
            connection_serial_number: 1,
            originator_vendor_id: 342,
            originator_serial_number: 0x12345,
            connection_timeout_multiplier: 1,
            o2t_rpi: 10_000,
            o2t_params: NetworkParams::decode(0x4822, false),
            t2o_rpi: 10_000,
            t2o_params: NetworkParams::decode(0x4822, false),
            transport: TransportTrigger::decode(1),
            path: vec![0x20, 0x04, 0x24, 0x01],
        }
    }

    #[test]
    fn forward_open_layout() {
        let body = request().encode(false);
        assert_eq!(body.len(), 36 + 4);
        assert_eq!(&body[..2], [0x0A, 0x0E]);
        assert_eq!(&body[6..10], 0x1234_0001u32.to_le_bytes());
        assert_eq!(&body[26..28], [0x22, 0x48]);
        assert_eq!(&body[34..], [1, 2, 0x20, 0x04, 0x24, 0x01]);
    }

    #[test]
    fn large_forward_open_layout() {
        let body = request().encode(true);
        assert_eq!(body.len(), 40 + 4);
        assert_eq!(&body[26..30], [0x22, 0, 0, 0x48]);
    }

    #[test]
    fn decode_response() {
        let mut data = Vec::new();
        data.put_u32_le(0xAABB_CCDD);
        data.put_u32_le(0x1234_0001);
        data.put_u16_le(1);
        data.put_u16_le(342);
        data.put_u32_le(0x12345);
        data.put_u32_le(10_000);
        data.put_u32_le(20_000);
        data.put_u8(1);
        data.put_u8(0);
        data.put_slice(&[9, 8]);
        let resp = ForwardOpenResponse::decode(&data).unwrap();
        assert_eq!(resp.o2t_network_connection_id, 0xAABB_CCDD);
        assert_eq!(resp.t2o_api, 20_000);
        assert_eq!(resp.application_reply, [9, 8]);
        assert!(ForwardOpenResponse::decode(&data[..20]).is_err());
    }
}
