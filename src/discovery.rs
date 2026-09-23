use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use bytes::Buf;
use tracing::{debug, warn};

use crate::cip::CipRevision;
use crate::codec::BufExt;
use crate::eip::{CommonPacket, CommonPacketItemId, EncapsPacket, HEADER_SIZE, decode_sockaddr};
use crate::objects::IdentityObject;
use crate::{EIP_DEFAULT_EXPLICIT_PORT, Result};

/// A device that answered ListIdentity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityItem {
    pub identity: IdentityObject,
    /// The device's encapsulation (TCP) address, as it reports it.
    pub socket_address: SocketAddrV4,
}

/// Where ListIdentity is broadcast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BroadcastTarget {
    /// The broadcast address of every IPv4 interface that is up. Reaches
    /// devices on each attached network, including Docker bridges.
    AllInterfaces,
    /// One address, e.g. a subnet's directed broadcast address. Note that
    /// 255.255.255.255 only leaves through the interface with the default route.
    Address(SocketAddrV4),
}

/// Finds devices by broadcasting ListIdentity over UDP.
#[derive(Debug, Clone)]
pub struct DiscoveryManager {
    target: BroadcastTarget,
    timeout: Duration,
}

impl DiscoveryManager {
    pub fn new(target: BroadcastTarget, timeout: Duration) -> Self {
        Self { target, timeout }
    }

    /// Broadcast on all interfaces. Devices may delay their reply by up to 2 s,
    /// so `timeout` should be longer than that.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self::new(BroadcastTarget::AllInterfaces, timeout)
    }

    /// Sends ListIdentity and collects replies until `timeout` has passed.
    pub fn discover(&self) -> Result<Vec<IdentityItem>> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;
        let targets = match &self.target {
            BroadcastTarget::Address(addr) => vec![*addr],
            BroadcastTarget::AllInterfaces => crate::netif::ipv4_broadcast_addrs()?
                .into_iter()
                .map(|ip| SocketAddrV4::new(ip, EIP_DEFAULT_EXPLICIT_PORT))
                .collect(),
        };
        let request = EncapsPacket::list_identity().encode();
        for target in &targets {
            debug!(%target, "sending ListIdentity");
            if let Err(e) = socket.send_to(&request, target) {
                warn!(%target, error = %e, "failed to send ListIdentity");
            }
        }

        let deadline = Instant::now() + self.timeout;
        let mut devices = Vec::new();
        let mut buf = [0u8; 1500];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            socket.set_read_timeout(Some(remaining))?;
            let (len, from) = match socket.recv_from(&mut buf) {
                Ok(r) => r,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(e) => return Err(e.into()),
            };
            match parse_list_identity_response(&buf[..len]) {
                Ok(found) => devices.extend(found),
                Err(e) => warn!(%from, error = %e, "ignoring malformed ListIdentity reply"),
            }
        }
        debug!(count = devices.len(), "discovery finished");
        Ok(devices)
    }
}

/// Parses a ListIdentity reply: encapsulation header, then one identity item
/// per CPF item.
pub fn parse_list_identity_response(data: &[u8]) -> Result<Vec<IdentityItem>> {
    let mut buf = data;
    buf.try_skip(HEADER_SIZE)?;
    let packet = CommonPacket::decode(buf)?;
    packet
        .items
        .iter()
        .filter(|item| item.type_id == CommonPacketItemId::LIST_IDENTITY)
        .map(|item| {
            let mut buf = &item.data[..];
            buf.try_get_u16_le()?; // encapsulation protocol version
            let socket_address = decode_sockaddr(&mut buf)?;
            let identity = IdentityObject {
                instance_id: 1,
                vendor_id: buf.try_get_u16_le()?,
                device_type: buf.try_get_u16_le()?,
                product_code: buf.try_get_u16_le()?,
                revision: CipRevision::decode(&mut buf)?,
                status: buf.try_get_u16_le()?,
                serial_number: buf.try_get_u32_le()?,
                product_name: buf.try_get_short_string()?,
            };
            Ok(IdentityItem {
                identity,
                socket_address,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rustfmt::skip]
    const LIST_IDENTITY_RESPONSE: &[u8] = &[
        0x63, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x02, 0x00,
        0x0c, 0x00, 0x42, 0x00, 0x01, 0x00, 0x00, 0x02, 0xaf, 0x12, 0xc0, 0xa8,
        0x01, 0x0f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x96, 0x00, 0x09, 0x00,
        0x05, 0x01, 0x30, 0x00, 0x92, 0xe1, 0x8d, 0x80, 0x1d, 0x50, 0x6f, 0x77, 0x65, 0x72, 0x46, 0x6c,
        0x65, 0x78, 0x20, 0x35, 0x32, 0x35, 0x20, 0x31, 0x50, 0x20, 0x31, 0x31, 0x30, 0x56, 0x20, 0x20,
        0x20, 0x2e, 0x35, 0x30, 0x48, 0x50, 0x00, 0x00, 0x00, 0xff,
        0x0c, 0x00, 0x42, 0x00, 0x01, 0x00, 0x00, 0x02, 0xaf, 0x12, 0xc0, 0xa8,
        0x01, 0x0f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x96, 0x00, 0x09, 0x00,
        0x05, 0x01, 0x30, 0x00, 0x92, 0xe1, 0x8d, 0x80, 0x1d, 0x50, 0x6f, 0x77, 0x65, 0x72, 0x46, 0x6c,
        0x65, 0x78, 0x20, 0x35, 0x32, 0x35, 0x20, 0x31, 0x50, 0x20, 0x31, 0x31, 0x30, 0x56, 0x20, 0x20,
        0x20, 0x2e, 0x35, 0x30, 0x48, 0x50, 0x00, 0x00, 0x00, 0xff,
    ];

    #[test]
    fn parse_powerflex_reply() {
        let devices = parse_list_identity_response(LIST_IDENTITY_RESPONSE).unwrap();
        assert_eq!(devices.len(), 2);
        let d = &devices[1];
        assert_eq!(d.socket_address.to_string(), "192.168.1.15:44818");
        assert_eq!(d.identity.vendor_id, 1);
        assert_eq!(d.identity.device_type, 0x96);
        assert_eq!(d.identity.product_code, 9);
        assert_eq!(d.identity.revision, CipRevision::new(5, 1));
        assert_eq!(d.identity.status, 0x30);
        assert_eq!(d.identity.serial_number, 0x808de192);
        assert_eq!(d.identity.product_name, "PowerFlex 525 1P 110V   .50HP");
    }

    #[test]
    fn truncated_reply_is_an_error() {
        assert!(parse_list_identity_response(&LIST_IDENTITY_RESPONSE[..60]).is_err());
    }
}
