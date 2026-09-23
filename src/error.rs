use crate::cip::GeneralStatusCode;
use crate::eip::EncapsStatus;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("not enough data: needed {requested} bytes, {available} available")]
    Truncated { requested: usize, available: usize },

    #[error("invalid packet: {0}")]
    InvalidPacket(String),

    #[error("encapsulation error: {0}")]
    Encaps(EncapsStatus),

    #[error("wrong session handle: expected {expected:#010x}, received {received:#010x}")]
    WrongSessionHandle { expected: u32, received: u32 },

    #[error("{context}: CIP status {status}, additional status {additional:04x?}")]
    Cip {
        context: String,
        status: GeneralStatusCode,
        additional: Vec<u16>,
    },

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("EDS: {0}")]
    Eds(String),

    #[error("IO thread is not running")]
    IoThreadStopped,

    #[error("{0}")]
    Unsupported(String),
}

impl From<bytes::TryGetError> for Error {
    fn from(e: bytes::TryGetError) -> Self {
        Error::Truncated {
            requested: e.requested,
            available: e.available,
        }
    }
}
