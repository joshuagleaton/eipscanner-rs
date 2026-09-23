use bytes::Buf;
use tracing::debug;

use crate::cip::{CipDataType, EPath, ServiceCode};
use crate::codec::BufExt;
use crate::session::Session;
use crate::{Error, MessageRouter, Result};

mod attr {
    pub const VALUE: u16 = 1;
    pub const DATA_SIZE: u16 = 6;
    pub const SCALING_MULTIPLIER: u16 = 13;
    pub const SCALING_OFFSET: u16 = 16;
}

mod descriptor {
    pub const SUPPORTS_SCALING: u16 = 1 << 2;
    pub const READ_ONLY: u16 = 1 << 4;
}

/// A fixed-size little-endian CIP value that can be scaled as a float.
pub trait CipValue: Copy {
    const SIZE: usize;
    fn decode(bytes: &[u8]) -> Self;
    fn encode(self) -> Vec<u8>;
    fn to_f64(self) -> f64;
    /// Integers round to the nearest value.
    fn from_f64(v: f64) -> Self;
}

macro_rules! impl_cip_value {
    ($($t:ty => $conv:expr),* $(,)?) => {$(
        impl CipValue for $t {
            const SIZE: usize = std::mem::size_of::<$t>();
            fn decode(bytes: &[u8]) -> Self {
                <$t>::from_le_bytes(bytes.try_into().expect("length checked by caller"))
            }
            fn encode(self) -> Vec<u8> {
                self.to_le_bytes().to_vec()
            }
            fn to_f64(self) -> f64 {
                self as f64
            }
            fn from_f64(v: f64) -> Self {
                let conv: fn(f64) -> f64 = $conv;
                conv(v) as $t
            }
        }
    )*};
}

impl_cip_value! {
    u8 => f64::round, i8 => f64::round,
    u16 => f64::round, i16 => f64::round,
    u32 => f64::round, i32 => f64::round,
    u64 => f64::round, i64 => f64::round,
    f32 => |v| v, f64 => |v| v,
}

/// Parameter object (class 0x0F). Read only.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterObject {
    pub instance_id: u16,
    pub has_full_attributes: bool,
    pub is_scalable: bool,
    pub is_read_only: bool,
    pub data_type: CipDataType,
    pub name: String,
    pub units: String,
    pub help: String,
    pub scaling_multiplier: u16,
    pub scaling_divisor: u16,
    pub scaling_base: u16,
    pub scaling_offset: i16,
    pub precision: u8,
    value: Vec<u8>,
    min_value: Vec<u8>,
    max_value: Vec<u8>,
    default_value: Vec<u8>,
}

impl ParameterObject {
    pub const CLASS_ID: u16 = 0x0F;

    /// A parameter with zeroed values of `type_size` bytes and no scaling,
    /// built without talking to a device.
    pub fn new(instance_id: u16, has_full_attributes: bool, type_size: usize) -> Self {
        Self {
            instance_id,
            has_full_attributes,
            is_scalable: false,
            is_read_only: false,
            data_type: CipDataType::ANY,
            name: String::new(),
            units: String::new(),
            help: String::new(),
            scaling_multiplier: 1,
            scaling_divisor: 1,
            scaling_base: 1,
            scaling_offset: 0,
            precision: 0,
            value: vec![0; type_size],
            min_value: vec![0; type_size],
            max_value: vec![0; type_size],
            default_value: vec![0; type_size],
        }
    }

