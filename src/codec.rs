//! Helpers on top of `bytes::Buf` for CIP types that span several fields.

use bytes::Buf;

use crate::{Error, Result};

pub(crate) trait BufExt: Buf {
    fn try_get_vec(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut v = vec![0; len];
        self.try_copy_to_slice(&mut v)?;
        Ok(v)
    }

    /// CIP SHORT_STRING: USINT length, then that many bytes.
    fn try_get_short_string(&mut self) -> Result<String> {
        let len = self.try_get_u8()? as usize;
        Ok(String::from_utf8_lossy(&self.try_get_vec(len)?).into_owned())
    }

    fn try_skip(&mut self, len: usize) -> Result<()> {
        if self.remaining() < len {
            return Err(Error::Truncated {
                requested: len,
                available: self.remaining(),
            });
        }
        self.advance(len);
        Ok(())
    }
}

impl<B: Buf + ?Sized> BufExt for B {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_string() {
        let buf = b"\x06Hello!";
        let mut rd = &buf[..];
        assert_eq!(rd.try_get_short_string().unwrap(), "Hello!");
        assert!(rd.is_empty());
    }

    #[test]
    fn truncated_read_is_an_error() {
        let mut rd: &[u8] = &[3, b'a'];
        assert!(matches!(
            rd.try_get_short_string(),
            Err(Error::Truncated {
                requested: 3,
                available: 1
            })
        ));
    }

    #[test]
    fn little_endian_floats() {
        use bytes::BufMut;
        let mut buf = Vec::new();
        buf.put_f32_le(10.5);
        buf.put_f64_le(10.5);
        assert_eq!(buf, [0, 0, 0x28, 0x41, 0, 0, 0, 0, 0, 0, 0x25, 0x40]);
    }
}
