use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use super::connection::{ConnectionKey, IoCallbacks, IoConnection, IoConnectionConfig};
use super::thread::{Command, IoSender, IoThread, IoThreadConfig, OpenFlag};
use crate::cip::EPath;
use crate::cip::ServiceCode;
use crate::cip::connection_manager::{
    self as cm, ConnectionConfig, ConnectionType, Direction, ForwardCloseRequest,
    ForwardOpenRequest, ForwardOpenResponse, MAX_CONFIG_DATA, MAX_SMALL_SIZE, NetworkParams,
    SizeType, TransportClass, TransportTrigger,
};
use crate::eip::{CommonPacketItemId, decode_sockaddr};
use crate::session::Session;
use crate::{EIP_DEFAULT_IMPLICIT_PORT, Error, MessageRouter, Result};

/// Opens and closes implicit IO connections, and owns the [`IoThread`] that
/// services them.
///
/// Forward Open/Close run on the calling thread over the given session; the
/// cyclic data exchange runs on the IO thread.
#[derive(Debug)]
pub struct ConnectionManager {
    router: MessageRouter,
    io: IoThread,
    open_count: Arc<AtomicUsize>,
    next_serial: AtomicU16,
    next_connection_id: AtomicU32,
}

impl ConnectionManager {
    /// Starts an IO thread with the default configuration.
    pub fn new() -> Result<Self> {
        Self::with_config(MessageRouter::new(), IoThreadConfig::default())
    }

    pub fn with_config(router: MessageRouter, config: IoThreadConfig) -> Result<Self> {
        // Random starting points keep connection IDs and serial numbers from
        // colliding with those of an earlier run (or manager) that the target
        // may still hold; a target identifies a connection by its serial number
        // together with the originator vendor ID and serial number.
        let random = RandomState::new().build_hasher().finish();
        Ok(Self {
            router,
            io: IoThread::spawn(config)?,
            open_count: Arc::new(AtomicUsize::new(0)),
            next_serial: AtomicU16::new(random as u16),
            next_connection_id: AtomicU32::new(((random >> 16) as u32) << 16),
        })
    }

    pub fn io_thread(&self) -> &IoThread {
        &self.io
    }

    pub fn has_open_connections(&self) -> bool {
        self.open_count.load(Ordering::SeqCst) > 0
    }

