//! Reader for EDS (Electronic Data Sheet) files.
//!
//! An EDS is the text file a vendor publishes for each EtherNet/IP device. It
//! lists the device's identity, parameters, assemblies (blocks of I/O data),
//! and the I/O connections it supports, including the connection path, data
//! sizes, and allowed packet intervals needed for a Forward Open.
//!
//! [`Eds::parse`] reads the whole file into [`Section`]s of [`Entry`]s. The
//! typed accessors cover the `[Device]`, `[Params]`, `[Assembly]`, and
//! `[Connection Manager]` sections; anything else can be read from the raw
//! sections.
//!
//! ```no_run
//! use eipscanner::eds::Eds;
//!
//! let eds = Eds::read("device.eds")?;
//! let conn = eds.connection(1)?;
//! let mut config = conn.to_connection_config()?;
//! config.originator_vendor_id = 342;
//! config.originator_serial_number = 0x12345;
//! # Ok::<(), eipscanner::Error>(())
//! ```

mod parse;

use std::path::Path;
use std::time::Duration;

use parse::parse_int;
pub use parse::{Entry, Field, Section};

use crate::cip::CipDataType;
use crate::cip::connection_manager::{
    ConnectionConfig, ConnectionPath, ConnectionType, Direction, Priority, SizeType,
    TransportClass, Trigger,
};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eds {
    pub sections: Vec<Section>,
}

