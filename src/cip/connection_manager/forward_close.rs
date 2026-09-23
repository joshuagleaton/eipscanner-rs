use bytes::BufMut;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForwardCloseRequest {
    pub connection_serial_number: u16,
    pub originator_vendor_id: u16,
    pub originator_serial_number: u32,
    pub connection_path: Vec<u8>,
}

impl ForwardCloseRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.connection_path.len());
        out.put_u8(0); // priority/time tick
        out.put_u8(0); // timeout ticks
        out.put_u16_le(self.connection_serial_number);
        out.put_u16_le(self.originator_vendor_id);
        out.put_u32_le(self.originator_serial_number);
        out.put_u8(self.connection_path.len().div_ceil(2) as u8);
        out.put_u8(0);
        out.put_slice(&self.connection_path);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let req = ForwardCloseRequest {
            connection_serial_number: 0x0102,
            originator_vendor_id: 0x0304,
            originator_serial_number: 0x0506_0708,
            connection_path: vec![0x20, 0x04, 0x24, 0x01],
        };
        assert_eq!(
            req.encode(),
            [
                0, 0, 0x02, 0x01, 0x04, 0x03, 0x08, 0x07, 0x06, 0x05, 2, 0, 0x20, 0x04, 0x24, 0x01
            ]
        );
    }
}
