//! Socket address items (O->T / T->O sockaddr info, ListIdentity). Unlike the
//! rest of EtherNet/IP these fields are big-endian, mirroring `struct sockaddr_in`.

use std::net::{Ipv4Addr, SocketAddrV4};

use bytes::{Buf, BufMut};

use crate::{Error, Result};

pub const SOCKADDR_SIZE: usize = 16;
const AF_INET: u16 = 2;

pub fn encode_sockaddr(addr: SocketAddrV4, out: &mut impl BufMut) {
    out.put_u16(AF_INET);
    out.put_u16(addr.port());
    out.put_slice(&addr.ip().octets());
    out.put_bytes(0, 8);
}

pub fn decode_sockaddr(buf: &mut impl Buf) -> Result<SocketAddrV4> {
    if buf.remaining() < SOCKADDR_SIZE {
        return Err(Error::Truncated {
            requested: SOCKADDR_SIZE,
            available: buf.remaining(),
        });
    }
    buf.get_u16(); // sin_family, not validated

    let port = buf.get_u16();
    let ip = Ipv4Addr::from(buf.get_u32());
    buf.advance(8);
    Ok(SocketAddrV4::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCALHOST_2222: [u8; 16] = [
        0x00, 0x02, 0x08, 0xae, 0x7f, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    #[test]
    fn encode() {
        let mut out = Vec::new();
        encode_sockaddr("127.0.0.1:2222".parse().unwrap(), &mut out);
        assert_eq!(out, LOCALHOST_2222);
    }

    #[test]
    fn decode() {
        let addr = decode_sockaddr(&mut &LOCALHOST_2222[..]).unwrap();
        assert_eq!(addr, "127.0.0.1:2222".parse().unwrap());
    }
}
