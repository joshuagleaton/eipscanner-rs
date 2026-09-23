use std::io::{Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::Duration;

use tracing::{debug, info, warn};

use crate::eip::{EncapsPacket, EncapsStatus, HEADER_SIZE};
use crate::{Error, Result};

/// An encapsulation session used for explicit messaging.
///
/// Implemented by [`SessionInfo`]; tests implement it to feed canned replies.
pub trait Session: Send + Sync {
    /// Sends a packet and waits for its reply.
    fn send_and_receive(&self, packet: &EncapsPacket) -> Result<EncapsPacket>;
    fn session_handle(&self) -> u32;
    fn remote_addr(&self) -> SocketAddrV4;
}

/// A registered encapsulation session over TCP. Unregisters on drop.
///
/// Requests are serialized by an internal lock, so one session can be shared
/// between threads (e.g. in an `Arc`).
#[derive(Debug)]
pub struct SessionInfo {
    stream: Mutex<TcpStream>,
    session_handle: u32,
    remote: SocketAddrV4,
}

impl SessionInfo {
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);

    /// Connects and registers a session, using [`Self::DEFAULT_TIMEOUT`].
    pub fn connect(host: &str, port: u16) -> Result<Self> {
        Self::connect_timeout(host, port, Self::DEFAULT_TIMEOUT)
    }

    /// Connects and registers a session. `timeout` applies to the TCP connect
    /// and to each read and write afterward.
    pub fn connect_timeout(host: &str, port: u16, timeout: Duration) -> Result<Self> {
        let remote = (host, port)
            .to_socket_addrs()?
            .find_map(|a| match a {
                SocketAddr::V4(v4) => Some(v4),
                SocketAddr::V6(_) => None,
            })
            .ok_or_else(|| Error::Unsupported(format!("{host} has no IPv4 address")))?;

        debug!(%remote, "connecting");
        let stream = TcpStream::connect_timeout(&remote.into(), timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.set_nodelay(true)?;

        let mut session = Self {
            stream: Mutex::new(stream),
            session_handle: 0,
            remote,
        };
        let reply = session.send_and_receive(&EncapsPacket::register_session())?;
        session.session_handle = reply.session_handle;
        info!(%remote, session_handle = session.session_handle, "registered session");
        Ok(session)
    }
}

impl Session for SessionInfo {
    fn send_and_receive(&self, packet: &EncapsPacket) -> Result<EncapsPacket> {
        // A poisoned lock only means another thread panicked mid-request; the
        // stream itself is still usable or will return an I/O error.
        let mut stream = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        stream.write_all(&packet.encode())?;

        let mut header = [0u8; HEADER_SIZE];
        stream.read_exact(&mut header)?;
        let mut data = vec![0u8; HEADER_SIZE + EncapsPacket::length_from_header(&header)];
        data[..HEADER_SIZE].copy_from_slice(&header);
        stream.read_exact(&mut data[HEADER_SIZE..])?;
        drop(stream);

        let reply = EncapsPacket::decode(&data)?;
        if reply.status != EncapsStatus::SUCCESS {
            return Err(Error::Encaps(reply.status));
        }
        if self.session_handle != 0 && reply.session_handle != self.session_handle {
            return Err(Error::WrongSessionHandle {
                expected: self.session_handle,
                received: reply.session_handle,
            });
        }
        Ok(reply)
    }

    fn session_handle(&self) -> u32 {
        self.session_handle
    }

    fn remote_addr(&self) -> SocketAddrV4 {
        self.remote
    }
}

impl Drop for SessionInfo {
    fn drop(&mut self) {
        let packet = EncapsPacket::unregister_session(self.session_handle).encode();
        let stream = self.stream.get_mut().unwrap_or_else(|e| e.into_inner());
        match stream.write_all(&packet) {
            Ok(()) => info!(session_handle = self.session_handle, "unregistered session"),
            Err(e) => {
                warn!(session_handle = self.session_handle, error = %e, "failed to unregister session")
            }
        }
    }
}
