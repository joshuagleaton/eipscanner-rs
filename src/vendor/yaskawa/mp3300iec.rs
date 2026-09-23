//! Yaskawa MP3300iec controllers.
//!
//! These controllers reject 16-bit path segments. The C++ library has a
//! separate `Yaskawa_MessageRouter` and `Yaskawa_EPath` for this; here it is a
//! [`MessageRouter`] that encodes 8-bit segments.

use crate::MessageRouter;

/// Assembly object class ID.
pub const ASSEMBLY_OBJECT: u16 = 0x04;

pub fn message_router() -> MessageRouter {
    MessageRouter::with_8bit_path_segments()
}
