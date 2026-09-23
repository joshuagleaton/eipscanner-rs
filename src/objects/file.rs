//! File object (class 0x37). Upload (device to scanner) only.

use bytes::Buf;
use tracing::{debug, error, info, warn};

use crate::cip::{EPath, GeneralStatusCode, ServiceCode};
use crate::codec::BufExt;
use crate::session::Session;
use crate::{Error, MessageRouter, Result};

pub const CLASS_ID: u16 = 0x37;
pub const MAX_TRANSFER_SIZE: u8 = 255;
const STATE_ATTRIBUTE: u16 = 1;

impl ServiceCode {
    pub const INITIATE_UPLOAD: Self = Self(0x4B);
    pub const UPLOAD_TRANSFER: Self = Self(0x4F);
}

wire_code! {
    FileObjectStateCode(u8) {
        NONEXISTENT = 0,
        FILE_EMPTY = 1,
        FILE_LOADED = 2,
        TRANSFER_UPLOAD_INITIATED = 3,
        TRANSFER_DOWNLOAD_INITIATED = 4,
        TRANSFER_UPLOAD_IN_PROGRESS = 5,
        TRANSFER_DOWNLOAD_IN_PROGRESS = 6,
        UNKNOWN = 255,
    }
}

wire_code! {
    TransferPacketType(u8) {
        FIRST = 0,
        MIDDLE = 1,
        LAST = 2,
        ABORT = 3,
        FIRST_AND_LAST = 4,
    }
}

/// Called once when an upload ends, with the status and (on success) the file.
pub type EndUploadHandler = Box<dyn FnOnce(GeneralStatusCode, Vec<u8>) + Send>;

enum State {
    NonExistent,
    Empty,
    Loaded,
    UploadInProgress {
        file_size: u32,
        transfer_size: u8,
        transfer_number: u8,
        content: Vec<u8>,
        handler: EndUploadHandler,
    },
}

impl State {
    fn code(&self) -> FileObjectStateCode {
        match self {
            State::NonExistent => FileObjectStateCode::NONEXISTENT,
            State::Empty => FileObjectStateCode::FILE_EMPTY,
            State::Loaded => FileObjectStateCode::FILE_LOADED,
            State::UploadInProgress { .. } => FileObjectStateCode::TRANSFER_UPLOAD_IN_PROGRESS,
        }
    }
}

pub struct FileObject {
    instance_id: u16,
    router: MessageRouter,
    state: State,
}

impl std::fmt::Debug for FileObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileObject")
            .field("instance_id", &self.instance_id)
            .field("state", &self.state.code())
            .finish()
    }
}

impl FileObject {
    /// Reads the object's state from the device. A device that is mid-upload is
    /// treated as loaded, so the upload is started over.
    pub fn new(session: &dyn Session, router: MessageRouter, instance_id: u16) -> Result<Self> {
        let reply = router
            .send_request(
                session,
                ServiceCode::GET_ATTRIBUTE_SINGLE,
                &EPath::attribute(CLASS_ID, instance_id, STATE_ATTRIBUTE),
                &[],
            )?
            .check(format!("read state of file object {instance_id}"))?;
        let code = FileObjectStateCode((&reply.data[..]).try_get_u8()?);
        let state = match code {
            FileObjectStateCode::NONEXISTENT => State::NonExistent,
            FileObjectStateCode::FILE_EMPTY => State::Empty,
            FileObjectStateCode::FILE_LOADED => State::Loaded,
            FileObjectStateCode::TRANSFER_UPLOAD_INITIATED
            | FileObjectStateCode::TRANSFER_UPLOAD_IN_PROGRESS => {
                warn!(
                    instance_id,
                    "file is uploading; the upload must be started again"
                );
                State::Loaded
            }
            other => {
                return Err(Error::Unsupported(format!(
                    "file object state {other} is not supported"
                )));
            }
        };
        debug!(instance_id, state = %state.code(), "file object state");
        Ok(Self {
            instance_id,
            router,
            state,
        })
    }

