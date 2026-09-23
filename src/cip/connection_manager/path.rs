use std::fmt;
use std::str::FromStr;

use bytes::BufMut;

use crate::{Error, Result};

const CLASS_8: u8 = 0x20;
const INSTANCE_8: u8 = 0x24;
const CONNECTION_POINT_8: u8 = 0x2C;
const ELECTRONIC_KEY: u8 = 0x34;
const KEY_FORMAT: u8 = 0x04;
const ASSEMBLY_CLASS: u16 = 0x04;

/// Vendor, product, and revision the target must match for the connection to
/// be accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ElectronicKey {
    pub vendor_id: u16,
    pub device_type: u16,
    pub product_code: u16,
    pub major_revision: u8,
    pub minor_revision: u8,
    /// Accept any device that claims to be compatible with this revision,
    /// rather than requiring an exact match.
    pub compatibility: bool,
}

/// Builds the connection path of a Forward Open.
///
/// ```
/// use eipscanner::cip::connection_manager::ConnectionPath;
///
/// // Configuration assembly 151, output 150, input 100.
/// let path = ConnectionPath::assembly(151, 150, 100);
/// assert_eq!(path.to_string(), "20 04 24 97 2C 96 2C 64");
///
/// // The same, for a module in slot 3 of a chassis behind the adapter.
/// let path = ConnectionPath::new().backplane_slot(3).then(ConnectionPath::assembly(151, 150, 100));
/// assert_eq!(path.to_string(), "01 03 20 04 24 97 2C 96 2C 64");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ConnectionPath(Vec<u8>);

impl ConnectionPath {
    pub fn new() -> Self {
        Self::default()
    }

    /// The usual I/O connection path: the Assembly class, the configuration
    /// instance, then the output (O->T) and input (T->O) connection points.
    pub fn assembly(config: u16, output: u16, input: u16) -> Self {
        Self::new()
            .class(ASSEMBLY_CLASS)
            .instance(config)
            .connection_point(output)
            .connection_point(input)
    }

    /// Route through a port. The link address is a slot number for a
    /// backplane, or e.g. an IP address as ASCII for an Ethernet port.
    pub fn port(mut self, port: u16, link_address: &[u8]) -> Self {
        let extended_port = port >= 15;
        let extended_link = link_address.len() != 1;
        let mut segment = if extended_port { 0x0F } else { port as u8 };
        if extended_link {
            segment |= 0x10;
        }
        let start = self.0.len();
        self.0.put_u8(segment);
        if extended_link {
            self.0.put_u8(link_address.len() as u8);
        }
        if extended_port {
            self.0.put_u16_le(port);
        }
        self.0.put_slice(link_address);
        if !(self.0.len() - start).is_multiple_of(2) {
            self.0.put_u8(0);
        }
        self
    }

    /// Route to a slot through backplane port 1.
    pub fn backplane_slot(self, slot: u8) -> Self {
        self.port(1, &[slot])
    }

    /// Parses a route in the comma-separated `port,link,port,link,...` form
    /// used by libplctag and Logix tools. A link is a slot or node number, or
    /// an IPv4 address for an Ethernet port.
    ///
    /// ```
    /// use eipscanner::cip::connection_manager::ConnectionPath;
    ///
    /// // Backplane slot 0, then out Ethernet port 2 to 10.0.0.5, then slot 3.
    /// let route = ConnectionPath::route("1,0,2,10.0.0.5,1,3")?;
    /// assert_eq!(route.to_string(), "01 00 12 08 31 30 2E 30 2E 30 2E 35 01 03");
    /// # Ok::<(), eipscanner::Error>(())
    /// ```
    pub fn route(spec: &str) -> Result<Self> {
        let tokens: Vec<&str> = spec
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        if !tokens.len().is_multiple_of(2) {
            return Err(Error::InvalidInput(format!(
                "route '{spec}' must be port,link pairs"
            )));
        }
        let mut path = Self::new();
        for pair in tokens.chunks(2) {
            let port: u16 = pair[0].parse().ok().filter(|&p| p != 0).ok_or_else(|| {
                Error::InvalidInput(format!("'{}' is not a port number", pair[0]))
            })?;
            path = if let Ok(link) = pair[1].parse::<u8>() {
                path.port(port, &[link])
            } else if pair[1].parse::<std::net::Ipv4Addr>().is_ok() {
                path.port(port, pair[1].as_bytes())
            } else {
                return Err(Error::InvalidInput(format!(
                    "'{}' is not a slot number or IPv4 address",
                    pair[1]
                )));
            };
        }
        Ok(path)
    }

    pub fn electronic_key(mut self, key: ElectronicKey) -> Self {
        self.0.put_u8(ELECTRONIC_KEY);
        self.0.put_u8(KEY_FORMAT);
        self.0.put_u16_le(key.vendor_id);
        self.0.put_u16_le(key.device_type);
        self.0.put_u16_le(key.product_code);
        self.0
            .put_u8((key.major_revision & 0x7F) | if key.compatibility { 0x80 } else { 0 });
        self.0.put_u8(key.minor_revision);
        self
    }

