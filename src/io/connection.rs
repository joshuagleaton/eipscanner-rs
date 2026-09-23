use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};

use bytes::Buf;
use tracing::warn;

use crate::eip::{ConnectedPacket, encode_connected_packet};

/// Data received on a T->O connection. Passed to the receive callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputData<'a> {
    /// CPF sequenced address item sequence number.
    pub sequence_number: u32,
    /// CIP sequence count; 0 for class 0 connections.
    pub sequence_count: u16,
    /// 32-bit run/idle header; 0 unless the connection uses the real-time format.
    pub run_idle_header: u32,
    pub data: &'a [u8],
}

pub type ReceiveHandler = Box<dyn FnMut(&InputData<'_>) + Send>;
pub type SendHandler = Box<dyn FnMut(&mut Vec<u8>) + Send>;
pub type CloseHandler = Box<dyn FnOnce() + Send>;

/// Callbacks for an IO connection. All of them run on the IO thread, so they
/// must return quickly and must not block.
#[derive(Default)]
pub struct IoCallbacks {
    pub(crate) on_receive: Option<ReceiveHandler>,
    pub(crate) on_send: Option<SendHandler>,
    pub(crate) on_close: Option<CloseHandler>,
}

impl IoCallbacks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called for each T->O packet that passes the size check.
    pub fn on_receive(mut self, f: impl FnMut(&InputData<'_>) + Send + 'static) -> Self {
        self.on_receive = Some(Box::new(f));
        self
    }

    /// Called just before each O->T packet is built, with the output data to
    /// modify in place.
    pub fn on_send(mut self, f: impl FnMut(&mut Vec<u8>) + Send + 'static) -> Self {
        self.on_send = Some(Box::new(f));
        self
    }

    /// Called once if the connection times out. Not called on forward close.
    pub fn on_close(mut self, f: impl FnOnce() + Send + 'static) -> Self {
        self.on_close = Some(Box::new(f));
        self
    }
}

impl std::fmt::Debug for IoCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IoCallbacks")
            .field("on_receive", &self.on_receive.is_some())
            .field("on_send", &self.on_send.is_some())
            .field("on_close", &self.on_close.is_some())
            .finish()
    }
}

/// Identifies a connection on the IO thread. T->O connection IDs are only
/// unique per device, so the device address is part of the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ConnectionKey {
    pub peer_ip: Ipv4Addr,
    pub t2o_id: u32,
}

/// Settings of an opened connection, from the Forward Open request and reply.
#[derive(Debug, Clone)]
pub(crate) struct IoConnectionConfig {
    pub o2t_id: u32,
    pub t2o_id: u32,
    pub serial_number: u16,
    pub o2t_api: Duration,
    pub timeout: Duration,
    pub o2t_size: usize,
    pub t2o_size: usize,
    pub o2t_fixed: bool,
    pub t2o_fixed: bool,
    pub has_sequence_count: bool,
    pub o2t_real_time_format: bool,
    pub t2o_real_time_format: bool,
    /// Where O->T packets are sent.
    pub destination: SocketAddrV4,
    /// T->O packets from any other address are dropped.
    pub peer_ip: Ipv4Addr,
}

impl IoConnectionConfig {
    pub fn key(&self) -> ConnectionKey {
        ConnectionKey {
            peer_ip: self.peer_ip,
            t2o_id: self.t2o_id,
        }
    }
}

/// Implicit messaging state of one connection: the O->T send schedule, sequence
/// numbers, and the T->O timeout. Performs no I/O.
pub(crate) struct IoConnection {
    cfg: IoConnectionConfig,
    output: Vec<u8>,
    run: bool,
    o2t_sequence_number: u32,
    sequence_count: u16,
    next_send: Instant,
    last_receive: Instant,
    o2t_size_warned: bool,
    t2o_size_warned: bool,
}

