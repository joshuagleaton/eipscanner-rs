//! EtherNet/IP encapsulation: the 24-byte encapsulation header and the common
//! packet format (CPF) items carried inside it.

mod cpf;
mod encaps;
mod sockaddr;

pub use cpf::{
    CommonPacket, CommonPacketItem, CommonPacketItemId, ConnectedPacket, decode_connected_packet,
    encode_connected_packet,
};
pub use encaps::{EncapsCommand, EncapsPacket, EncapsStatus, HEADER_SIZE};
pub use sockaddr::{SOCKADDR_SIZE, decode_sockaddr, encode_sockaddr};