    /// Reads the parameter from a device. With `full_attributes`, also reads
    /// the name, units, help, limits, and scaling.
    pub fn read(
        session: &dyn Session,
        router: &MessageRouter,
        instance_id: u16,
        full_attributes: bool,
    ) -> Result<Self> {
        let size_reply = router
            .send_request(
                session,
                ServiceCode::GET_ATTRIBUTE_SINGLE,
                &EPath::attribute(Self::CLASS_ID, instance_id, attr::DATA_SIZE),
                &[],
            )?
            .check("read parameter data size")?;
        let data_size = (&size_reply.data[..]).try_get_u8()? as usize;
        let mut param = Self::new(instance_id, full_attributes, data_size);

        let all = router
            .send_request(
                session,
                ServiceCode::GET_ATTRIBUTE_ALL,
                &EPath::instance(Self::CLASS_ID, instance_id),
                &[],
            )?
            .check("read parameter attributes")?;
        let mut buf = &all.data[..];
        param.value = buf.try_get_vec(data_size)?;
        let link_path_size = buf.try_get_u8()? as usize;
        buf.try_skip(link_path_size)?;
        let desc = buf.try_get_u16_le()?;
        param.data_type = CipDataType(buf.try_get_u8()?);
        param.is_scalable = desc & descriptor::SUPPORTS_SCALING != 0;
        param.is_read_only = desc & descriptor::READ_ONLY != 0;
        debug!(instance_id, descriptor = desc, "read parameter descriptor");

        if full_attributes {
            buf.try_skip(1)?; // data size, read above
            param.name = buf.try_get_short_string()?;
            param.units = buf.try_get_short_string()?;
            param.help = buf.try_get_short_string()?;
            param.min_value = buf.try_get_vec(data_size)?;
            param.max_value = buf.try_get_vec(data_size)?;
            param.default_value = buf.try_get_vec(data_size)?;
            if param.is_scalable {
                // Scaling attributes 13-16 and their links 17-20, then precision.
                buf.try_skip(16)?;
                param.precision = buf.try_get_u8()?;

                // Scaling values are read individually, matching the C++ library.
                let mut scaling = Vec::with_capacity(8);
                for id in attr::SCALING_MULTIPLIER..=attr::SCALING_OFFSET {
                    let reply = router
                        .send_request(
                            session,
                            ServiceCode::GET_ATTRIBUTE_SINGLE,
                            &EPath::attribute(Self::CLASS_ID, instance_id, id),
                            &[],
                        )?
                        .check(format!("read parameter attribute {id}"))?;
                    scaling.extend(reply.data);
                }
                let mut buf = &scaling[..];
                param.scaling_multiplier = buf.try_get_u16_le()?;
                param.scaling_divisor = buf.try_get_u16_le()?;
                param.scaling_base = buf.try_get_u16_le()?;
                param.scaling_offset = buf.try_get_i16_le()?;
            }
        }
        debug!(instance_id, name = %param.name, data_type = %param.data_type, "read parameter object");
        Ok(param)
    }

    /// Re-reads the value attribute.
    pub fn update_value(&mut self, session: &dyn Session, router: &MessageRouter) -> Result<()> {
        let reply = router
            .send_request(
                session,
                ServiceCode::GET_ATTRIBUTE_SINGLE,
                &EPath::attribute(Self::CLASS_ID, self.instance_id, attr::VALUE),
                &[],
            )?
            .check("read parameter value")?;
        self.value = reply.data;
        Ok(())
    }

    pub fn raw_value(&self) -> &[u8] {
        &self.value
    }

    pub fn actual_value<T: CipValue>(&self) -> Result<T> {
        decode(&self.value)
    }

    pub fn min_value<T: CipValue>(&self) -> Result<T> {
        decode(&self.min_value)
    }

    pub fn max_value<T: CipValue>(&self) -> Result<T> {
        decode(&self.max_value)
    }

    pub fn default_value<T: CipValue>(&self) -> Result<T> {
        decode(&self.default_value)
    }

    pub fn eng_value<T: CipValue>(&self) -> Result<f64> {
        Ok(self.actual_to_eng(self.actual_value::<T>()?.to_f64()))
    }

    pub fn eng_min_value<T: CipValue>(&self) -> Result<f64> {
        Ok(self.actual_to_eng(self.min_value::<T>()?.to_f64()))
    }

    pub fn eng_max_value<T: CipValue>(&self) -> Result<f64> {
        Ok(self.actual_to_eng(self.max_value::<T>()?.to_f64()))
    }

    pub fn eng_default_value<T: CipValue>(&self) -> Result<f64> {
        Ok(self.actual_to_eng(self.default_value::<T>()?.to_f64()))
    }

    pub fn set_eng_min_value<T: CipValue>(&mut self, value: f64) {
        self.min_value = T::from_f64(self.eng_to_actual(value)).encode();
    }

    pub fn set_eng_max_value<T: CipValue>(&mut self, value: f64) {
        self.max_value = T::from_f64(self.eng_to_actual(value)).encode();
    }

    pub fn set_eng_default_value<T: CipValue>(&mut self, value: f64) {
        self.default_value = T::from_f64(self.eng_to_actual(value)).encode();
    }

    /// `(actual + offset) * multiplier * base / (divisor * 10^precision)` when scalable.
    pub fn actual_to_eng(&self, actual: f64) -> f64 {
        if !self.is_scalable {
            return actual;
        }
        (actual + self.scaling_offset as f64)
            * self.scaling_multiplier as f64
            * self.scaling_base as f64
            / (self.scaling_divisor as f64 * 10f64.powi(self.precision as i32))
    }

    pub fn eng_to_actual(&self, eng: f64) -> f64 {
        if !self.is_scalable {
            return eng;
        }
        eng * self.scaling_divisor as f64 * 10f64.powi(self.precision as i32)
            / (self.scaling_multiplier as f64 * self.scaling_base as f64)
            - self.scaling_offset as f64
    }
}

