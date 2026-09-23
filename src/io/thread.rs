use std::collections::{HashMap, hash_map};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tracing::{debug, error, info, trace, warn};

use super::connection::{ConnectionKey, IoCallbacks, IoConnection};
use super::rt::{self, RealtimeConfig};
use crate::eip::decode_connected_packet;
use crate::{EIP_DEFAULT_IMPLICIT_PORT, Error, Result};

#[derive(Debug, Clone)]
pub struct IoThreadConfig {
    /// Local address of the UDP socket used to send and receive implicit data.
    pub bind_addr: SocketAddrV4,
    pub realtime: RealtimeConfig,
    pub thread_name: String,
    /// Longest the thread sleeps when no connection needs attention.
    pub idle_wait: Duration,
}

impl Default for IoThreadConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, EIP_DEFAULT_IMPLICIT_PORT),
            realtime: RealtimeConfig::default(),
            thread_name: "eip-io".into(),
            idle_wait: Duration::from_millis(100),
        }
    }
}

/// Open/closed state shared between a connection's handle and the IO thread.
#[derive(Debug)]
pub(crate) struct OpenFlag {
    open: AtomicBool,
    open_count: Arc<AtomicUsize>,
}

impl OpenFlag {
    pub fn new(open_count: Arc<AtomicUsize>) -> Arc<Self> {
        open_count.fetch_add(1, Ordering::SeqCst);
        Arc::new(Self {
            open: AtomicBool::new(true),
            open_count,
        })
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    /// Marks the connection closed. Returns false if it already was.
    pub fn close(&self) -> bool {
        let was_open = self.open.swap(false, Ordering::SeqCst);
        if was_open {
            self.open_count.fetch_sub(1, Ordering::SeqCst);
        }
        was_open
    }
}

pub(crate) enum Command {
    Add {
        conn: IoConnection,
        callbacks: IoCallbacks,
        flag: Arc<OpenFlag>,
    },
    Remove(ConnectionKey),
    SetOutput(ConnectionKey, Vec<u8>),
    SetRun(ConnectionKey, bool),
    Shutdown,
}

/// Sends commands to the IO thread. Commands that change the schedule also
/// wake it through a loopback socket.
#[derive(Debug, Clone)]
pub(crate) struct IoSender {
    tx: mpsc::Sender<Command>,
    waker: Arc<UdpSocket>,
}

impl IoSender {
    pub fn send(&self, cmd: Command) -> Result<()> {
        let wake = !matches!(cmd, Command::SetOutput(..) | Command::SetRun(..));
        self.tx.send(cmd).map_err(|_| Error::IoThreadStopped)?;
        if wake {
            // Losing a wake byte only delays the command by `idle_wait`.
            let _ = self.waker.send(&[0]);
        }
        Ok(())
    }
}

/// The dedicated thread that sends and receives implicit (UDP) data.
///
/// Stops when dropped. Connections still open are not closed on the target;
/// they time out there.
#[derive(Debug)]
pub struct IoThread {
    sender: IoSender,
    local_addr: SocketAddrV4,
    join: Option<JoinHandle<()>>,
}

impl IoThread {
    /// Binds the UDP socket, starts the thread, and applies the real-time
    /// settings. Fails if any of those fail.
    pub fn spawn(config: IoThreadConfig) -> Result<Self> {
        let socket = UdpSocket::bind(config.bind_addr)?;
        socket.set_nonblocking(true)?;
        let local_addr = match socket.local_addr()? {
            SocketAddr::V4(a) => a,
            SocketAddr::V6(_) => unreachable!("bound to an IPv4 address"),
        };

        let wake_rx = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        wake_rx.set_nonblocking(true)?;
        let waker = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        waker.connect(wake_rx.local_addr()?)?;

        let (tx, rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let realtime = config.realtime.clone();
        let idle_wait = config.idle_wait;

        let join = std::thread::Builder::new()
            .name(config.thread_name.clone())
            .spawn(move || {
                let result = rt::apply(&realtime);
                let ok = result.is_ok();
                let _ = started_tx.send(result);
                if ok {
                    Worker::new(socket, wake_rx, rx, idle_wait).run();
                }
            })?;

        match started_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = join.join();
                return Err(e);
            }
            Err(_) => return Err(Error::IoThreadStopped),
        }
        info!(%local_addr, realtime = ?config.realtime, "IO thread started");

        Ok(Self {
            sender: IoSender {
                tx,
                waker: Arc::new(waker),
            },
            local_addr,
            join: Some(join),
        })
    }

    /// Address of the UDP socket.
    pub fn local_addr(&self) -> SocketAddrV4 {
        self.local_addr
    }

    pub(crate) fn sender(&self) -> &IoSender {
        &self.sender
    }
}

impl Drop for IoThread {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Shutdown);
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            error!("IO thread panicked");
        }
    }
}

/// Writes one byte per page so the buffer is backed by real memory before the
/// first packet. Zeroed allocations may otherwise be mapped lazily, which would
/// put page faults on the first sends and receives.
fn prefault(buf: &mut [u8]) {
    const PAGE: usize = 4096;
    for i in (0..buf.len()).step_by(PAGE) {
        // black_box keeps the compiler from dropping a store of zero into
        // memory it knows is already zero.
        buf[i] = std::hint::black_box(0);
    }
}

