//! CIP (Common Industrial Protocol) encoding: paths, services, status codes, and
//! message router requests and responses.

pub mod connection_manager;
mod epath;
mod message_router;
mod revision;
mod types;

pub use epath::{EPath, SegmentSize};
pub use message_router::{MessageRouterResponse, encode_request};
pub use revision::CipRevision;
pub use types::{CipDataType, GeneralStatusCode, ServiceCode};