impl IoConnection {
    pub fn new(cfg: IoConnectionConfig, now: Instant) -> Self {
        Self {
            output: vec![0; cfg.o2t_size],
            cfg,
            run: true,
            o2t_sequence_number: 0,
            sequence_count: 0,
            next_send: now,
            last_receive: now,
            o2t_size_warned: false,
            t2o_size_warned: false,
        }
    }

    pub fn config(&self) -> &IoConnectionConfig {
        &self.cfg
    }

    pub fn set_output(&mut self, data: Vec<u8>) {
        self.output = data;
    }

    pub fn output_mut(&mut self) -> &mut Vec<u8> {
        &mut self.output
    }

    pub fn set_run(&mut self, run: bool) {
        self.run = run;
    }

    pub fn timeout_at(&self) -> Instant {
        self.last_receive + self.cfg.timeout
    }

    pub fn is_timed_out(&self, now: Instant) -> bool {
        now >= self.timeout_at()
    }

    pub fn is_send_due(&self, now: Instant) -> bool {
        now >= self.next_send
    }

    /// The next time this connection needs attention.
    pub fn next_deadline(&self) -> Instant {
        self.next_send.min(self.timeout_at())
    }

    /// Builds the next O->T packet into `out` and advances the schedule.
    /// Returns false if the packet must not be sent because the output size
    /// doesn't match a fixed-size connection.
    pub fn build_packet(&mut self, now: Instant, out: &mut Vec<u8>) -> bool {
        // Keep the original phase; skip slots missed entirely rather than bursting.
        self.next_send += self.cfg.o2t_api;
        if self.next_send <= now {
            let missed = (now - self.next_send).as_nanos() / self.cfg.o2t_api.as_nanos().max(1) + 1;
            self.next_send += self.cfg.o2t_api * missed as u32;
        }

        if self.cfg.o2t_fixed && self.output.len() != self.cfg.o2t_size {
            if !self.o2t_size_warned {
                warn!(
                    o2t_id = self.cfg.o2t_id,
                    expected = self.cfg.o2t_size,
                    actual = self.output.len(),
                    "output size does not match fixed-size connection; not sending"
                );
                self.o2t_size_warned = true;
            }
            return false;
        }
        self.o2t_size_warned = false;

        self.o2t_sequence_number = self.o2t_sequence_number.wrapping_add(1);
        let seq_bytes;
        let header_bytes;
        let mut parts: [&[u8]; 3] = [&[], &[], &self.output];
        if self.cfg.has_sequence_count {
            self.sequence_count = self.sequence_count.wrapping_add(1);
            seq_bytes = self.sequence_count.to_le_bytes();
            parts[0] = &seq_bytes;
        }
        if self.cfg.o2t_real_time_format {
            header_bytes = u32::from(self.run).to_le_bytes();
            parts[1] = &header_bytes;
        }
        encode_connected_packet(out, self.cfg.o2t_id, self.o2t_sequence_number, &parts);
        true
    }