struct Entry {
    conn: IoConnection,
    callbacks: IoCallbacks,
    flag: Arc<OpenFlag>,
}

struct Worker {
    socket: UdpSocket,
    wake_rx: UdpSocket,
    commands: mpsc::Receiver<Command>,
    idle_wait: Duration,
    connections: HashMap<ConnectionKey, Entry>,
    rx_buf: Vec<u8>,
    tx_buf: Vec<u8>,
}

impl Worker {
    fn new(
        socket: UdpSocket,
        wake_rx: UdpSocket,
        commands: mpsc::Receiver<Command>,
        idle_wait: Duration,
    ) -> Self {
        let mut rx_buf = vec![0; 65536];
        let mut tx_buf = vec![0; 1500];
        prefault(&mut rx_buf);
        prefault(&mut tx_buf);
        tx_buf.clear();
        Self {
            socket,
            wake_rx,
            commands,
            idle_wait,
            connections: HashMap::new(),
            rx_buf,
            tx_buf,
        }
    }

    fn run(mut self) {
        loop {
            if !self.drain_commands() {
                break;
            }
            let now = Instant::now();
            self.service_connections(now);

            let deadline = self
                .connections
                .values()
                .map(|e| e.conn.next_deadline())
                .min()
                .unwrap_or(now + self.idle_wait)
                .min(now + self.idle_wait);

            match rt::wait_readable([&self.socket, &self.wake_rx], deadline) {
                Ok([data_ready, wake_ready]) => {
                    if data_ready {
                        self.receive_all();
                    }
                    if wake_ready {
                        while self.wake_rx.recv(&mut [0; 16]).is_ok() {}
                    }
                }
                Err(e) => {
                    error!(error = %e, "wait on IO sockets failed");
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        debug!("IO thread stopped");
    }

    /// Returns false on shutdown.
    fn drain_commands(&mut self) -> bool {
        loop {
            match self.commands.try_recv() {
                Ok(Command::Add {
                    conn,
                    callbacks,
                    flag,
                }) => {
                    let key = conn.config().key();
                    match self.connections.entry(key) {
                        hash_map::Entry::Occupied(_) => {
                            error!(
                                peer = %key.peer_ip,
                                t2o_id = key.t2o_id,
                                "connection with this device and T2O ID already exists"
                            );
                            flag.close();
                        }
                        hash_map::Entry::Vacant(slot) => {
                            slot.insert(Entry {
                                conn,
                                callbacks,
                                flag,
                            });
                        }
                    }
                }
                Ok(Command::Remove(key)) => {
                    self.connections.remove(&key);
                }
                Ok(Command::SetOutput(key, data)) => {
                    if let Some(e) = self.connections.get_mut(&key) {
                        e.conn.set_output(data);
                    }
                }
                Ok(Command::SetRun(key, run)) => {
                    if let Some(e) = self.connections.get_mut(&key) {
                        e.conn.set_run(run);
                    }
                }
                Ok(Command::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => return false,
                Err(mpsc::TryRecvError::Empty) => return true,
            }
        }
    }

    fn service_connections(&mut self, now: Instant) {
        let mut timed_out = Vec::new();
        for (&key, entry) in &mut self.connections {
            if entry.conn.is_timed_out(now) {
                timed_out.push(key);
                continue;
            }
            if !entry.conn.is_send_due(now) {
                continue;
            }
            if let Some(on_send) = &mut entry.callbacks.on_send {
                on_send(entry.conn.output_mut());
            }
            if entry.conn.build_packet(now, &mut self.tx_buf) {
                let dest = entry.conn.config().destination;
                if let Err(e) = self.socket.send_to(&self.tx_buf, dest) {
                    warn!(%dest, error = %e, "failed to send O2T data");
                }
            }
        }
        for key in timed_out {
            if let Some(mut entry) = self.connections.remove(&key) {
                warn!(
                    peer = %key.peer_ip,
                    t2o_id = key.t2o_id,
                    serial = entry.conn.config().serial_number,
                    "connection timed out"
                );
                entry.flag.close();
                if let Some(on_close) = entry.callbacks.on_close.take() {
                    on_close();
                }
            }
        }
    }

    fn receive_all(&mut self) {
        loop {
            let (len, from) = match self.socket.recv_from(&mut self.rx_buf) {
                Ok(r) => r,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                // ICMP port unreachable from a previous send surfaces here on some platforms.
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                Err(e) => {
                    warn!(error = %e, "failed to receive T2O data");
                    return;
                }
            };
            let now = Instant::now();
            let packet = match decode_connected_packet(&self.rx_buf[..len]) {
                Ok(p) => p,
                Err(e) => {
                    debug!(%from, error = %e, "dropping malformed implicit packet");
                    continue;
                }
            };
            let IpAddr::V4(peer_ip) = from.ip() else {
                continue;
            };
            let key = ConnectionKey {
                peer_ip,
                t2o_id: packet.connection_id,
            };
            let Some(entry) = self.connections.get_mut(&key) else {
                debug!(%from, t2o_id = packet.connection_id, "data for unknown connection");
                continue;
            };
            trace!(t2o_id = packet.connection_id, len, "received T2O data");
            if let Some(input) = entry.conn.on_receive(&packet, now)
                && let Some(on_receive) = &mut entry.callbacks.on_receive
            {
                on_receive(&input);
            }
        }
    }
}