    /// Opens an I/O connection. Uses a Large Forward Open when either
    /// direction's size on the wire exceeds [`MAX_SMALL_SIZE`].
    pub fn forward_open(
        &self,
        session: &dyn Session,
        config: &ConnectionConfig,
        callbacks: IoCallbacks,
    ) -> Result<IoConnectionHandle> {
        if config.config_data.len() > MAX_CONFIG_DATA {
            return Err(Error::Unsupported(format!(
                "configuration data is {} bytes; at most {MAX_CONFIG_DATA} fit in a Forward Open",
                config.config_data.len()
            )));
        }
        if config.timeout_multiplier > 7 {
            return Err(Error::Unsupported(format!(
                "timeout multiplier {} is above the maximum of 7",
                config.timeout_multiplier
            )));
        }
        if !matches!(
            config.transport_class,
            TransportClass::Class0 | TransportClass::Class1
        ) {
            return Err(Error::Unsupported(
                "only transport classes 0 and 1 are supported for I/O connections".into(),
            ));
        }
        let o2t_wire = config.wire_size(&config.o2t);
        let t2o_wire = config.wire_size(&config.t2o);
        if o2t_wire.max(t2o_wire) > u16::MAX as usize {
            return Err(Error::Unsupported(format!(
                "connection size {} is above the maximum of {}",
                o2t_wire.max(t2o_wire),
                u16::MAX
            )));
        }
        let large = o2t_wire.max(t2o_wire) > MAX_SMALL_SIZE as usize;

        let network_params = |dir: &Direction, wire: usize| NetworkParams {
            redundant_owner: false,
            connection_type: dir.connection_type,
            priority: dir.priority,
            size_type: dir.size_type,
            size: wire as u16,
        };
        let micros = |d: Duration| d.as_micros().min(u32::MAX as u128) as u32;
        let next_id = || self.next_connection_id.fetch_add(1, Ordering::Relaxed) + 1;
        let request = ForwardOpenRequest {
            // 2^10 ms ticks x 5: the target gives up on the request after about 5 s.
            priority_time_tick: 0x0A,
            timeout_ticks: 0x05,
            o2t_connection_id: if config.o2t.connection_type == ConnectionType::Multicast {
                next_id()
            } else {
                0
            },
            t2o_connection_id: if config.t2o.connection_type == ConnectionType::PointToPoint {
                next_id()
            } else {
                0
            },
            connection_serial_number: self.next_serial.fetch_add(1, Ordering::Relaxed),
            originator_vendor_id: config.originator_vendor_id,
            originator_serial_number: config.originator_serial_number,
            connection_timeout_multiplier: config.timeout_multiplier,
            o2t_rpi: micros(config.o2t.rpi),
            o2t_params: network_params(&config.o2t, o2t_wire),
            t2o_rpi: micros(config.t2o.rpi),
            t2o_params: network_params(&config.t2o, t2o_wire),
            transport: TransportTrigger {
                class: config.transport_class,
                trigger: config.trigger,
                server: false,
            },
            path: config.encoded_path(),
        };

        let service = if large {
            ServiceCode::LARGE_FORWARD_OPEN
        } else {
            ServiceCode::FORWARD_OPEN
        };
        let response = self
            .router
            .send_request(
                session,
                service,
                &EPath::instance(cm::CLASS_ID, 1),
                &request.encode(large),
            )?
            .check("forward open")?;
        let reply = ForwardOpenResponse::decode(&response.data)?;
        info!(
            o2t_id = reply.o2t_network_connection_id,
            t2o_id = reply.t2o_network_connection_id,
            serial = reply.connection_serial_number,
            o2t_api_us = reply.o2t_api,
            t2o_api_us = reply.t2o_api,
            large,
            "opened IO connection"
        );

        let peer_ip = *session.remote_addr().ip();
        let o2t_sockaddr = response
            .additional_packet_items
            .iter()
            .find(|item| item.type_id == CommonPacketItemId::O2T_SOCKADDR_INFO)
            .map(|item| decode_sockaddr(&mut &item.data[..]))
            .transpose()?;
        let destination = o2t_destination(o2t_sockaddr, peer_ip);

        let multiplier = 4u64 << config.timeout_multiplier;
        let io_config = IoConnectionConfig {
            o2t_id: reply.o2t_network_connection_id,
            t2o_id: reply.t2o_network_connection_id,
            serial_number: reply.connection_serial_number,
            o2t_api: Duration::from_micros(reply.o2t_api as u64),
            timeout: Duration::from_micros(reply.t2o_api as u64 * multiplier),
            o2t_size: config.o2t.size as usize,
            t2o_size: config.t2o.size as usize,
            o2t_fixed: config.o2t.size_type == SizeType::Fixed,
            t2o_fixed: config.t2o.size_type == SizeType::Fixed,
            has_sequence_count: config.transport_class.has_sequence_count(),
            o2t_real_time_format: config.o2t.run_idle_header,
            t2o_real_time_format: config.t2o.run_idle_header,
            destination,
            peer_ip,
        };

        let flag = OpenFlag::new(self.open_count.clone());
        let handle = IoConnectionHandle {
            key: io_config.key(),
            o2t_id: io_config.o2t_id,
            o2t_api: io_config.o2t_api,
            t2o_api: Duration::from_micros(reply.t2o_api as u64),
            destination,
            close_request: ForwardCloseRequest {
                connection_serial_number: reply.connection_serial_number,
                originator_vendor_id: config.originator_vendor_id,
                originator_serial_number: config.originator_serial_number,
                connection_path: config.path.as_bytes().to_vec(),
            },
            flag: flag.clone(),
            sender: self.io.sender().clone(),
        };
        self.io.sender().send(Command::Add {
            conn: IoConnection::new(io_config, Instant::now()),
            callbacks,
            flag,
        })?;
        Ok(handle)
    }

