use std::fmt;

use bytes::{Buf, BufMut};

use crate::{Error, Result};

const CLASS_8: u8 = 0x20;
const CLASS_16: u8 = 0x21;
const INSTANCE_8: u8 = 0x24;
const INSTANCE_16: u8 = 0x25;
const ATTRIBUTE_8: u8 = 0x30;
const ATTRIBUTE_16: u8 = 0x31;

/// How logical segments are encoded in a padded EPATH.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentSize {
    /// Always 16-bit segments (`0x21 0x00 lo hi`). Matches the C++ library's default.
    #[default]
    Bits16,
    /// 8-bit segments when the value fits, 16-bit otherwise. Some devices
    /// (e.g. Yaskawa MP3300iec) only accept 8-bit segments.
    Bits8,
}

/// A logical path of class, instance, and attribute IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EPath {
    class_id: u16,
    instance_id: Option<u16>,
    attribute_id: Option<u16>,
}

impl EPath {
    pub fn class(class_id: u16) -> Self {
        Self {
            class_id,
            instance_id: None,
            attribute_id: None,
        }
    }

    pub fn instance(class_id: u16, instance_id: u16) -> Self {
        Self {
            class_id,
            instance_id: Some(instance_id),
            attribute_id: None,
        }
    }

    pub fn attribute(class_id: u16, instance_id: u16, attribute_id: u16) -> Self {
        Self {
            class_id,
            instance_id: Some(instance_id),
            attribute_id: Some(attribute_id),
        }
    }

    pub fn class_id(&self) -> u16 {
        self.class_id
    }

    pub fn instance_id(&self) -> Option<u16> {
        self.instance_id
    }

    pub fn attribute_id(&self) -> Option<u16> {
        self.attribute_id
    }

    /// Encodes the path; its size in 16-bit words is `len() / 2`.
    pub fn encode(&self, size: SegmentSize) -> Vec<u8> {
        let mut out = Vec::with_capacity(12);
        put_segment(&mut out, CLASS_8, CLASS_16, self.class_id, size);
        if let Some(id) = self.instance_id {
            put_segment(&mut out, INSTANCE_8, INSTANCE_16, id, size);
        }
        if let Some(id) = self.attribute_id {
            put_segment(&mut out, ATTRIBUTE_8, ATTRIBUTE_16, id, size);
        }
        out
    }

    /// Decodes a padded path containing class, instance, and attribute segments.
    pub fn decode(mut data: &[u8]) -> Result<Self> {
        let mut path = EPath::default();
        while data.has_remaining() {
            let segment = data.try_get_u8()?;
            let value = match segment {
                CLASS_8 | INSTANCE_8 | ATTRIBUTE_8 => data.try_get_u8()? as u16,
                CLASS_16 | INSTANCE_16 | ATTRIBUTE_16 => {
                    data.try_get_u8()?;
                    data.try_get_u16_le()?
                }
                other => {
                    return Err(Error::InvalidPacket(format!(
                        "unknown EPATH segment {other:#04x}"
                    )));
                }
            };
            match segment {
                CLASS_8 | CLASS_16 => path.class_id = value,
                INSTANCE_8 | INSTANCE_16 => path.instance_id = Some(value),
                _ => path.attribute_id = Some(value),
            }
        }
        Ok(path)
    }
}

fn put_segment(out: &mut Vec<u8>, seg8: u8, seg16: u8, value: u16, size: SegmentSize) {
    match size {
        SegmentSize::Bits8 if value <= u8::MAX as u16 => {
            out.put_u8(seg8);
            out.put_u8(value as u8);
        }
        _ => {
            out.put_u8(seg16);
            out.put_u8(0);
            out.put_u16_le(value);
        }
    }
}

impl fmt::Display for EPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[classId={}", self.class_id)?;
        if let Some(id) = self.instance_id {
            write!(f, " objectId={id}")?;
        }
        if let Some(id) = self.attribute_id {
            write!(f, " attributeId={id}")?;
        }
        write!(f, "]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_16_bit() {
        assert_eq!(
            EPath::attribute(5, 2, 1).encode(SegmentSize::Bits16),
            [0x21, 0, 5, 0, 0x25, 0, 2, 0, 0x31, 0, 1, 0]
        );
        assert_eq!(EPath::class(5).encode(SegmentSize::Bits16), [0x21, 0, 5, 0]);
    }

    #[test]
    fn encode_8_bit_falls_back_for_large_ids() {
        assert_eq!(
            EPath::attribute(5, 2, 1).encode(SegmentSize::Bits8),
            [0x20, 5, 0x24, 2, 0x30, 1]
        );
        assert_eq!(
            EPath::instance(4, 300).encode(SegmentSize::Bits8),
            [0x20, 4, 0x25, 0, 0x2c, 0x01]
        );
    }

    #[test]
    fn decode_8_bit() {
        let path = EPath::decode(&[0x20, 0x05, 0x24, 0x02, 0x30, 0x01]).unwrap();
        assert_eq!(path, EPath::attribute(5, 2, 1));
    }

    #[test]
    fn decode_16_bit() {
        let path = EPath::decode(&[0x21, 0, 0x05, 0, 0x25, 0, 0x02, 0, 0x31, 0, 0x01, 0]).unwrap();
        assert_eq!(path, EPath::attribute(5, 2, 1));
    }

    #[test]
    fn decode_mixed() {
        let path = EPath::decode(&[0x21, 0, 0x05, 0, 0x24, 0x02, 0x31, 0, 0x01, 0]).unwrap();
        assert_eq!(path, EPath::attribute(5, 2, 1));
    }

    #[test]
    fn decode_partial_paths() {
        assert_eq!(EPath::decode(&[0x21, 0, 0x05, 0]).unwrap(), EPath::class(5));
        assert_eq!(
            EPath::decode(&[0x21, 0, 0x05, 0, 0x24, 0x02]).unwrap(),
            EPath::instance(5, 2)
        );
    }

    #[test]
    fn decode_errors() {
        assert!(EPath::decode(&[0x21, 0, 0x05, 0, 0xf4, 0x02]).is_err());
        assert!(EPath::decode(&[0x21, 0, 0x05, 0, 0x24]).is_err());
    }

    #[test]
    fn display() {
        assert_eq!(
            EPath::attribute(1, 2, 3).to_string(),
            "[classId=1 objectId=2 attributeId=3]"
        );
        assert_eq!(EPath::class(1).to_string(), "[classId=1]");
    }
}
