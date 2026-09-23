//! A [`Session`] that answers message router requests from a queue.

use std::collections::VecDeque;
use std::net::SocketAddrV4;
use std::sync::Mutex;

use crate::cip::{EPath, GeneralStatusCode, ServiceCode};
use crate::eip::{CommonPacket, CommonPacketItem, EncapsPacket};
use crate::session::Session;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Request {
    pub service: ServiceCode,
    pub path: EPath,
    pub data: Vec<u8>,
}

type Reply = (GeneralStatusCode, Vec<u8>, Vec<CommonPacketItem>);

pub(crate) struct MockSession {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<Request>>,
    remote: SocketAddrV4,
}

impl Default for MockSession {
    fn default() -> Self {
        Self {
            replies: Mutex::default(),
            requests: Mutex::default(),
            remote: "127.0.0.1:44818".parse().unwrap(),
        }
    }
}

impl MockSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// The device address the session reports.
    pub fn with_remote(mut self, remote: SocketAddrV4) -> Self {
        self.remote = remote;
        self
    }

    pub fn reply(self, data: &[u8]) -> Self {
        self.reply_status(GeneralStatusCode::SUCCESS, data)
    }

    pub fn reply_status(self, status: GeneralStatusCode, data: &[u8]) -> Self {
        self.replies
            .lock()
            .unwrap()
            .push_back((status, data.to_vec(), Vec::new()));
        self
    }

    /// A successful reply with extra CPF items after the data item.
    pub fn reply_with_items(self, data: &[u8], items: Vec<CommonPacketItem>) -> Self {
        self.replies
            .lock()
            .unwrap()
            .push_back((GeneralStatusCode::SUCCESS, data.to_vec(), items));
        self
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Session for MockSession {
    fn send_and_receive(&self, packet: &EncapsPacket) -> Result<EncapsPacket> {
        let cpf = CommonPacket::decode(&packet.data[6..])?;
        let mr = &cpf.items[1].data;
        let path_len = mr[1] as usize * 2;
        self.requests.lock().unwrap().push(Request {
            service: ServiceCode(mr[0]),
            path: EPath::decode(&mr[2..2 + path_len])?,
            data: mr[2 + path_len..].to_vec(),
        });

        let (status, data, extra) = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| Error::Unsupported("mock session has no reply queued".into()))?;
        let mut response = vec![mr[0] | 0x80, 0, status.0, 0];
        response.extend(data);
        let mut items = vec![
            CommonPacketItem::null_address(),
            CommonPacketItem::unconnected_data(response),
        ];
        items.extend(extra);
        let cpf = CommonPacket::new(items);
        let mut reply_data = vec![0; 6];
        reply_data.extend(cpf.encode());
        Ok(EncapsPacket::new(
            packet.command,
            packet.session_handle,
            reply_data,
        ))
    }

    fn session_handle(&self) -> u32 {
        1
    }

    fn remote_addr(&self) -> SocketAddrV4 {
        self.remote
    }
}
