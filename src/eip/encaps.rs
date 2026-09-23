use bytes::{Buf, BufMut};

use crate::{Error, Result};

pub const HEADER_SIZE: usize = 24;

wire_code! {
    EncapsCommand(u16) {
        NOP = 0x0000,
        LIST_SERVICES = 0x0004,
        LIST_IDENTITY = 0x0063,
        LIST_INTERFACES = 0x0064,
        REGISTER_SESSION = 0x0065,
        UN_REGISTER_SESSION = 0x0066,
        SEND_RR_DATA = 0x006F,
        SEND_UNIT_DATA = 0x0070,
        INDICATE_STATUS = 0x0072,
        CANCEL = 0x0073,
    }
}

wire_code! {
    EncapsStatus(u32) {
        SUCCESS = 0x0000,
        UNSUPPORTED_COMMAND = 0x0001,
        INSUFFICIENT_MEMORY = 0x0002,
        INVALID_FORMAT_OR_DATA = 0x0003,
        INVALID_SESSION_HANDLE = 0x0064,
        UNSUPPORTED_PROTOCOL_VERSION = 0x0069,
    }
}

/// Encapsulation packet: 24-byte header followed by command data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncapsPacket {
    pub command: EncapsCommand,
    pub session_handle: u32,
    pub status: EncapsStatus,
    pub sender_context: [u8; 8],
    pub options: u32,
    pub data: Vec<u8>,
}

impl EncapsPacket {
    pub fn new(command: EncapsCommand, session_handle: u32, data: Vec<u8>) -> Self {
        Self {
            command,
            session_handle,
            data,
            ..Default::default()
        }
    }

    /// Register Session with protocol version 1 and no options.
    pub fn register_session() -> Self {
        Self::new(EncapsCommand::REGISTER_SESSION, 0, vec![1, 0, 0, 0])
    }

    pub fn unregister_session(session_handle: u32) -> Self {
        Self::new(
            EncapsCommand::UN_REGISTER_SESSION,
            session_handle,
            Vec::new(),
        )
    }

    /// SendRRData with interface handle 0 and the given timeout (seconds).
    pub fn send_rr_data(session_handle: u32, timeout: u16, cpf: &[u8]) -> Self {
        let mut data = Vec::with_capacity(6 + cpf.len());
        data.put_u32_le(0);
        data.put_u16_le(timeout);
        data.put_slice(cpf);
        Self::new(EncapsCommand::SEND_RR_DATA, session_handle, data)
    }

    pub fn list_identity() -> Self {
        Self::new(EncapsCommand::LIST_IDENTITY, 0, Vec::new())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_SIZE + self.data.len());
        out.put_u16_le(self.command.0);
        out.put_u16_le(self.data.len() as u16);
        out.put_u32_le(self.session_handle);
        out.put_u32_le(self.status.0);
        out.put_slice(&self.sender_context);
        out.put_u32_le(self.options);
        out.put_slice(&self.data);
        out
    }

    /// Decodes a complete packet; the data length must match the header.
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(Error::InvalidPacket(format!(
                "encapsulation header must be {HEADER_SIZE} bytes, got {}",
                data.len()
            )));
        }
        let mut buf = data;
        let command = EncapsCommand(buf.get_u16_le());
        let length = buf.get_u16_le() as usize;
        let session_handle = buf.get_u32_le();
        let status = EncapsStatus(buf.get_u32_le());
        let mut sender_context = [0; 8];
        buf.copy_to_slice(&mut sender_context);
        let options = buf.get_u32_le();
        if buf.len() != length {
            return Err(Error::InvalidPacket(format!(
                "encapsulation data must be {length} bytes, got {}",
                buf.len()
            )));
        }
        Ok(Self {
            command,
            session_handle,
            status,
            sender_context,
            options,
            data: buf.to_vec(),
        })
    }

    /// Data length field of an encoded header.
    pub fn length_from_header(header: &[u8; HEADER_SIZE]) -> usize {
        u16::from_le_bytes([header[2], header[3]]) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RR_DATA: [u8; 35] = [
        0x6F, 0, 0xB, 0, 0xDD, 0xCC, 0xBB, 0xAA, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0x64, 0, 1, 2, 3, 4, 5,
    ];

    #[test]
    fn decode() {
        let packet = EncapsPacket::decode(&RR_DATA).unwrap();
        assert_eq!(packet.command, EncapsCommand::SEND_RR_DATA);
        assert_eq!(packet.status, EncapsStatus::SUCCESS);
        assert_eq!(packet.session_handle, 0xaabbccdd);
        assert_eq!(packet.data, [0, 0, 0, 0, 0x64, 0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn decode_errors() {
        assert!(EncapsPacket::decode(&RR_DATA[..23]).is_err());
        let mut long = RR_DATA.to_vec();
        long.push(10);
        assert!(EncapsPacket::decode(&long).is_err());
    }

    #[test]
    fn register_session() {
        let mut expected = vec![0x65, 0, 4, 0];
        expected.extend([0; 20]);
        expected.extend([1, 0, 0, 0]);
        assert_eq!(EncapsPacket::register_session().encode(), expected);
    }

    #[test]
    fn unregister_session() {
        let mut expected = vec![0x66, 0, 0, 0, 0xdd, 0xcc, 0xbb, 0xaa];
        expected.extend([0; 16]);
        assert_eq!(
            EncapsPacket::unregister_session(0xaabbccdd).encode(),
            expected
        );
    }

    #[test]
    fn send_rr_data() {
        let packet = EncapsPacket::send_rr_data(0xaabbccdd, 100, &[1, 2, 3, 4, 5]);
        assert_eq!(packet.encode(), RR_DATA);
    }

    #[test]
    fn list_identity() {
        let mut expected = vec![0x63];
        expected.extend([0; 23]);
        assert_eq!(EncapsPacket::list_identity().encode(), expected);
    }

    #[test]
    fn length_from_header() {
        let header: [u8; HEADER_SIZE] = RR_DATA[..HEADER_SIZE].try_into().unwrap();
        assert_eq!(EncapsPacket::length_from_header(&header), 11);
    }
}