impl Eds {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(Self {
            sections: parse::parse(text)?,
        })
    }

    /// Reads and parses a file. Bytes that aren't valid UTF-8 (EDS files are
    /// often Latin-1) are replaced rather than rejected.
    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        Self::parse(&String::from_utf8_lossy(&std::fs::read(path)?))
    }

    /// Finds a section by name, ignoring case.
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    fn entry(&self, section: &str, keyword: &str) -> Result<&Entry> {
        self.section(section)
            .and_then(|s| s.get(keyword))
            .ok_or_else(|| Error::Eds(format!("[{section}] has no entry '{keyword}'")))
    }

    pub fn device(&self) -> Result<EdsDevice> {
        let text = |key: &str| -> Result<String> {
            Ok(self
                .entry("Device", key)?
                .field(0)
                .text()
                .unwrap_or_default()
                .to_string())
        };
        let int = |key: &str| -> Result<u64> {
            let e = self.entry("Device", key)?;
            int_field(e, 0)?.ok_or_else(|| Error::Eds(format!("line {}: {key} is empty", e.line)))
        };
        Ok(EdsDevice {
            vendor_id: int("VendCode")? as u16,
            vendor_name: text("VendName")?,
            product_type: int("ProdType")? as u16,
            product_code: int("ProdCode")? as u16,
            major_revision: int("MajRev")? as u8,
            minor_revision: int("MinRev")? as u8,
            product_name: text("ProdName")?,
            catalog: text("Catalog").unwrap_or_default(),
        })
    }

    /// `ParamN` from `[Params]`.
    pub fn param(&self, number: u32) -> Result<EdsParam> {
        let e = self.entry("Params", &format!("Param{number}"))?;
        let text = |i: usize| e.field(i).text().map(str::to_string);
        Ok(EdsParam {
            number,
            data_type: CipDataType(int_field(e, 4)?.unwrap_or(0) as u8),
            data_size: int_field(e, 5)?.unwrap_or(0) as usize,
            name: text(6).unwrap_or_default(),
            units: text(7).unwrap_or_default(),
            help: text(8).unwrap_or_default(),
            min: text(9),
            max: text(10),
            default: text(11),
        })
    }

    /// `AssemN` from `[Assembly]`.
    pub fn assembly(&self, number: u32) -> Result<EdsAssembly> {
        let e = self.entry("Assembly", &format!("Assem{number}"))?;
        // Fields 6.. are (size in bits, member) pairs.
        let member_bits: u64 = e.fields[6.min(e.fields.len())..]
            .iter()
            .step_by(2)
            .filter_map(|f| f.text().and_then(parse_int))
            .sum();
        let size = match int_field(e, 2)? {
            Some(bytes) => bytes as usize,
            None => member_bits.div_ceil(8) as usize,
        };
        Ok(EdsAssembly {
            number,
            name: e.field(0).text().unwrap_or_default().to_string(),
            size,
        })
    }

    /// All `ConnectionN` entries of `[Connection Manager]`, in file order.
    pub fn connections(&self) -> Result<Vec<EdsConnection>> {
        let Some(section) = self.section("Connection Manager") else {
            return Ok(Vec::new());
        };
        section
            .entries
            .iter()
            .filter_map(|e| numbered(&e.keyword, "Connection").map(|n| (n, e)))
            .map(|(n, e)| self.decode_connection(n, e))
            .collect()
    }

    pub fn connection(&self, number: u32) -> Result<EdsConnection> {
        let e = self.entry("Connection Manager", &format!("Connection{number}"))?;
        self.decode_connection(number, e)
    }

    fn decode_connection(&self, number: u32, e: &Entry) -> Result<EdsConnection> {
        let required = |i: usize, what: &str| -> Result<u32> {
            int_field(e, i)?.map(|v| v as u32).ok_or_else(|| {
                Error::Eds(format!("line {}: Connection{number} has no {what}", e.line))
            })
        };
        let path_text = e.field(14).text().unwrap_or_default();
        Ok(EdsConnection {
            number,
            name: e.field(12).text().unwrap_or_default().to_string(),
            help: e.field(13).text().unwrap_or_default().to_string(),
            trigger_transport: required(0, "trigger and transport field")?,
            connection_params: required(1, "connection parameters field")?,
            o2t: self.direction(e, 2)?,
            t2o: self.direction(e, 5)?,
            config_assembly: [9, 11]
                .iter()
                .find_map(|&i| e.field(i).text().and_then(|t| numbered(t, "Assem"))),
            path: self
                .resolve_path(path_text)
                .map_err(|err| Error::Eds(format!("line {}: Connection{number}: {err}", e.line)))?,
        })
    }

    /// RPI, size, and format fields starting at `first`.
    fn direction(&self, e: &Entry, first: usize) -> Result<EdsDirection> {
        let rpi = match e.field(first).text() {
            None => None,
            Some(t) => Some(match numbered(t, "Param") {
                Some(n) => {
                    let p = self.param(n)?;
                    let int =
                        |v: &Option<String>| v.as_deref().and_then(parse_int).map(|v| v as u32);
                    EdsRpi {
                        min: int(&p.min),
                        max: int(&p.max),
                        default: int(&p.default),
                    }
                }
                None => {
                    let v = parse_int(t).map(|v| v as u32);
                    EdsRpi {
                        min: v,
                        max: v,
                        default: v,
                    }
                }
            }),
        };
        let assembly = e.field(first + 2).text().and_then(|t| numbered(t, "Assem"));
        let size = match e.field(first + 1).text() {
            Some(t) => match numbered(t, "Param") {
                Some(n) => self.param(n)?.default_int().unwrap_or(0) as usize,
                None => parse_int(t).ok_or_else(|| {
                    Error::Eds(format!("line {}: bad connection size '{t}'", e.line))
                })? as usize,
            },
            None => match assembly {
                Some(n) => self.assembly(n)?.size,
                None => 0,
            },
        };
        Ok(EdsDirection {
            rpi,
            size,
            assembly,
        })
    }

    /// Converts an EDS path string: hex bytes, and `ParamN` or `[ParamN]`
    /// references, which are replaced by the parameter's default value.
    fn resolve_path(&self, text: &str) -> Result<ConnectionPath> {
        let mut bytes = Vec::new();
        for token in text.split_whitespace() {
            let name = token.trim_start_matches('[').trim_end_matches(']');
            if let Some(n) = numbered(name, "Param") {
                let p = self.param(n)?;
                let value = p
                    .default_int()
                    .ok_or_else(|| Error::Eds(format!("Param{n} in path has no default value")))?;
                bytes.extend_from_slice(&value.to_le_bytes()[..p.data_size.clamp(1, 8)]);
            } else {
                let b = u8::from_str_radix(token, 16)
                    .map_err(|_| Error::Eds(format!("unsupported path token '{token}'")))?;
                bytes.push(b);
            }
        }
        Ok(ConnectionPath::from(bytes))
    }
}

