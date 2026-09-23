use bytes::Buf;

use crate::cip::{EPath, ServiceCode};
use crate::codec::BufExt;
use crate::session::Session;
use crate::{MessageRouter, Result};

const FULL_INFORMATION: u16 = 0;
const VALID_DATA: u16 = 1;
const REAL_TIME: u16 = 1 << 1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FullInformation {
    pub fault_code: u16,
    pub dsi_port: u8,
    pub dsi_device_object: u8,
    pub fault_text: String,
    pub timer_value: u64,
    pub is_valid_data: bool,
    pub is_real_time: bool,
}

/// DPI Fault object (class 0x97).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DpiFaultObject {
    pub instance_id: u16,
    pub full_information: FullInformation,
}

impl DpiFaultObject {
    pub const CLASS_ID: u16 = 0x97;

    pub fn read(session: &dyn Session, router: &MessageRouter, instance_id: u16) -> Result<Self> {
        let reply = router
            .send_request(
                session,
                ServiceCode::GET_ATTRIBUTE_SINGLE,
                &EPath::attribute(Self::CLASS_ID, instance_id, FULL_INFORMATION),
                &[],
            )?
            .check("read DPI fault full information")?;
        let mut buf = &reply.data[..];
        let fault_code = buf.try_get_u16_le()?;
        let dsi_port = buf.try_get_u8()?;
        let dsi_device_object = buf.try_get_u8()?;
        let text = buf.try_get_vec(16)?;
        let timer_value = buf.try_get_u64_le()?;
        let flags = buf.try_get_u16_le()?;
        Ok(Self {
            instance_id,
            full_information: FullInformation {
                fault_code,
                dsi_port,
                dsi_device_object,
                fault_text: String::from_utf8_lossy(&text)
                    .trim_end_matches('\0')
                    .to_string(),
                timer_value,
                is_valid_data: flags & VALID_DATA != 0,
                is_real_time: flags & REAL_TIME != 0,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockSession;

    #[test]
    fn read_full_information() {
        let mut data = vec![0x0D, 0x00, 1, 2];
        data.extend(b"Ground Fault\0\0\0\0");
        data.extend(0x0102_0304_0506_0708u64.to_le_bytes());
        data.extend([0x03, 0x00]);
        let s = MockSession::new().reply(&data);
        let obj = DpiFaultObject::read(&s, &MessageRouter::new(), 1).unwrap();
        let info = &obj.full_information;
        assert_eq!(info.fault_code, 13);
        assert_eq!((info.dsi_port, info.dsi_device_object), (1, 2));
        assert_eq!(info.fault_text, "Ground Fault");
        assert_eq!(info.timer_value, 0x0102_0304_0506_0708);
        assert!(info.is_valid_data && info.is_real_time);
        assert_eq!(s.requests()[0].path, EPath::attribute(0x97, 1, 0));
    }
}
