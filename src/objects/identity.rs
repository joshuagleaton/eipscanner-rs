use bytes::Buf;

use crate::cip::{CipRevision, EPath, ServiceCode};
use crate::codec::BufExt;
use crate::session::Session;
use crate::{MessageRouter, Result};

/// Identity object (class 0x01).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityObject {
    pub instance_id: u16,
    pub vendor_id: u16,
    pub device_type: u16,
    pub product_code: u16,
    pub revision: CipRevision,
    pub status: u16,
    pub serial_number: u32,
    pub product_name: String,
}

impl IdentityObject {
    pub const CLASS_ID: u16 = 0x01;

    /// Reads all attributes with Get_Attributes_All.
    pub fn read(session: &dyn Session, router: &MessageRouter, instance_id: u16) -> Result<Self> {
        let response = router
            .send_request(
                session,
                ServiceCode::GET_ATTRIBUTE_ALL,
                &EPath::instance(Self::CLASS_ID, instance_id),
                &[],
            )?
            .check("read identity object")?;
        let mut buf = &response.data[..];
        Ok(Self {
            instance_id,
            vendor_id: buf.try_get_u16_le()?,
            device_type: buf.try_get_u16_le()?,
            product_code: buf.try_get_u16_le()?,
            revision: CipRevision::decode(&mut buf)?,
            status: buf.try_get_u16_le()?,
            serial_number: buf.try_get_u32_le()?,
            product_name: buf.try_get_short_string()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cip::GeneralStatusCode;
    use crate::test_support::MockSession;
    use crate::{Error, MessageRouter};

    #[test]
    fn read_all_attributes() {
        let session = MockSession::new().reply(&[
            1, 0, 2, 0, 3, 0, 4, 5, 0x30, 0, 0x78, 0x56, 0x34, 0x12, 5, b'D', b'r', b'i', b'v',
            b'e',
        ]);
        let identity = IdentityObject::read(&session, &MessageRouter::new(), 1).unwrap();
        assert_eq!(identity.vendor_id, 1);
        assert_eq!(identity.device_type, 2);
        assert_eq!(identity.product_code, 3);
        assert_eq!(identity.revision, CipRevision::new(4, 5));
        assert_eq!(identity.status, 0x30);
        assert_eq!(identity.serial_number, 0x12345678);
        assert_eq!(identity.product_name, "Drive");

        let req = &session.requests()[0];
        assert_eq!(req.service, ServiceCode::GET_ATTRIBUTE_ALL);
        assert_eq!(req.path, EPath::instance(1, 1));
    }

    #[test]
    fn error_status() {
        let session =
            MockSession::new().reply_status(GeneralStatusCode::OBJECT_DOES_NOT_EXIST, &[]);
        let err = IdentityObject::read(&session, &MessageRouter::new(), 1).unwrap_err();
        assert!(matches!(
            err,
            Error::Cip {
                status: GeneralStatusCode::OBJECT_DOES_NOT_EXIST,
                ..
            }
        ));
    }

    #[test]
    fn short_reply() {
        let session = MockSession::new().reply(&[1, 0, 2]);
        assert!(matches!(
            IdentityObject::read(&session, &MessageRouter::new(), 1),
            Err(Error::Truncated { .. })
        ));
    }
}