fn decode<T: CipValue>(bytes: &[u8]) -> Result<T> {
    if bytes.len() != T::SIZE {
        return Err(Error::InvalidPacket(format!(
            "parameter value is {} bytes, requested type is {}",
            bytes.len(),
            T::SIZE
        )));
    }
    Ok(T::decode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockSession;

    const ID: u16 = 1;

    fn param_data(descriptor: u8) -> Vec<u8> {
        let mut d = vec![
            0x01, 0, 0, 0, 6, 0x20, 0x05, 0x24, 0x02, 0x30, 0x01, descriptor, 0,
        ];
        d.push(CipDataType::UDINT.0);
        d.push(4);
        d.extend(b"\x05PARAM\x03MPa\x04HELP");
        d.extend([0, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0]);
        d.extend([2, 0, 4, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        d
    }

    fn session(full: bool, descriptor: u8) -> MockSession {
        let s = MockSession::new()
            .reply(&[4])
            .reply(&param_data(descriptor));
        if full && descriptor & 0x04 != 0 {
            s.reply(&[2, 0])
                .reply(&[4, 0])
                .reply(&[1, 0])
                .reply(&[6, 0])
        } else {
            s
        }
    }

    #[test]
    fn read_stub_data() {
        let s = session(false, 0x04);
        let p = ParameterObject::read(&s, &MessageRouter::new(), ID, false).unwrap();
        assert!(!p.has_full_attributes);
        assert!(p.is_scalable);
        assert!(!p.is_read_only);
        assert_eq!(p.actual_value::<u32>().unwrap(), 1);
        assert_eq!(p.data_type, CipDataType::UDINT);
        assert_eq!(p.name, "");
        let reqs = s.requests();
        assert_eq!(reqs[0].path, EPath::attribute(0x0F, ID, 6));
        assert_eq!(reqs[1].service, ServiceCode::GET_ATTRIBUTE_ALL);
    }

    #[test]
    fn read_full_data() {
        let s = session(true, 0x04);
        let p = ParameterObject::read(&s, &MessageRouter::new(), ID, true).unwrap();
        assert!(p.has_full_attributes);
        assert_eq!(p.name, "PARAM");
        assert_eq!(p.units, "MPa");
        assert_eq!(p.help, "HELP");
        assert_eq!(p.min_value::<u32>().unwrap(), 0);
        assert_eq!(p.max_value::<u32>().unwrap(), 5);
        assert_eq!(p.default_value::<u32>().unwrap(), 3);
        assert_eq!(
            (
                p.scaling_multiplier,
                p.scaling_divisor,
                p.scaling_base,
                p.scaling_offset,
                p.precision
            ),
            (2, 4, 1, 6, 1)
        );
        assert!((p.eng_value::<u32>().unwrap() - 0.35).abs() < 1e-12);
        assert!((p.eng_min_value::<u32>().unwrap() - 0.3).abs() < 1e-12);
        assert!((p.eng_max_value::<u32>().unwrap() - 0.55).abs() < 1e-12);
        assert!((p.eng_default_value::<u32>().unwrap() - 0.45).abs() < 1e-12);
        let ids: Vec<_> = s.requests()[2..]
            .iter()
            .map(|r| r.path.attribute_id())
            .collect();
        assert_eq!(ids, [Some(13), Some(14), Some(15), Some(16)]);
    }

    #[test]
    fn update_value() {
        let s = session(true, 0x04).reply(&[4, 0, 0, 0]);
        let router = MessageRouter::new();
        let mut p = ParameterObject::read(&s, &router, ID, true).unwrap();
        p.update_value(&s, &router).unwrap();
        assert_eq!(p.actual_value::<u32>().unwrap(), 4);
        assert_eq!(
            s.requests().last().unwrap().path,
            EPath::attribute(0x0F, ID, 1)
        );
    }

    #[test]
    fn offline_setters() {
        let mut p = ParameterObject::new(ID, true, 4);
        p.is_read_only = true;
        p.is_scalable = true;
        p.scaling_multiplier = 2;
        p.scaling_divisor = 4;
        p.scaling_base = 1;
        p.scaling_offset = 6;
        p.precision = 1;
        assert!((p.eng_value::<u32>().unwrap() - 0.3).abs() < 1e-12);
        p.set_eng_min_value::<u32>(0.3);
        assert_eq!(p.min_value::<u32>().unwrap(), 0);
        p.set_eng_max_value::<u32>(0.55);
        assert_eq!(p.max_value::<u32>().unwrap(), 5);
        p.set_eng_default_value::<u32>(0.45);
        assert_eq!(p.default_value::<u32>().unwrap(), 3);
    }

    #[test]
    fn read_only_descriptor() {
        let s = session(false, 0x10);
        let p = ParameterObject::read(&s, &MessageRouter::new(), ID, false).unwrap();
        assert!(p.is_read_only);
    }

    #[test]
    fn wrong_type_size() {
        let p = ParameterObject::new(ID, false, 4);
        assert!(p.actual_value::<u16>().is_err());
    }
}
