//! Opens a class 1 IO connection to each given device, exchanges data for a
//! while, then closes them. All connections share one IO thread. Prints per
//! connection the O->T send intervals, which show the IO thread's timing, and
//! how many inputs echoed the outputs back.
//!
//! Connection path and sizes match the C++ ImplicitMessagingExample and the
//! OpENer sample adapter: config assembly 151, output 150, input 100. The
//! sample adapter copies its outputs to its inputs, so each device is sent its
//! own fill byte and should echo it.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
use eipscanner::cip::connection_manager::{ConnectionConfig, ConnectionPath, Direction};
use eipscanner::eds::Eds;
use eipscanner::io::{ConnectionManager, IoCallbacks, IoThreadConfig, RealtimeConfig};
use eipscanner::{EIP_DEFAULT_EXPLICIT_PORT, MessageRouter, SessionInfo};

/// Opens a class 1 IO connection to each device, exchanges data for a while,
/// then closes them and prints per-connection statistics.
#[derive(Parser)]
struct Cli {
    /// Device IP addresses or host names
    #[arg(required = true)]
    hosts: Vec<String>,
    /// TCP port for explicit messaging
    #[arg(long, default_value_t = EIP_DEFAULT_EXPLICIT_PORT)]
    port: u16,
    /// Take the connection settings from this EDS file instead of the
    /// built-in ones for the OpENer sample adapter
    #[arg(long)]
    eds: Option<std::path::PathBuf>,
    /// Which ConnectionN of the EDS to open
    #[arg(long, default_value_t = 1, requires = "eds")]
    connection: u32,
    /// Requested packet interval, ms (fractions allowed) [default: the EDS
    /// default, or 10]
    #[arg(long)]
    rpi_ms: Option<f64>,
    /// Connection times out after (4 << N) x RPI without input. Raise it for
    /// adapters that send slower than the RPI they grant; OpENer rounds up to
    /// a multiple of 10 ms.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=7))]
    timeout_multiplier: u8,
    /// How long to run, seconds
    #[arg(long, default_value_t = 5)]
    seconds: u64,
    /// Configuration data for the configuration assembly, as hex bytes, e.g.
    /// "01 02 03" (the OpENer sample's assembly 151 takes 10 bytes)
    #[arg(long, value_parser = parse_hex)]
    config_data: Option<HexBytes>,
    /// Data size each way, bytes [default: from the EDS, or 32]
    #[arg(long)]
    size: Option<u16>,
    /// Run the IO thread under SCHED_FIFO at this priority (Linux)
    #[arg(long, value_parser = clap::value_parser!(i32).range(1..=99))]
    fifo_priority: Option<i32>,
    /// Pin the IO thread to this CPU (Linux)
    #[arg(long)]
    cpu: Option<usize>,
    /// Lock the process's memory with mlockall before starting (Linux)
    #[arg(long)]
    lock_memory: bool,
    /// IO thread timer slack, ns (Linux)
    #[arg(long)]
    timer_slack_ns: Option<u64>,
}

#[derive(Default)]
struct Stats {
    last_send: Option<Instant>,
    sends: u64,
    send_sum: Duration,
    send_min: Option<Duration>,
    send_max: Duration,
    received: u64,
    echoed: u64,
    timed_out: bool,
}

