//! OS scheduling hooks for the IO thread and the wait primitive it sleeps in.

use std::io;
use std::net::UdpSocket;
use std::time::Instant;

use crate::{Error, Result};

/// Real-time settings applied to the IO thread when it starts. Linux only;
/// on other platforms any non-default setting makes the thread fail to start.
///
/// These affect only the IO thread. Process-wide settings, such as
/// `mlockall`, are left to the application.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RealtimeConfig {
    /// Run the IO thread under `SCHED_FIFO` at this priority (1-99). Needs
    /// `CAP_SYS_NICE` or an `rtprio` limit.
    pub fifo_priority: Option<i32>,
    /// Pin the IO thread to these CPUs.
    pub cpu_affinity: Vec<usize>,
    /// Timer slack for the IO thread, nanoseconds. The kernel default of 50 µs
    /// delays every wakeup by up to that much; `SCHED_FIFO` threads ignore it.
    pub timer_slack_ns: Option<u64>,
}

impl RealtimeConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn apply(cfg: &RealtimeConfig) -> Result<()> {
    fn check(rc: libc::c_int, what: &str) -> Result<()> {
        if rc == 0 {
            Ok(())
        } else {
            let e = io::Error::last_os_error();
            Err(Error::Io(io::Error::new(e.kind(), format!("{what}: {e}"))))
        }
    }

    if !cfg.cpu_affinity.is_empty() {
        // SAFETY: cpu_set_t is plain data; CPU_SET bounds-checks the index.
        let rc = unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            for &cpu in &cfg.cpu_affinity {
                libc::CPU_SET(cpu, &mut set);
            }
            libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set)
        };
        check(rc, "sched_setaffinity")?;
    }

    if let Some(ns) = cfg.timer_slack_ns {
        // SAFETY: PR_SET_TIMERSLACK takes an unsigned long and affects only this thread.
        let rc = unsafe { libc::prctl(libc::PR_SET_TIMERSLACK, ns as libc::c_ulong, 0, 0, 0) };
        check(rc, "prctl(PR_SET_TIMERSLACK)")?;
    }

    if let Some(priority) = cfg.fifo_priority {
        let param = libc::sched_param {
            sched_priority: priority,
        };
        // SAFETY: param outlives the call; pthread_self is always valid.
        let rc =
            unsafe { libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param) };
        if rc != 0 {
            let e = io::Error::from_raw_os_error(rc);
            return Err(Error::Io(io::Error::new(
                e.kind(),
                format!("pthread_setschedparam(SCHED_FIFO, {priority}): {e}"),
            )));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn apply(cfg: &RealtimeConfig) -> Result<()> {
    if cfg.is_default() {
        Ok(())
    } else {
        Err(Error::Unsupported(
            "real-time IO thread settings are only supported on Linux".into(),
        ))
    }
}

/// Blocks until one of `sockets` is readable or `deadline` passes. Returns
/// which sockets are readable; spurious `true`s are possible, so the sockets
/// must be non-blocking.
#[cfg(unix)]
pub(crate) fn wait_readable<const N: usize>(
    sockets: [&UdpSocket; N],
    deadline: Instant,
) -> io::Result<[bool; N]> {
    use std::os::fd::AsRawFd;

    let mut fds = sockets.map(|s| libc::pollfd {
        fd: s.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    });
    let timeout = deadline.saturating_duration_since(Instant::now());

    #[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
    let rc = {
        // ppoll takes a timespec, so sub-millisecond deadlines are honored.
        let ts = libc::timespec {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as libc::c_long,
        };
        // SAFETY: fds and ts are valid for the duration of the call.
        unsafe { libc::ppoll(fds.as_mut_ptr(), N as libc::nfds_t, &ts, std::ptr::null()) }
    };
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd")))]
    let rc = {
        let ms = timeout.as_micros().div_ceil(1000).min(i32::MAX as u128) as libc::c_int;
        // SAFETY: fds is valid for the duration of the call.
        unsafe { libc::poll(fds.as_mut_ptr(), N as libc::nfds_t, ms) }
    };

    if rc < 0 {
        let e = io::Error::last_os_error();
        return if e.kind() == io::ErrorKind::Interrupted {
            Ok([false; N])
        } else {
            Err(e)
        };
    }
    Ok(fds.map(|f| f.revents & (libc::POLLIN | libc::POLLERR) != 0))
}

/// Fallback without poll: sleeps at most 1 ms and reports every socket as
/// possibly readable.
#[cfg(not(unix))]
pub(crate) fn wait_readable<const N: usize>(
    _sockets: [&UdpSocket; N],
    deadline: Instant,
) -> io::Result<[bool; N]> {
    let timeout = deadline.saturating_duration_since(Instant::now());
    std::thread::sleep(timeout.min(std::time::Duration::from_millis(1)));
    Ok([true; N])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn wait_times_out_then_sees_data() {
        let a = UdpSocket::bind("127.0.0.1:0").unwrap();
        let b = UdpSocket::bind("127.0.0.1:0").unwrap();
        let start = Instant::now();
        let ready = wait_readable([&a, &b], start + Duration::from_millis(5)).unwrap();
        assert!(start.elapsed() >= Duration::from_millis(5));
        #[cfg(unix)]
        assert_eq!(ready, [false, false]);
        let _ = ready;

        b.send_to(&[1], a.local_addr().unwrap()).unwrap();
        let ready = wait_readable([&a, &b], Instant::now() + Duration::from_secs(1)).unwrap();
        assert!(ready[0]);
    }

    #[test]
    fn default_config_applies_everywhere() {
        apply(&RealtimeConfig::default()).unwrap();
    }
}