    /// Stops the connection on the IO thread and sends Forward Close.
    ///
    /// A connection that already timed out is only logged; nothing is sent.
    /// The connection is removed locally even if the target rejects the close.
    pub fn forward_close(&self, session: &dyn Session, handle: IoConnectionHandle) -> Result<()> {
        if !handle.flag.close() {
            warn!(t2o_id = handle.key.t2o_id, "connection is already closed");
            return Ok(());
        }
        handle.sender.send(Command::Remove(handle.key))?;
        info!(peer = %handle.key.peer_ip, t2o_id = handle.key.t2o_id, "closing IO connection");
        self.router
            .send_request(
                session,
                ServiceCode::FORWARD_CLOSE,
                &EPath::instance(cm::CLASS_ID, 1),
                &handle.close_request.encode(),
            )?
            .check("forward close")?;
        Ok(())
    }
}

/// Where to send O->T data: the address from the Forward Open reply's O->T
/// sockaddr item, with an unspecified IP or a zero port replaced by the
/// device's IP or port 2222.
fn o2t_destination(sockaddr: Option<SocketAddrV4>, peer_ip: Ipv4Addr) -> SocketAddrV4 {
    let Some(addr) = sockaddr else {
        return SocketAddrV4::new(peer_ip, EIP_DEFAULT_IMPLICIT_PORT);
    };
    let ip = if addr.ip().is_unspecified() {
        peer_ip
    } else {
        *addr.ip()
    };
    let port = if addr.port() == 0 {
        EIP_DEFAULT_IMPLICIT_PORT
    } else {
        addr.port()
    };
    SocketAddrV4::new(ip, port)
}

/// Handle to an open IO connection.
///
/// Dropping the handle does not close the connection; pass it to
/// [`ConnectionManager::forward_close`].
#[derive(Debug)]
pub struct IoConnectionHandle {
    key: ConnectionKey,
    o2t_id: u32,
    o2t_api: Duration,
    t2o_api: Duration,
    destination: SocketAddrV4,
    close_request: ForwardCloseRequest,
    flag: Arc<OpenFlag>,
    sender: IoSender,
}

impl IoConnectionHandle {
    /// Replaces the output data. Applied at the next O->T send.
    pub fn set_output(&self, data: Vec<u8>) -> Result<()> {
        self.sender.send(Command::SetOutput(self.key, data))
    }

    /// Sets the run/idle header bit sent with O->T data (real-time format only).
    pub fn set_run(&self, run: bool) -> Result<()> {
        self.sender.send(Command::SetRun(self.key, run))
    }

    /// False once the connection has timed out or been closed.
    pub fn is_open(&self) -> bool {
        self.flag.is_open()
    }

    pub fn t2o_connection_id(&self) -> u32 {
        self.key.t2o_id
    }

    /// Address of the device this connection is with.
    pub fn peer_ip(&self) -> Ipv4Addr {
        self.key.peer_ip
    }

    pub fn o2t_connection_id(&self) -> u32 {
        self.o2t_id
    }

    pub fn serial_number(&self) -> u16 {
        self.close_request.connection_serial_number
    }

    /// Actual O->T packet interval granted by the target.
    pub fn o2t_api(&self) -> Duration {
        self.o2t_api
    }

    /// Actual T->O packet interval granted by the target.
    pub fn t2o_api(&self) -> Duration {
        self.t2o_api
    }