/// `"Param12"` with prefix `"Param"` gives 12; case is ignored.
fn numbered(text: &str, prefix: &str) -> Option<u32> {
    let head = text.get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    text[prefix.len()..].parse().ok()
}

fn int_field(e: &Entry, i: usize) -> Result<Option<u64>> {
    match e.field(i).text() {
        None => Ok(None),
        Some(t) => parse_int(t).map(Some).ok_or_else(|| {
            Error::Eds(format!(
                "line {}: '{}' field {i} is not a number: '{t}'",
                e.line, e.keyword
            ))
        }),
    }
}

/// The `[Device]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdsDevice {
    pub vendor_id: u16,
    pub vendor_name: String,
    pub product_type: u16,
    pub product_code: u16,
    pub major_revision: u8,
    pub minor_revision: u8,
    pub product_name: String,
    pub catalog: String,
}

/// A `ParamN` entry. Limits and default are kept as text since their type
/// depends on `data_type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdsParam {
    pub number: u32,
    pub data_type: CipDataType,
    pub data_size: usize,
    pub name: String,
    pub units: String,
    pub help: String,
    pub min: Option<String>,
    pub max: Option<String>,
    pub default: Option<String>,
}

impl EdsParam {
    pub fn default_int(&self) -> Option<u64> {
        self.default.as_deref().and_then(parse_int)
    }
}

/// An `AssemN` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdsAssembly {
    pub number: u32,
    pub name: String,
    /// Bytes; from the size field, or the sum of the member sizes if it's empty.
    pub size: usize,
}

/// Allowed packet interval, microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdsRpi {
    pub min: Option<u32>,
    pub max: Option<u32>,
    pub default: Option<u32>,
}

/// One direction (O->T or T->O) of a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdsDirection {
    pub rpi: Option<EdsRpi>,
    /// Application data size in bytes; 0 for a heartbeat.
    pub size: usize,
    /// The `AssemN` the data is described by, if any.
    pub assembly: Option<u32>,
}

/// Real-time format value meaning "32-bit run/idle header".
const FORMAT_HEADER: u32 = 4;

/// A `ConnectionN` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdsConnection {
    pub number: u32,
    pub name: String,
    pub help: String,
    /// Bits 0-15 supported transport classes, 16-18 triggers (cyclic, change
    /// of state, application), 24-27 transport types (listen-only, input-only,
    /// exclusive owner, redundant owner).
    pub trigger_transport: u32,
    /// Bits 0-3 fixed/variable size support (O->T fixed, O->T variable, T->O
    /// fixed, T->O variable), 8-10 and 12-14 O->T and T->O real-time format,
    /// 16-18 and 20-22 connection types (null, multicast, point-to-point),
    /// 24-26 and 28-30 priorities (low, high, scheduled).
    pub connection_params: u32,
    pub o2t: EdsDirection,
    pub t2o: EdsDirection,
    /// Configuration assembly. The EDS doesn't give its data; see
    /// [`ConnectionConfig::config_data`].
    pub config_assembly: Option<u32>,
    pub path: ConnectionPath,
}

impl EdsConnection {
    fn bit(v: u32, n: u32) -> bool {
        v & (1 << n) != 0
    }

