//! EtherNet/IP scanner: explicit messaging, implicit (class 1) IO connections, and discovery.
//!
//! Rust port of [EIPScanner](https://github.com/joshuagleaton/EIPScanner). Layers:
//!
//! - [`eip`], [`cip`]: packet encoding and decoding only, no sockets.
//! - [`SessionInfo`], [`MessageRouter`], [`DiscoveryManager`], [`objects`]: blocking
//!   explicit messaging over `std::net`.
//! - [`io`]: implicit messaging, run on a dedicated OS thread that can be given
//!   real-time scheduling.

#[macro_use]
mod code;

pub mod cip;
mod codec;
pub mod discovery;
pub mod eds;
pub mod eip;
mod error;
pub mod io;
pub mod message_router;
mod netif;
pub mod objects;
pub mod session;
#[cfg(test)]
mod test_support;
#[cfg(feature = "vendor")]
pub mod vendor;

pub use discovery::{BroadcastTarget, DiscoveryManager, IdentityItem};
pub use error::{Error, Result};
pub use message_router::MessageRouter;
pub use session::{Session, SessionInfo};

/// TCP port for explicit messaging.
pub const EIP_DEFAULT_EXPLICIT_PORT: u16 = 0xAF12;
/// UDP port for implicit messaging.
pub const EIP_DEFAULT_IMPLICIT_PORT: u16 = 0x08AE;