    /// Where O->T data is sent.
    pub fn destination(&self) -> SocketAddrV4 {
        self.destination
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
    use std::sync::mpsc;

    use bytes::BufMut;

    use super::*;
    use crate::cip::connection_manager::ConnectionPath;
    use crate::eip::{
        CommonPacketItem, decode_connected_packet, encode_connected_packet, encode_sockaddr,
    };
    use crate::test_support::MockSession;

    const O2T_ID: u32 = 0xAA00_0001;
    const RPI: Duration = Duration::from_millis(5);

    fn forward_open_reply(t2o_id: u32) -> Vec<u8> {
        let mut d = Vec::new();
        d.put_u32_le(O2T_ID);
        d.put_u32_le(t2o_id);
        d.put_u16_le(1);
        d.put_u16_le(342);
        d.put_u32_le(0x12345);
        d.put_u32_le(RPI.as_micros() as u32);
        d.put_u32_le(RPI.as_micros() as u32);
        d.put_u16_le(0);
        d
    }

    fn params() -> ConnectionConfig {
        let mut config = ConnectionConfig::new(
            ConnectionPath::assembly(1, 150, 100),
            Direction::new(2, RPI).with_run_idle_header(),
            Direction::new(3, RPI),
        );
        config.originator_vendor_id = 342;
        config.originator_serial_number = 0x12345;
        config
    }

    /// Runs a connection against a fake target on loopback: Forward Open, one
    /// exchange in each direction, then either a timeout or a Forward Close.
    fn open_against_fake_target(
        session: &MockSession,
        target: &UdpSocket,
    ) -> (
        ConnectionManager,
        IoConnectionHandle,
        mpsc::Receiver<Vec<u8>>,
        mpsc::Receiver<()>,
    ) {
        let config = IoThreadConfig {
            bind_addr: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            ..Default::default()
        };
        let manager = ConnectionManager::with_config(MessageRouter::new(), config).unwrap();
        let (data_tx, data_rx) = mpsc::channel();
        let (close_tx, close_rx) = mpsc::channel();
        let callbacks = IoCallbacks::new()
            .on_receive(move |input| {
                let _ = data_tx.send(input.data.to_vec());
            })
            .on_close(move || {
                let _ = close_tx.send(());
            });
        let handle = manager.forward_open(session, &params(), callbacks).unwrap();
        target
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        (manager, handle, data_rx, close_rx)
    }

    fn fake_target() -> (UdpSocket, Vec<CommonPacketItem>) {
        fake_target_at(Ipv4Addr::LOCALHOST)
    }

    fn fake_target_at(ip: Ipv4Addr) -> (UdpSocket, Vec<CommonPacketItem>) {
        let target = UdpSocket::bind((ip, 0)).unwrap();
        let SocketAddr::V4(addr) = target.local_addr().unwrap() else {
            unreachable!()
        };
        let mut sockaddr = Vec::new();
        encode_sockaddr(addr, &mut sockaddr);
        (
            target,
            vec![CommonPacketItem::new(
                CommonPacketItemId::O2T_SOCKADDR_INFO,
                sockaddr,
            )],
        )
    }

    #[test]
    fn exchange_data_then_time_out() {
        let (target, items) = fake_target();
        let session = MockSession::new().reply_with_items(&forward_open_reply(0), items);
        let (manager, handle, data_rx, close_rx) = open_against_fake_target(&session, &target);
        handle.set_output(vec![0x11, 0x22]).unwrap();

        let request = &session.requests()[0];
        assert_eq!(request.service, ServiceCode::FORWARD_OPEN);
        assert_eq!(request.path, EPath::instance(6, 1));
        // Connection sizes include the sequence count (2) and run/idle header (4, O->T only).
        let o2t_ncp = u16::from_le_bytes([request.data[26], request.data[27]]);
        let t2o_ncp = u16::from_le_bytes([request.data[32], request.data[33]]);
        assert_eq!(o2t_ncp & 0x1FF, 2 + 2 + 4);
        assert_eq!(t2o_ncp & 0x1FF, 3 + 2);
        assert!(manager.has_open_connections());

        // O->T: wait for a packet carrying the new output.
        let mut buf = [0u8; 512];
        let scanner_addr = loop {
            let (len, from) = target.recv_from(&mut buf).unwrap();
            let pkt = decode_connected_packet(&buf[..len]).unwrap();
            assert_eq!(pkt.connection_id, O2T_ID);
            if pkt.data[2..6] == [1, 0, 0, 0] && pkt.data[6..] == [0x11, 0x22] {
                break from;
            }
        };

        // T->O, using the T2O ID from the canned Forward Open reply.
        let mut out = Vec::new();
        encode_connected_packet(&mut out, 0, 1, &[&[1, 0], &[7, 8, 9]]);
        target.send_to(&out, scanner_addr).unwrap();
        assert_eq!(
            data_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            [7, 8, 9]
        );

        // Silence from the target closes the connection after 4 x RPI.
        close_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!handle.is_open());
        assert!(!manager.has_open_connections());
        // Closing a timed-out connection sends nothing.
        manager.forward_close(&session, handle).unwrap();
        assert_eq!(session.requests().len(), 1);
    }