    /// Builds a connection config from the EDS entry: path, sizes, packet
    /// intervals (the default, or 10 ms if the EDS gives none), transport
    /// class and trigger, connection types, priorities, and whether each
    /// direction has a run/idle header. Point-to-point is preferred, since
    /// multicast input isn't supported. The originator vendor ID and serial
    /// number, and any configuration data, are left for the caller.
    pub fn to_connection_config(&self) -> Result<ConnectionConfig> {
        let tt = self.trigger_transport;
        let cp = self.connection_params;
        let unsupported = |what: &str| {
            Error::Unsupported(format!("Connection{} ({}): {what}", self.number, self.name))
        };

        let transport_class = if Self::bit(tt, 1) {
            TransportClass::Class1
        } else if Self::bit(tt, 0) {
            TransportClass::Class0
        } else {
            return Err(unsupported(
                "only transport classes 0 and 1 are supported for I/O",
            ));
        };
        let trigger = if Self::bit(tt, 17) && !Self::bit(tt, 16) {
            Trigger::ChangeOfState
        } else if Self::bit(tt, 18) && !Self::bit(tt, 16) {
            Trigger::Application
        } else {
            Trigger::Cyclic
        };

        // O->T and T->O use the same bit layout, 4 bits apart.
        let direction = |shift: u32, fixed_bit: u32, dir: &EdsDirection, name: &str| {
            let connection_type = if Self::bit(cp, 18 + shift) {
                ConnectionType::PointToPoint
            } else if Self::bit(cp, 16 + shift) {
                ConnectionType::Null
            } else {
                return Err(unsupported(&format!("{name} is not point-to-point")));
            };
            let priority = if Self::bit(cp, 26 + shift) {
                Priority::Scheduled
            } else if Self::bit(cp, 25 + shift) {
                Priority::High
            } else {
                Priority::Low
            };
            let rpi_us = dir.rpi.and_then(|r| r.default.or(r.min)).unwrap_or(10_000);
            Ok(Direction {
                size: dir.size as u16,
                rpi: Duration::from_micros(rpi_us as u64),
                connection_type,
                priority,
                size_type: if Self::bit(cp, fixed_bit) {
                    SizeType::Fixed
                } else {
                    SizeType::Variable
                },
                run_idle_header: (cp >> (8 + shift)) & 0x7 == FORMAT_HEADER,
            })
        };

        let mut config = ConnectionConfig::new(
            self.path.clone(),
            direction(0, 0, &self.o2t, "O->T")?,
            direction(4, 2, &self.t2o, "T->O")?,
        );
        config.transport_class = transport_class;
        config.trigger = trigger;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the OpENer sample application's EDS.
    const SAMPLE: &str = r#"
[Device]
        VendCode = 1;
        VendName = "Rockwell Automation";
        ProdType = 12;
        ProdCode = 65001;
        MajRev = 2;
        MinRev = 3;
        ProdName = "OpENer PC";
        Catalog = "OpENer-2.x";

[Params]
        Param1 =
                0, ,, 0x0000, 0xD1, 1, "Input Data", "", "",
                ,,0, ,,,, ,,,, ;
        Param4 =
                0,                      $ reserved, shall equal 0
                ,,                      $ Link Path Size, Link Path
                0x0000,                 $ Descriptor
                0xC8,                   $ Data Type
                4,                      $ Data Size in bytes
                "RPI",                  $ name
                "",                     $ units
                "New Help String",      $ help string
                20000,,30000,           $ min, max, default data values
                ,,,,                    $ mult, div, base, offset scaling
                ,,,,                    $ mult, div, base, offset links
                ;                       $ decimal places
        Param9 =
                0, ,, 0x0000, 0xC6, 1, "Config instance", "", "",
                ,,0x97, ,,,, ,,,, ;

[Assembly]
        Assem100 = "Input Assembly", "", 32, 0x0000, ,, 8,Param1, 8,Param1;
        Assem150 = "Output Assembly", "", , 0x0000, ,, 128,Param1, 128,Param1;
        Assem151 = "Config Assembly", "", 10, 0x0000, ,, ;

[Connection Manager]
        Connection1 =
                0x84010002,
                0x44640405,
                Param4,,Assem150,       $ O->T RPI, size, format
                Param4,,Assem100,       $ T->O RPI, size, format
                ,,                      $ config #1 size, format
                ,Assem151,              $ config #2 size, format
                "Exlusive Owner",       $ Connection Name
                "",                     $ help string
                "20 04 24 97 2C 96 2C 64";    $ Path
        Connection2 =
                0x02010002, 0x44640305,
                5000,0,,                $ O->T: fixed 5 ms RPI, heartbeat
                Param4,,Assem100,
                ,, ,Assem151,
                "Input Only", "",
                "20 04 24 [Param9] 2C 98 2C 64";
"#;

    #[test]
    fn device() {
        let d = Eds::parse(SAMPLE).unwrap().device().unwrap();
        assert_eq!(
            (d.vendor_id, d.product_type, d.product_code),
            (1, 12, 65001)
        );
        assert_eq!((d.major_revision, d.minor_revision), (2, 3));
        assert_eq!(d.product_name, "OpENer PC");
        assert_eq!(d.catalog, "OpENer-2.x");
    }

    #[test]
    fn assemblies_and_params() {
        let eds = Eds::parse(SAMPLE).unwrap();
        assert_eq!(eds.assembly(100).unwrap().size, 32);
        assert_eq!(eds.assembly(150).unwrap().size, 32, "size from members");
        let rpi = eds.param(4).unwrap();
        assert_eq!(rpi.data_type, CipDataType::UDINT);
        assert_eq!(
            (rpi.min.as_deref(), rpi.max.as_deref(), rpi.default_int()),
            (Some("20000"), None, Some(30000))
        );
    }

    #[test]
    fn exclusive_owner_connection() {
        let eds = Eds::parse(SAMPLE).unwrap();
        let c = eds.connection(1).unwrap();
        assert_eq!(c.name, "Exlusive Owner");
        assert_eq!(c.path, ConnectionPath::assembly(151, 150, 100));
        assert_eq!((c.o2t.size, c.t2o.size), (32, 32));
        assert_eq!(c.config_assembly, Some(151));
        assert_eq!(
            c.o2t.rpi,
            Some(EdsRpi {
                min: Some(20000),
                max: None,
                default: Some(30000)
            })
        );

        let config = c.to_connection_config().unwrap();
        let rpi = Duration::from_millis(30);
        let expected = ConnectionConfig::new(
            ConnectionPath::assembly(151, 150, 100),
            Direction::new(32, rpi).with_run_idle_header(),
            Direction::new(32, rpi),
        );
        assert_eq!(config, expected);
    }

    #[test]
    fn param_reference_in_path_and_literal_rpi() {
        let eds = Eds::parse(SAMPLE).unwrap();
        let c = eds.connection(2).unwrap();
        assert_eq!(c.path.to_string(), "20 04 24 97 2C 98 2C 64");
        assert_eq!(c.o2t.size, 0);
        assert_eq!(c.o2t.rpi.unwrap().default, Some(5000));
        let config = c.to_connection_config().unwrap();
        assert_eq!(config.o2t.rpi, Duration::from_millis(5));
        assert!(
            !config.o2t.run_idle_header,
            "format 3 is not the run/idle header"
        );
        assert_eq!(eds.connections().unwrap().len(), 2);
    }

    #[test]
    fn missing_entries_are_errors() {
        let eds = Eds::parse(SAMPLE).unwrap();
        assert!(eds.connection(7).is_err());
        assert!(eds.assembly(1).is_err());
        let bad = Eds::parse(
            "[Connection Manager]\n Connection1 = 0x2, 0x5, ,,, ,,, ,,,, \"x\", \"\", \"20 ZZ\";",
        )
        .unwrap();
        let err = bad.connection(1).unwrap_err().to_string();
        assert!(err.contains("ZZ"), "{err}");
    }
}
