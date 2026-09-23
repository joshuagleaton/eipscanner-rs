use std::fmt;

use bytes::{Buf, BufMut};

use crate::Result;

/// Major and minor revision, encoded as two USINTs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CipRevision {
    pub major: u8,
    pub minor: u8,
}

impl CipRevision {
    pub fn new(major: u8, minor: u8) -> Self {
        Self { major, minor }
    }

    pub fn decode(buf: &mut impl Buf) -> Result<Self> {
        Ok(Self {
            major: buf.try_get_u8()?,
            minor: buf.try_get_u8()?,
        })
    }

    pub fn encode(&self, buf: &mut impl BufMut) {
        buf.put_u8(self.major);
        buf.put_u8(self.minor);
    }
}

impl fmt::Display for CipRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_display() {
        let rev = CipRevision::new(1, 2);
        let mut buf = Vec::new();
        rev.encode(&mut buf);
        assert_eq!(buf, [1, 2]);
        assert_eq!(CipRevision::decode(&mut &buf[..]).unwrap(), rev);
        assert_eq!(rev.to_string(), "1.2");
        assert_eq!(CipRevision::default(), CipRevision::new(0, 0));
    }
}