    #[test]
    fn forward_close_stops_sending() {
        let (target, items) = fake_target();
        let session = MockSession::new()
            .reply_with_items(&forward_open_reply(0), items)
            .reply(&[]);
        let (manager, handle, _data_rx, close_rx) = open_against_fake_target(&session, &target);
        target.recv_from(&mut [0u8; 512]).unwrap();

        manager.forward_close(&session, handle).unwrap();
        assert!(!manager.has_open_connections());
        let close = &session.requests()[1];
        assert_eq!(close.service, ServiceCode::FORWARD_CLOSE);
        assert_eq!(&close.data[2..4], 1u16.to_le_bytes());
        assert_eq!(close.data[10], 4); // path size in words
        assert!(close_rx.try_recv().is_err());

        // Drain anything sent before the close was processed, then expect silence.
        target.set_read_timeout(Some(RPI * 4)).unwrap();
        while target.recv_from(&mut [0u8; 512]).is_ok() {}
        assert!(target.recv_from(&mut [0u8; 512]).is_err());
    }

    #[test]
    fn rejected_forward_open() {
        let session =
            MockSession::new().reply_status(crate::cip::GeneralStatusCode::CONNECTION_FAILURE, &[]);
        let config = IoThreadConfig {
            bind_addr: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            ..Default::default()
        };
        let manager = ConnectionManager::with_config(MessageRouter::new(), config).unwrap();
        let err = manager
            .forward_open(&session, &params(), IoCallbacks::new())
            .unwrap_err();
        assert!(matches!(err, crate::Error::Cip { .. }));
        assert!(!manager.has_open_connections());
    }

