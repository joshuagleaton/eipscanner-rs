//! Connection Manager object (class 0x06) requests: Forward Open, Large Forward
//! Open, and Forward Close.

mod forward_close;
mod forward_open;
mod params;
mod path;

pub use forward_close::ForwardCloseRequest;
pub use forward_open::{ForwardOpenRequest, ForwardOpenResponse};
pub use params::{
    ConnectionConfig, ConnectionType, Direction, MAX_CONFIG_DATA, MAX_SMALL_SIZE, NetworkParams,
    Priority, SizeType, TransportClass, TransportTrigger, Trigger,
};
pub use path::{ConnectionPath, ElectronicKey};

use super::ServiceCode;

pub const CLASS_ID: u16 = 0x06;

impl ServiceCode {
    pub const FORWARD_CLOSE: Self = Self(0x4E);
    pub const FORWARD_OPEN: Self = Self(0x54);
    pub const LARGE_FORWARD_OPEN: Self = Self(0x5B);
}
