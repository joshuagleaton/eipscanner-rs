use std::time::Duration;

use super::ConnectionPath;

/// How a connection's packets are addressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionType {
    /// No data in this direction (e.g. the O->T side of a listen-only connection).
    Null,
    Multicast,
    #[default]
    PointToPoint,
    Reserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    Low,
    High,
    #[default]
    Scheduled,
    Urgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeType {
    #[default]
    Fixed,
    Variable,
}

/// Network connection parameters as they appear in a Forward Open. `size` is
/// the size on the wire, including the sequence count and run/idle header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetworkParams {
    pub redundant_owner: bool,
    pub connection_type: ConnectionType,
    pub priority: Priority,
    pub size_type: SizeType,
    pub size: u16,
}

/// Largest size the 16-bit (non-large) Forward Open can carry.
pub const MAX_SMALL_SIZE: u16 = 0x1FF;

impl NetworkParams {
    /// Encodes the 16-bit form (`large = false`) or the 32-bit form used by
    /// Large Forward Open, which moves the flags up by 16 bits.
    pub fn encode(&self, large: bool) -> u32 {
        let shift = if large { 16 } else { 0 };
        let size = if large {
            self.size as u32
        } else {
            (self.size & MAX_SMALL_SIZE) as u32
        };
        let flags = (u32::from(self.redundant_owner) << 15)
            | ((self.connection_type as u32) << 13)
            | ((self.priority as u32) << 10)
            | ((self.size_type as u32) << 9);
        (flags << shift) | size
    }

    pub fn decode(raw: u32, large: bool) -> Self {
        let (flags, size) = if large {
            (raw >> 16, raw & 0xFFFF)
        } else {
            (raw, raw & MAX_SMALL_SIZE as u32)
        };
        Self {
            redundant_owner: flags & (1 << 15) != 0,
            connection_type: match (flags >> 13) & 3 {
                0 => ConnectionType::Null,
                1 => ConnectionType::Multicast,
                2 => ConnectionType::PointToPoint,
                _ => ConnectionType::Reserved,
            },
            priority: match (flags >> 10) & 3 {
                0 => Priority::Low,
                1 => Priority::High,
                2 => Priority::Scheduled,
                _ => Priority::Urgent,
            },
            size_type: if flags & (1 << 9) != 0 {
                SizeType::Variable
            } else {
                SizeType::Fixed
            },
            size: size as u16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportClass {
    Class0,
    #[default]
    Class1,
    Class2,
    Class3,
}

impl TransportClass {
    /// Classes 1-3 prefix data with a 16-bit sequence count.
    pub fn has_sequence_count(self) -> bool {
        self != TransportClass::Class0
    }
}

/// What makes the target produce data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Trigger {
    #[default]
    Cyclic,
    ChangeOfState,
    Application,
}

/// The transport class/trigger byte of a Forward Open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransportTrigger {
    pub class: TransportClass,
    pub trigger: Trigger,
    /// Direction bit: set when the target is the server of a class 2/3
    /// connection. Clear for I/O connections.
    pub server: bool,
}

impl TransportTrigger {
    pub fn encode(&self) -> u8 {
        (u8::from(self.server) << 7) | ((self.trigger as u8) << 4) | self.class as u8
    }

    pub fn decode(raw: u8) -> Self {
        Self {
            class: match raw & 0x0F {
                0 => TransportClass::Class0,
                1 => TransportClass::Class1,
                2 => TransportClass::Class2,
                _ => TransportClass::Class3,
            },
            trigger: match (raw >> 4) & 0x07 {
                1 => Trigger::ChangeOfState,
                2 => Trigger::Application,
                _ => Trigger::Cyclic,
            },
            server: raw & 0x80 != 0,
        }
    }
}

/// One direction of an I/O connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Direction {
    /// Application data size in bytes, without sequence count or run/idle
    /// header; 0 for a heartbeat.
    pub size: u16,
    /// Requested packet interval.
    pub rpi: Duration,
    pub connection_type: ConnectionType,
    pub priority: Priority,
    pub size_type: SizeType,
    /// Data is preceded by a 32-bit run/idle header.
    pub run_idle_header: bool,
}

impl Direction {
    /// Point-to-point, scheduled priority, fixed size, no run/idle header.
    pub fn new(size: u16, rpi: Duration) -> Self {
        Self {
            size,
            rpi,
            connection_type: ConnectionType::PointToPoint,
            priority: Priority::Scheduled,
            size_type: SizeType::Fixed,
            run_idle_header: false,
        }
    }

    pub fn with_run_idle_header(mut self) -> Self {
        self.run_idle_header = true;
        self
    }
}

/// Largest configuration data a simple data segment can carry (255 words).
pub const MAX_CONFIG_DATA: usize = 510;