    /// Parses T->O data. Returns `None` for packets that are malformed or have
    /// the wrong size; those don't reset the timeout.
    pub fn on_receive<'a>(
        &mut self,
        packet: &ConnectedPacket<'a>,
        now: Instant,
    ) -> Option<InputData<'a>> {
        let mut buf = packet.data;
        let sequence_count = if self.cfg.has_sequence_count {
            buf.try_get_u16_le().ok()?
        } else {
            0
        };
        let run_idle_header = if self.cfg.t2o_real_time_format {
            buf.try_get_u32_le().ok()?
        } else {
            0
        };
        if self.cfg.t2o_fixed && buf.len() != self.cfg.t2o_size {
            if !self.t2o_size_warned {
                warn!(
                    t2o_id = self.cfg.t2o_id,
                    expected = self.cfg.t2o_size,
                    actual = buf.len(),
                    "received data size does not match fixed-size connection; ignoring"
                );
                self.t2o_size_warned = true;
            }
            return None;
        }
        self.t2o_size_warned = false;
        self.last_receive = now;
        Some(InputData {
            sequence_number: packet.sequence_number,
            sequence_count,
            run_idle_header,
            data: buf,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eip::decode_connected_packet;

    fn config() -> IoConnectionConfig {
        IoConnectionConfig {
            o2t_id: 0x100,
            t2o_id: 0x200,
            serial_number: 1,
            o2t_api: Duration::from_millis(10),
            timeout: Duration::from_millis(40),
            o2t_size: 2,
            t2o_size: 3,
            o2t_fixed: true,
            t2o_fixed: true,
            has_sequence_count: true,
            o2t_real_time_format: true,
            t2o_real_time_format: false,
            destination: "127.0.0.1:2222".parse().unwrap(),
            peer_ip: Ipv4Addr::LOCALHOST,
        }
    }

    #[test]
    fn o2t_packet_layout_and_counters() {
        let t0 = Instant::now();
        let mut conn = IoConnection::new(config(), t0);
        conn.set_output(vec![0xAA, 0xBB]);
        assert!(conn.is_send_due(t0));

        let mut out = Vec::new();
        assert!(conn.build_packet(t0, &mut out));
        let pkt = decode_connected_packet(&out).unwrap();
        assert_eq!(pkt.connection_id, 0x100);
        assert_eq!(pkt.sequence_number, 1);
        // sequence count, run header (run = 1), data
        assert_eq!(pkt.data, [1, 0, 1, 0, 0, 0, 0xAA, 0xBB]);

        conn.set_run(false);
        assert!(conn.build_packet(t0 + Duration::from_millis(10), &mut out));
        let pkt = decode_connected_packet(&out).unwrap();
        assert_eq!(pkt.sequence_number, 2);
        assert_eq!(pkt.data, [2, 0, 0, 0, 0, 0, 0xAA, 0xBB]);
    }

    #[test]
    fn schedule_keeps_phase_and_skips_missed_slots() {
        let t0 = Instant::now();
        let mut conn = IoConnection::new(config(), t0);
        let mut out = Vec::new();
        conn.build_packet(t0, &mut out);
        assert_eq!(conn.next_send, t0 + Duration::from_millis(10));

        // Woke 2 ms late: next slot stays on the 10 ms grid.
        conn.build_packet(t0 + Duration::from_millis(12), &mut out);
        assert_eq!(conn.next_send, t0 + Duration::from_millis(20));

        // Stalled for 35 ms: slots at 20 and 30 are skipped, not sent in a burst.
        conn.build_packet(t0 + Duration::from_millis(55), &mut out);
        assert_eq!(conn.next_send, t0 + Duration::from_millis(60));
    }

    #[test]
    fn fixed_size_mismatch_is_not_sent() {
        let t0 = Instant::now();
        let mut conn = IoConnection::new(config(), t0);
        conn.set_output(vec![1, 2, 3]);
        let mut out = Vec::new();
        assert!(!conn.build_packet(t0, &mut out));
    }

    #[test]
    fn receive_resets_timeout() {
        let t0 = Instant::now();
        let mut conn = IoConnection::new(config(), t0);
        let t1 = t0 + Duration::from_millis(30);
        let pkt = ConnectedPacket {
            connection_id: 0x200,
            sequence_number: 9,
            data: &[5, 0, 1, 2, 3],
        };
        let input = conn.on_receive(&pkt, t1).unwrap();
        assert_eq!(input.sequence_count, 5);
        assert_eq!(input.data, [1, 2, 3]);
        assert!(!conn.is_timed_out(t0 + Duration::from_millis(60)));
        assert!(conn.is_timed_out(t1 + Duration::from_millis(40)));
    }

    #[test]
    fn wrong_size_input_is_ignored() {
        let t0 = Instant::now();
        let mut conn = IoConnection::new(config(), t0);
        let pkt = ConnectedPacket {
            connection_id: 0x200,
            sequence_number: 1,
            data: &[5, 0, 1, 2],
        };
        assert!(
            conn.on_receive(&pkt, t0 + Duration::from_millis(30))
                .is_none()
        );
        assert!(conn.is_timed_out(t0 + Duration::from_millis(40)));
    }
}