    pub fn instance_id(&self) -> u16 {
        self.instance_id
    }

    pub fn state(&self) -> FileObjectStateCode {
        self.state.code()
    }

    /// Starts an upload. Call [`Self::handle_transfers`] until it returns false;
    /// `handler` is called when the upload ends. Does nothing unless a file is
    /// loaded.
    pub fn begin_upload(
        &mut self,
        session: &dyn Session,
        handler: impl FnOnce(GeneralStatusCode, Vec<u8>) + Send + 'static,
    ) -> Result<()> {
        if !matches!(self.state, State::Loaded) {
            warn!(instance_id = self.instance_id, state = %self.state(), "file cannot be uploaded");
            return Ok(());
        }
        info!(instance_id = self.instance_id, "initiate upload");
        let reply = self.router.send_request(
            session,
            ServiceCode::INITIATE_UPLOAD,
            &EPath::instance(CLASS_ID, self.instance_id),
            &[MAX_TRANSFER_SIZE],
        )?;
        if !reply.is_success() {
            error!(instance_id = self.instance_id, status = %reply.general_status, "initiate upload failed");
            handler(reply.general_status, Vec::new());
            return Ok(());
        }
        let mut buf = &reply.data[..];
        let file_size = buf.try_get_u32_le()?;
        let transfer_size = buf.try_get_u8()?;
        self.state = State::UploadInProgress {
            file_size,
            transfer_size,
            transfer_number: 0,
            content: Vec::with_capacity(file_size as usize),
            handler: Box::new(handler),
        };
        Ok(())
    }

    /// Requests the next upload packet. Returns true while more packets remain.
    pub fn handle_transfers(&mut self, session: &dyn Session) -> Result<bool> {
        let State::UploadInProgress {
            transfer_number, ..
        } = &self.state
        else {
            warn!(instance_id = self.instance_id, state = %self.state(), "nothing to transfer");
            return Ok(false);
        };
        let reply = self.router.send_request(
            session,
            ServiceCode::UPLOAD_TRANSFER,
            &EPath::instance(CLASS_ID, self.instance_id),
            &[*transfer_number],
        )?;
        if !reply.is_success() {
            error!(instance_id = self.instance_id, status = %reply.general_status, "upload transfer failed");
            self.finish(reply.general_status, false);
            return Ok(false);
        }

        let mut buf = &reply.data[..];
        let received_number = buf.try_get_u8()?;
        let packet_type = TransferPacketType(buf.try_get_u8()?);
        debug!(instance_id = self.instance_id, %packet_type, "received transfer packet");

        let State::UploadInProgress {
            transfer_size,
            transfer_number,
            content,
            ..
        } = &mut self.state
        else {
            unreachable!("checked above");
        };
        if received_number != *transfer_number {
            error!(instance_id = self.instance_id, "wrong transfer number");
            self.finish(GeneralStatusCode::INVALID_PARAMETER, false);
            return Ok(false);
        }
        match packet_type {
            TransferPacketType::FIRST | TransferPacketType::MIDDLE => {
                content.extend(buf.try_get_vec(*transfer_size as usize)?);
                *transfer_number = transfer_number.wrapping_add(1);
                Ok(true)
            }
            TransferPacketType::LAST | TransferPacketType::FIRST_AND_LAST => {
                // The last packet ends with a 16-bit checksum.
                let len = buf.remaining().saturating_sub(2);
                content.extend(buf.try_get_vec(len)?);
                self.finish(GeneralStatusCode::SUCCESS, true);
                Ok(false)
            }
            other => {
                self.finish(GeneralStatusCode::INVALID_REPLY_RECEIVED, false);
                Err(Error::InvalidPacket(format!(
                    "unexpected transfer packet type {other}"
                )))
            }
        }
    }