fn main() -> eipscanner::Result<()> {
    let cli = Cli::parse();
    common::init_logging();
    let run_for = Duration::from_secs(cli.seconds);

    if cli.lock_memory {
        lock_memory()?;
    }
    let realtime = RealtimeConfig {
        fifo_priority: cli.fifo_priority,
        cpu_affinity: cli.cpu.into_iter().collect(),
        timer_slack_ns: cli.timer_slack_ns,
    };
    let manager = ConnectionManager::with_config(
        MessageRouter::new(),
        IoThreadConfig {
            realtime,
            ..Default::default()
        },
    )?;
    let mut config = match &cli.eds {
        Some(path) => {
            let eds = Eds::read(path)?;
            let conn = eds.connection(cli.connection)?;
            println!(
                "{}: Connection{} \"{}\", path {}",
                path.display(),
                conn.number,
                conn.name,
                conn.path
            );
            conn.to_connection_config()?
        }
        None => {
            let rpi = Duration::from_millis(10);
            ConnectionConfig::new(
                ConnectionPath::assembly(151, 150, 100),
                Direction::new(32, rpi).with_run_idle_header(),
                Direction::new(32, rpi),
            )
        }
    };
    config.originator_vendor_id = 342;
    config.originator_serial_number = 0x12345;
    config.timeout_multiplier = cli.timeout_multiplier;
    if let Some(HexBytes(data)) = &cli.config_data {
        config.config_data = data.clone();
    }
    if let Some(ms) = cli.rpi_ms {
        let rpi = Duration::from_secs_f64(ms / 1000.0);
        config.o2t.rpi = rpi;
        config.t2o.rpi = rpi;
    }
    if let Some(size) = cli.size {
        config.o2t.size = size;
        config.t2o.size = size;
    }
    let size = config.o2t.size;

    let mut connections = Vec::new();
    for (i, host) in cli.hosts.iter().enumerate() {
        let session = SessionInfo::connect(host, cli.port)?;
        let fill = 0x10 + i as u8;
        let stats = Arc::new(Mutex::new(Stats::default()));
        let (recv_stats, send_stats, close_stats) = (stats.clone(), stats.clone(), stats.clone());
        let callbacks = IoCallbacks::new()
            .on_receive(move |input| {
                tracing::debug!(seq = input.sequence_count, data = ?input.data, "received");
                let mut s = recv_stats.lock().unwrap();
                s.received += 1;
                if !input.data.is_empty() && input.data.iter().all(|&b| b == fill) {
                    s.echoed += 1;
                }
            })
            .on_send(move |_output| {
                let now = Instant::now();
                let mut s = send_stats.lock().unwrap();
                if let Some(last) = s.last_send {
                    let dt = now - last;
                    s.sends += 1;
                    s.send_sum += dt;
                    s.send_min = Some(s.send_min.map_or(dt, |m| m.min(dt)));
                    s.send_max = s.send_max.max(dt);
                }
                s.last_send = Some(now);
            })
            .on_close(move || close_stats.lock().unwrap().timed_out = true);

        let io = manager.forward_open(&session, &config, callbacks)?;
        io.set_output(vec![fill; size as usize])?;
        println!(
            "{host}: connection open, O->T API {:?}, T->O API {:?}",
            io.o2t_api(),
            io.t2o_api()
        );
        connections.push((host, session, io, stats));
    }

    let start = Instant::now();
    while manager.has_open_connections() && start.elapsed() < run_for {
        std::thread::sleep(Duration::from_millis(100));
    }

    for (host, session, io, stats) in connections {
        manager.forward_close(&session, io)?;
        let s = stats.lock().unwrap();
        let sends = if s.sends > 0 {
            format!(
                "sent {}, interval min {:?} mean {:?} max {:?}",
                s.sends + 1,
                s.send_min.unwrap_or_default(),
                s.send_sum / s.sends as u32,
                s.send_max
            )
        } else {
            "sent 0-1".to_string()
        };
        // With no output data there is nothing for the adapter to echo.
        let echoed = if size > 0 {
            format!(", echoed {}", s.echoed)
        } else {
            String::new()
        };
        println!(
            "{host}: {sends}; received {}{echoed}{}",
            s.received,
            if s.timed_out { "; TIMED OUT" } else { "" }
        );
    }
    Ok(())
}

/// Keeps all current and future memory of the process in RAM. This is a
/// process-wide decision, so it belongs to the application, not the library.
#[cfg(target_os = "linux")]
fn lock_memory() -> eipscanner::Result<()> {
    // SAFETY: plain syscall with constant flags.
    if unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn lock_memory() -> eipscanner::Result<()> {
    Err(eipscanner::Error::Unsupported(
        "--lock-memory is Linux only".into(),
    ))
}

/// One argument holding several bytes; a bare `Vec<u8>` would make clap
/// expect several arguments.
#[derive(Clone)]
struct HexBytes(Vec<u8>);

fn parse_hex(s: &str) -> Result<HexBytes, String> {
    s.split_whitespace()
        .map(|t| u8::from_str_radix(t, 16).map_err(|_| format!("'{t}' is not a hex byte")))
        .collect::<Result<_, _>>()
        .map(HexBytes)
}
