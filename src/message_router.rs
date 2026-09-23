use bytes::Buf;
use tracing::debug;

use crate::cip::{EPath, MessageRouterResponse, SegmentSize, ServiceCode, encode_request};
use crate::codec::BufExt;
use crate::eip::{CommonPacket, CommonPacketItem, EncapsPacket};
use crate::session::Session;
use crate::{Error, Result};

/// Sends unconnected explicit requests (SendRRData) to a device's message router.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MessageRouter {
    segment_size: SegmentSize,
}

impl MessageRouter {
    /// Encodes paths with 16-bit segments.
    pub fn new() -> Self {
        Self::default()
    }

    /// Encodes paths with 8-bit segments where the IDs fit.
    pub fn with_8bit_path_segments() -> Self {
        Self {
            segment_size: SegmentSize::Bits8,
        }
    }

    pub fn segment_size(&self) -> SegmentSize {
        self.segment_size
    }

    pub fn send_request(
        &self,
        session: &dyn Session,
        service: ServiceCode,
        path: &EPath,
        data: &[u8],
    ) -> Result<MessageRouterResponse> {
        self.send_request_with_items(session, service, path, data, &[])
    }

    /// Sends a request with extra common packet items after the unconnected data
    /// item. Returns the response whatever its general status; see
    /// [`MessageRouterResponse::check`].
    pub fn send_request_with_items(
        &self,
        session: &dyn Session,
        service: ServiceCode,
        path: &EPath,
        data: &[u8],
        additional_items: &[CommonPacketItem],
    ) -> Result<MessageRouterResponse> {
        debug!(%service, %path, "send request");
        let request = encode_request(service, path, data, self.segment_size);
        let mut items = Vec::with_capacity(2 + additional_items.len());
        items.push(CommonPacketItem::null_address());
        items.push(CommonPacketItem::unconnected_data(request));
        items.extend_from_slice(additional_items);

        let packet = EncapsPacket::send_rr_data(
            session.session_handle(),
            0,
            &CommonPacket::new(items).encode(),
        );
        let reply = session.send_and_receive(&packet)?;

        let mut buf = &reply.data[..];
        buf.try_skip(6)?; // interface handle, timeout
        let mut items = CommonPacket::decode(buf.chunk())?.items.into_iter();
        let data_item = items
            .nth(1)
            .ok_or_else(|| Error::InvalidPacket("SendRRData reply has no data item".into()))?;
        let mut response = MessageRouterResponse::decode(&data_item.data)?;
        response.additional_packet_items = items.collect();
        Ok(response)
    }
}