/// An I/O connection to open with
/// [`ConnectionManager::forward_open`](crate::io::ConnectionManager::forward_open).
///
/// ```
/// use std::time::Duration;
/// use eipscanner::cip::connection_manager::{ConnectionConfig, ConnectionPath, Direction};
///
/// let rpi = Duration::from_millis(10);
/// let config = ConnectionConfig::new(
///     ConnectionPath::assembly(151, 150, 100),
///     Direction::new(32, rpi).with_run_idle_header(), // outputs
///     Direction::new(32, rpi),                        // inputs
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    pub path: ConnectionPath,
    /// Data for the configuration assembly named in the path, sent with the
    /// Forward Open. Empty for none; at most [`MAX_CONFIG_DATA`] bytes.
    pub config_data: Vec<u8>,
    /// Outputs, originator to target.
    pub o2t: Direction,
    /// Inputs, target to originator.
    pub t2o: Direction,
    pub transport_class: TransportClass,
    pub trigger: Trigger,
    /// The connection times out after `4 << timeout_multiplier` RPIs without
    /// data. At most 7.
    pub timeout_multiplier: u8,
    /// Identifies this scanner to the target, together with the serial number.
    pub originator_vendor_id: u16,
    pub originator_serial_number: u32,
}

impl ConnectionConfig {
    /// Class 1, cyclic, timeout multiplier 0 (4 RPIs), originator IDs 0.
    pub fn new(path: ConnectionPath, o2t: Direction, t2o: Direction) -> Self {
        Self {
            path,
            config_data: Vec::new(),
            o2t,
            t2o,
            transport_class: TransportClass::Class1,
            trigger: Trigger::Cyclic,
            timeout_multiplier: 0,
            originator_vendor_id: 0,
            originator_serial_number: 0,
        }
    }

    /// Bytes on the wire for one direction: sequence count, run/idle header, data.
    pub fn wire_size(&self, dir: &Direction) -> usize {
        let seq = if self.transport_class.has_sequence_count() {
            2
        } else {
            0
        };
        let header = if dir.run_idle_header { 4 } else { 0 };
        seq + header + dir.size as usize
    }

    /// The connection path as sent in a Forward Open: `path`, then
    /// `config_data` as a simple data segment (truncated to [`MAX_CONFIG_DATA`]).
    pub fn encoded_path(&self) -> Vec<u8> {
        let mut out = self.path.as_bytes().to_vec();
        if !self.config_data.is_empty() {
            let data = &self.config_data[..self.config_data.len().min(MAX_CONFIG_DATA)];
            let words = data.len().div_ceil(2);
            out.push(0x80);
            out.push(words as u8);
            out.extend_from_slice(data);
            out.resize(out.len() + words * 2 - data.len(), 0);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_params_small() {
        let p = NetworkParams {
            connection_type: ConnectionType::PointToPoint,
            priority: Priority::Scheduled,
            size_type: SizeType::Variable,
            size: 32,
            ..Default::default()
        };
        assert_eq!(p.encode(false), (2 << 13) | (2 << 10) | (1 << 9) | 32);
        assert_eq!(NetworkParams::decode(p.encode(false), false), p);
    }

    #[test]
    fn network_params_large() {
        let p = NetworkParams {
            redundant_owner: true,
            connection_type: ConnectionType::Multicast,
            priority: Priority::Urgent,
            size_type: SizeType::Fixed,
            size: 1000,
        };
        assert_eq!(p.encode(true), (1 << 31) | (1 << 29) | (3 << 26) | 1000);
        assert_eq!(NetworkParams::decode(p.encode(true), true), p);
    }

    #[test]
    fn transport_trigger() {
        let t = TransportTrigger {
            class: TransportClass::Class1,
            trigger: Trigger::ChangeOfState,
            server: false,
        };
        assert_eq!(t.encode(), 0x11);
        assert_eq!(TransportTrigger::decode(0x11), t);
        assert_eq!(TransportTrigger::decode(0x83).class, TransportClass::Class3);
        assert!(TransportTrigger::decode(0x83).server);
    }

    #[test]
    fn wire_sizes_and_config_segment() {
        let rpi = Duration::from_millis(10);
        let mut c = ConnectionConfig::new(
            ConnectionPath::assembly(151, 150, 100),
            Direction::new(32, rpi).with_run_idle_header(),
            Direction::new(32, rpi),
        );
        assert_eq!((c.wire_size(&c.o2t), c.wire_size(&c.t2o)), (38, 34));
        assert_eq!(c.encoded_path(), c.path.as_bytes());

        c.config_data = vec![1, 2, 3];
        assert_eq!(
            c.encoded_path(),
            [
                0x20, 0x04, 0x24, 0x97, 0x2C, 0x96, 0x2C, 0x64, 0x80, 2, 1, 2, 3, 0
            ]
        );

        c.transport_class = TransportClass::Class0;
        assert_eq!(c.wire_size(&c.o2t), 36);
    }
}