    pub fn class(self, id: u16) -> Self {
        self.logical(CLASS_8, id)
    }

    pub fn instance(self, id: u16) -> Self {
        self.logical(INSTANCE_8, id)
    }

    pub fn connection_point(self, id: u16) -> Self {
        self.logical(CONNECTION_POINT_8, id)
    }

    /// Appends another path.
    pub fn then(mut self, other: ConnectionPath) -> Self {
        self.0.extend(other.0);
        self
    }

    /// 8-bit segment when the value fits, otherwise a padded 16-bit one.
    fn logical(mut self, segment_8: u8, id: u16) -> Self {
        if id <= u8::MAX as u16 {
            self.0.put_u8(segment_8);
            self.0.put_u8(id as u8);
        } else {
            self.0.put_u8(segment_8 | 0x01);
            self.0.put_u8(0);
            self.0.put_u16_le(id);
        }
        self
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// Raw path bytes, e.g. from a device's documentation.
impl From<Vec<u8>> for ConnectionPath {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl From<ConnectionPath> for Vec<u8> {
    fn from(path: ConnectionPath) -> Self {
        path.0
    }
}

/// Space-separated hex bytes, the notation EDS files use.
impl fmt::Display for ConnectionPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{b:02X}")?;
        }
        Ok(())
    }
}

/// Parses space-separated hex bytes, e.g. `"20 04 24 97 2C 96 2C 64"`.
impl FromStr for ConnectionPath {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        s.split_whitespace()
            .map(|t| {
                u8::from_str_radix(t, 16).map_err(|_| {
                    Error::InvalidInput(format!("'{t}' is not a hex byte in path '{s}'"))
                })
            })
            .collect::<Result<Vec<u8>>>()
            .map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembly_path() {
        assert_eq!(
            ConnectionPath::assembly(151, 150, 100).as_bytes(),
            [0x20, 0x04, 0x24, 0x97, 0x2C, 0x96, 0x2C, 0x64]
        );
    }

    #[test]
    fn large_ids_use_16_bit_segments() {
        assert_eq!(
            ConnectionPath::new()
                .class(4)
                .instance(0x0300)
                .connection_point(0x0102)
                .as_bytes(),
            [0x20, 0x04, 0x25, 0x00, 0x00, 0x03, 0x2D, 0x00, 0x02, 0x01]
        );
    }

    #[test]
    fn port_segments() {
        assert_eq!(
            ConnectionPath::new().backplane_slot(3).as_bytes(),
            [0x01, 0x03]
        );
        // Ethernet port 2 to 10.0.0.1: extended link address, padded to even length.
        assert_eq!(
            ConnectionPath::new().port(2, b"10.0.0.1").as_bytes(),
            [0x12, 0x08, b'1', b'0', b'.', b'0', b'.', b'0', b'.', b'1']
        );
        assert_eq!(
            ConnectionPath::new().port(2, b"10.0.0.10").as_bytes(),
            [
                0x12, 0x09, b'1', b'0', b'.', b'0', b'.', b'0', b'.', b'1', b'0', 0
            ]
        );
        // Port 20: extended port ID, then the link address, then a pad byte.
        assert_eq!(
            ConnectionPath::new().port(20, &[5]).as_bytes(),
            [0x0F, 20, 0, 5]
        );
    }

    #[test]
    fn electronic_key() {
        let key = ElectronicKey {
            vendor_id: 1,
            device_type: 12,
            product_code: 65001,
            major_revision: 2,
            minor_revision: 3,
            compatibility: true,
        };
        assert_eq!(
            ConnectionPath::new().electronic_key(key).as_bytes(),
            [0x34, 0x04, 1, 0, 12, 0, 0xE9, 0xFD, 0x82, 3]
        );
    }

    #[test]
    fn hex_round_trip() {
        let path: ConnectionPath = "20 04 24 97 2C 96 2C 64".parse().unwrap();
        assert_eq!(path, ConnectionPath::assembly(151, 150, 100));
        assert_eq!(path.to_string(), "20 04 24 97 2C 96 2C 64");
        assert!("20 xx".parse::<ConnectionPath>().is_err());
    }

    #[test]
    fn route_strings() {
        assert_eq!(
            ConnectionPath::route("1,0").unwrap().as_bytes(),
            [0x01, 0x00]
        );
        assert_eq!(
            ConnectionPath::route(" 1, 0, 1 ,3 ").unwrap().as_bytes(),
            [0x01, 0x00, 0x01, 0x03]
        );
        assert_eq!(
            ConnectionPath::route("2,10.0.0.1").unwrap(),
            ConnectionPath::new().port(2, b"10.0.0.1")
        );
        assert_eq!(
            ConnectionPath::route("20,5").unwrap().as_bytes(),
            [0x0F, 20, 0, 5]
        );
        assert_eq!(ConnectionPath::route("").unwrap(), ConnectionPath::new());
        for bad in ["1", "0,1", "x,1", "1,256", "1,host"] {
            assert!(
                matches!(ConnectionPath::route(bad), Err(Error::InvalidInput(_))),
                "{bad}"
            );
        }
    }
}