    /// Two devices that both assign T2O ID 0 get separate connections, and
    /// each device's input reaches only its own callback.
    #[test]
    fn same_t2o_id_on_two_devices() {
        let config = IoThreadConfig {
            bind_addr: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
            ..Default::default()
        };
        let manager = ConnectionManager::with_config(MessageRouter::new(), config).unwrap();
        let scanner_port = manager.io_thread().local_addr().port();

        let mut devices = Vec::new();
        for ip in [Ipv4Addr::new(127, 0, 0, 1), Ipv4Addr::new(127, 0, 0, 2)] {
            let (target, items) = fake_target_at(ip);
            let session = MockSession::new()
                .with_remote(SocketAddrV4::new(ip, 44818))
                .reply_with_items(&forward_open_reply(0), items);
            let (tx, rx) = mpsc::channel();
            let callbacks = IoCallbacks::new().on_receive(move |input| {
                let _ = tx.send(input.data.to_vec());
            });
            let config = ConnectionConfig {
                timeout_multiplier: 7,
                ..params()
            };
            let handle = manager.forward_open(&session, &config, callbacks).unwrap();
            assert_eq!(handle.t2o_connection_id(), 0);
            assert_eq!(handle.peer_ip(), ip);
            devices.push((ip, target, handle, rx));
        }

        for (i, (ip, target, _, _)) in devices.iter().enumerate() {
            let mut out = Vec::new();
            encode_connected_packet(&mut out, 0, 1, &[&[1, 0], &[i as u8; 3]]);
            target
                .send_to(&out, SocketAddrV4::new(*ip, scanner_port))
                .unwrap();
        }
        for (i, (_, _, handle, rx)) in devices.iter().enumerate() {
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(1)).unwrap(),
                [i as u8; 3]
            );
            assert!(rx.try_recv().is_err());
            assert!(handle.is_open());
        }
    }

    #[test]
    fn config_data_is_sent_after_the_path() {
        let (target, items) = fake_target();
        let session = MockSession::new().reply_with_items(&forward_open_reply(0), items);
        let manager = ConnectionManager::with_config(
            MessageRouter::new(),
            IoThreadConfig {
                bind_addr: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                ..Default::default()
            },
        )
        .unwrap();
        let with_config = ConnectionConfig {
            config_data: vec![0xA1, 0xA2, 0xA3],
            ..params()
        };
        manager
            .forward_open(&session, &with_config, IoCallbacks::new())
            .unwrap();
        drop(target);

        let body = &session.requests()[0].data;
        // Path size in words: 8 path bytes + 2 segment header + 4 padded data.
        assert_eq!(body[35], 7);
        assert_eq!(&body[36 + 8..], [0x80, 2, 0xA1, 0xA2, 0xA3, 0]);

        let too_big = ConnectionConfig {
            config_data: vec![0; cm::MAX_CONFIG_DATA + 1],
            ..params()
        };
        assert!(matches!(
            manager.forward_open(&session, &too_big, IoCallbacks::new()),
            Err(Error::Unsupported(_))
        ));
        assert_eq!(
            session.requests().len(),
            1,
            "nothing sent for oversized data"
        );
    }

    #[test]
    fn o2t_destination_fallbacks() {
        let peer = Ipv4Addr::new(10, 0, 0, 5);
        let at = |s: &str| Some(s.parse::<SocketAddrV4>().unwrap());
        assert_eq!(o2t_destination(None, peer).to_string(), "10.0.0.5:2222");
        assert_eq!(
            o2t_destination(at("0.0.0.0:3000"), peer).to_string(),
            "10.0.0.5:3000"
        );
        assert_eq!(
            o2t_destination(at("0.0.0.0:0"), peer).to_string(),
            "10.0.0.5:2222"
        );
        assert_eq!(
            o2t_destination(at("10.0.0.9:0"), peer).to_string(),
            "10.0.0.9:2222"
        );
        assert_eq!(
            o2t_destination(at("239.1.2.3:2222"), peer).to_string(),
            "239.1.2.3:2222"
        );
    }

    /// Sizes beyond the 16-bit form switch to Large Forward Open, whose
    /// network parameters are 32 bits.
    #[test]
    fn large_sizes_use_large_forward_open() {
        let (_target, items) = fake_target();
        let session = MockSession::new().reply_with_items(&forward_open_reply(0), items);
        let manager = ConnectionManager::with_config(
            MessageRouter::new(),
            IoThreadConfig {
                bind_addr: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                ..Default::default()
            },
        )
        .unwrap();
        let config = ConnectionConfig {
            o2t: Direction::new(600, RPI),
            ..params()
        };
        manager
            .forward_open(&session, &config, IoCallbacks::new())
            .unwrap();
        let req = &session.requests()[0];
        assert_eq!(req.service, ServiceCode::LARGE_FORWARD_OPEN);
        let o2t_params = u32::from_le_bytes(req.data[26..30].try_into().unwrap());
        assert_eq!(NetworkParams::decode(o2t_params, true).size, 602);
    }
}