    /// Uploads the whole file, blocking until done.
    pub fn upload(&mut self, session: &dyn Session) -> Result<Vec<u8>> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.begin_upload(session, move |status, data| {
            let _ = tx.send((status, data));
        })?;
        while self.handle_transfers(session)? {}
        match rx.try_recv() {
            Ok((GeneralStatusCode::SUCCESS, data)) => Ok(data),
            Ok((status, _)) => Err(Error::Cip {
                context: format!("upload of file object {}", self.instance_id),
                status,
                additional: Vec::new(),
            }),
            Err(_) => Err(Error::Unsupported(format!(
                "file object {} is {} and cannot be uploaded",
                self.instance_id,
                self.state()
            ))),
        }
    }

    /// Ends an upload, returning to the loaded state and calling the handler.
    fn finish(&mut self, status: GeneralStatusCode, check_size: bool) {
        let State::UploadInProgress {
            file_size,
            content,
            handler,
            ..
        } = std::mem::replace(&mut self.state, State::Loaded)
        else {
            return;
        };
        if check_size && content.len() != file_size as usize {
            error!(
                instance_id = self.instance_id,
                expected = file_size,
                actual = content.len(),
                "wrong size of uploaded file"
            );
            handler(GeneralStatusCode::INVALID_PARAMETER, Vec::new());
        } else if status == GeneralStatusCode::SUCCESS {
            handler(status, content);
        } else {
            handler(status, Vec::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockSession;

    #[test]
    fn upload_in_three_packets() {
        let s = MockSession::new()
            .reply(&[2]) // state: loaded
            .reply(&[7, 0, 0, 0, 3]) // file size 7, transfer size 3
            .reply(&[0, 0, b'a', b'b', b'c'])
            .reply(&[1, 1, b'd', b'e', b'f'])
            .reply(&[2, 2, b'g', 0xAA, 0xBB]);
        let mut file = FileObject::new(&s, MessageRouter::new(), 1).unwrap();
        assert_eq!(file.state(), FileObjectStateCode::FILE_LOADED);
        assert_eq!(file.upload(&s).unwrap(), b"abcdefg");
        assert_eq!(file.state(), FileObjectStateCode::FILE_LOADED);

        let reqs = s.requests();
        assert_eq!(reqs[1].service, ServiceCode::INITIATE_UPLOAD);
        assert_eq!(reqs[1].data, [255]);
        assert_eq!(reqs[2].data, [0]);
        assert_eq!(reqs[4].data, [2]);
    }

    #[test]
    fn wrong_file_size() {
        let s = MockSession::new()
            .reply(&[2])
            .reply(&[5, 0, 0, 0, 3])
            .reply(&[0, 4, b'a', b'b', 0, 0]);
        let mut file = FileObject::new(&s, MessageRouter::new(), 1).unwrap();
        let err = file.upload(&s).unwrap_err();
        assert!(matches!(
            err,
            Error::Cip {
                status: GeneralStatusCode::INVALID_PARAMETER,
                ..
            }
        ));
    }

    #[test]
    fn wrong_transfer_number() {
        let s = MockSession::new()
            .reply(&[2])
            .reply(&[5, 0, 0, 0, 3])
            .reply(&[3, 0, 1, 2, 3]);
        let mut file = FileObject::new(&s, MessageRouter::new(), 1).unwrap();
        assert!(file.upload(&s).is_err());
    }

    #[test]
    fn empty_file_cannot_be_uploaded() {
        let s = MockSession::new().reply(&[1]);
        let mut file = FileObject::new(&s, MessageRouter::new(), 1).unwrap();
        assert_eq!(file.state(), FileObjectStateCode::FILE_EMPTY);
        assert!(file.upload(&s).is_err());
        assert_eq!(s.requests().len(), 1);
    }

    #[test]
    fn uploading_state_is_treated_as_loaded() {
        let s = MockSession::new().reply(&[5]);
        let file = FileObject::new(&s, MessageRouter::new(), 1).unwrap();
        assert_eq!(file.state(), FileObjectStateCode::FILE_LOADED);
    }
}
