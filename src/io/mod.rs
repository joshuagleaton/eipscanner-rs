//! Implicit messaging (class 0/1 IO connections).
//!
//! [`ConnectionManager`] opens connections with Forward Open on the calling
//! thread and hands them to an [`IoThread`], a dedicated OS thread that sends
//! O->T data on schedule, receives T->O data, and runs the callbacks in
//! [`IoCallbacks`]. The thread sleeps in `ppoll` until the next send or timeout
//! deadline, and can be given `SCHED_FIFO` priority, CPU affinity, and locked
//! memory through [`RealtimeConfig`].

mod connection;
mod manager;
mod rt;
mod thread;

pub use connection::{CloseHandler, InputData, IoCallbacks, ReceiveHandler, SendHandler};
pub use manager::{ConnectionManager, IoConnectionHandle};
pub use rt::RealtimeConfig;
pub use thread::{IoThread, IoThreadConfig};
