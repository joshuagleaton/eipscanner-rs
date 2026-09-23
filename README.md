# eipscanner-rs

An EtherNet/IP scanner library in Rust, ported from the C++
[EIPScanner](https://github.com/joshuagleaton/EIPScanner).

## Background

**EtherNet/IP** is an industrial protocol that carries **CIP** (Common Industrial
Protocol) messages over TCP and UDP. A **scanner** (this library) talks to
**adapters** or **targets** (drives, IO blocks, PLCs). There are two kinds of
traffic:

- **Explicit messaging**: request/response over TCP port 44818, used to read and
  write object attributes (for example "get the vendor ID from the Identity
  object"). Requests go through the device's *message router* and address an
  object with an *EPATH* (class, instance, attribute).
- **Implicit messaging** (IO connections): cyclic UDP packets on port 2222. A
  scanner opens a connection with a **Forward Open** request, after which both
  sides send data every **RPI** (requested packet interval). Directions are
  named **O->T** (originator to target, the scanner's outputs) and **T->O**
  (target to originator, the scanner's inputs). If T->O data stops for longer
  than the connection timeout (a multiple of the RPI), the connection is
  dropped.

## Features

- Explicit messaging (unconnected, SendRRData)
- Implicit messaging, class 0/1, point-to-point, on a dedicated IO thread with
  optional real-time scheduling
- Discovery (ListIdentity broadcast)
- CIP objects: Identity, Parameter (read), File (upload)
- EDS file reader: device info, assemblies, parameters, and I/O connections,
  which convert directly to Forward Open parameters
- Vendor objects (feature `vendor`, on by default): Rockwell PowerFlex 525 DPI
  faults, Yaskawa MP3300iec 8-bit path router

## Layout

The crate is split so the protocol code can be tested and reused without sockets.

| Module | What it does | I/O |
|---|---|---|
| `eip` | Encapsulation header, common packet format items | none |
| `cip` | EPATH, status and service codes, message router and Forward Open encoding | none |
| `session`, `message_router` | TCP session and explicit requests | blocking `std::net` |
| `objects`, `vendor` | Typed CIP objects built on the message router | blocking |
| `discovery` | ListIdentity broadcast | blocking UDP |
| `io` | IO connections and the IO thread | dedicated thread |
| `eds` | EDS file reader | reads a file |

## Usage

Explicit messaging:

```rust
use eipscanner::cip::{EPath, ServiceCode};
use eipscanner::{MessageRouter, SessionInfo};

let session = SessionInfo::connect("192.168.1.10", 44818)?;
let reply = MessageRouter::new()
    .send_request(&session, ServiceCode::GET_ATTRIBUTE_SINGLE, &EPath::attribute(0x01, 1, 1), &[])?
    .check("read vendor ID")?;
```

Implicit messaging:

```rust
use std::time::Duration;
use eipscanner::cip::connection_manager::{ConnectionConfig, ConnectionPath, Direction};
use eipscanner::io::{ConnectionManager, IoCallbacks};

let manager = ConnectionManager::new()?; // starts the IO thread, binds UDP 2222
let rpi = Duration::from_millis(10);
let mut config = ConnectionConfig::new(
    // Configuration assembly 151, output 150, input 100.
    ConnectionPath::assembly(151, 150, 100),
    Direction::new(32, rpi).with_run_idle_header(), // outputs (O->T)
    Direction::new(32, rpi),                        // inputs (T->O)
);
config.originator_vendor_id = 342;
config.originator_serial_number = 0x12345;
let io = manager.forward_open(
    &session,
    &config,
    IoCallbacks::new().on_receive(|input| println!("{:?}", input.data)),
)?;
io.set_output(vec![0; 32])?;
// ...
manager.forward_close(&session, io)?;
```

Callbacks run on the IO thread. They must return quickly and must not block;
hand data to other threads through a channel or similar.

One `ConnectionManager` handles connections to any number of devices: open a
`SessionInfo` per device and call `forward_open` with each. All connections
share the manager's IO thread and its UDP socket on port 2222, so use one
manager per process. Incoming data is matched to a connection by the device's
IP address and the T->O connection ID.

A `Direction`'s size is the application data size. The 2-byte sequence count
(class 1) and the 4-byte run/idle header are added to the size sent to the
target, and a Large Forward Open is used when that size exceeds 511 bytes.
`Direction` also sets the connection type, priority, and fixed or variable
size; `ConnectionConfig` holds configuration assembly data (`config_data`),
the transport class and trigger, and the timeout multiplier.

To reach a device behind a gateway or in a chassis, put a route in front of
the path. `ConnectionPath::route` takes the `port,link` form used by libplctag
and Logix tools:

```rust
// Backplane port 1, slot 3, then the module's assemblies.
let path = ConnectionPath::route("1,3")?.then(ConnectionPath::assembly(151, 150, 100));
```

### Connection settings from an EDS

A device's EDS (Electronic Data Sheet) file lists the I/O connections it
supports, with their paths, sizes, and packet intervals. Vendors publish it,
and many devices also store it in their File object (usually instance `0xC8`;
see the `file_object` example). `eds::Eds` reads it and turns a connection
entry into a `ConnectionConfig`:

```rust
use eipscanner::eds::Eds;

let eds = Eds::read("device.eds")?;
let mut config = eds.connection(1)?.to_connection_config()?;
config.originator_vendor_id = 342;
config.originator_serial_number = 0x12345;
let io = manager.forward_open(&session, &config, callbacks)?;
```

The EDS names the configuration assembly but not the data to put in it, so
`config_data` is left empty; set it if the device needs configuration. It is
sent as a data segment after the connection path. Connections whose input is
multicast-only are rejected, since multicast input isn't supported.

## The IO thread

`ConnectionManager` owns an `IoThread`: one OS thread with one UDP socket that
sends every connection's O->T data and receives all T->O data. It sleeps in
`ppoll` (nanosecond timeout) until the next send or timeout deadline. The send
schedule keeps its phase: a late wakeup doesn't shift later packets, and a long
stall skips missed slots rather than sending a burst.

The send and receive path doesn't allocate, and per-packet logging is at
`trace` level. Replacing output with `set_output` moves the new `Vec` to the IO
thread through a channel; to avoid that, write the output in place from the
`on_send` callback.

For tighter timing on Linux, pass a `RealtimeConfig` through `IoThreadConfig`:

```rust
use eipscanner::io::{IoThreadConfig, RealtimeConfig};

let config = IoThreadConfig {
    realtime: RealtimeConfig {
        fifo_priority: Some(80),   // SCHED_FIFO
        cpu_affinity: vec![3],     // pin to an isolated core
        timer_slack_ns: Some(1),
    },
    ..Default::default()
};
let manager = ConnectionManager::with_config(MessageRouter::new(), config)?;
```

If a setting can't be applied, the manager fails to start. These settings
affect only the IO thread.

Locking memory (`mlockall`) is not a library setting, because it applies to
every thread and allocation in the process. With `MCL_FUTURE`, allocations
beyond the `memlock` limit fail, and in Rust a failed allocation aborts. If
your application wants it, call it once in `main`:

```rust
// SAFETY: plain syscall with constant flags.
if unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) } != 0 {
    return Err(std::io::Error::last_os_error().into());
}
```

The IO thread touches its own buffers at startup so they are resident before
the first packet either way.

`SCHED_FIFO` and `mlockall` need privileges: run as root, give the binary
capabilities (`sudo setcap cap_sys_nice,cap_ipc_lock+ep <binary>`), or raise
`rtprio` and `memlock` in `/etc/security/limits.conf`. A PREEMPT_RT kernel and
an isolated core (`isolcpus=`) reduce jitter further.

Real-time settings are Linux only. On other Unix systems the thread waits with
`poll` (millisecond resolution); on Windows it polls at 1 ms.

## Examples

Each example takes `-h`/`--help`. Logging goes to stderr and follows `RUST_LOG`
(default `info`).

| Example | What it does |
|---|---|
| `discovery` | Broadcast ListIdentity, print devices |
| `eds_info` | Print the device and connections an EDS file describes |
| `explicit_messaging` | Read vendor ID, write an assembly |
| `identity_object` | Print the Identity object |
| `parameter_object` | List Parameter objects with scaling |
| `file_object` | Upload a file (e.g. the EDS) |
| `implicit_messaging` | Run class 1 connections to one or more devices, print per-connection send-interval and echo stats; `--eds FILE --connection N` takes the settings from an EDS; takes the real-time options |
| `powerflex525_faults` | Read (and optionally clear) a PowerFlex 525 fault queue |
| `yaskawa_assembly` | Write and read an assembly with 8-bit paths |

```shell
cargo run --example implicit_messaging -- 192.168.1.10 --rpi-ms 2 --seconds 10
```

## Testing

```shell
cargo test
```

The tests use the byte vectors from the C++ test suite where they exist.
Explicit-messaging objects run against a mock session. The IO connection tests
run the real IO thread against a fake target on loopback UDP.

### Against a real adapter

`tests/opener.rs` runs against the OpENer sample adapter in Docker: identity,
discovery, explicit assembly reads and writes, I/O connections with echoed
data, configuration data, simultaneous connections, EDS-derived connections,
and Large Forward Open. These tests are ignored by plain `cargo test`.
`tools/opener-tests.sh` starts the adapter from an OpENer checkout with the
Docker setup (branch `jgleaton/feature-docker` of
[joshuagleaton/OpENer](https://github.com/joshuagleaton/OpENer), expected at
`../OpENer` or `$OPENER_DIR`) and runs them:

```shell
tools/opener-tests.sh            # start the adapter, run all tests
tools/opener-tests.sh --down     # ... and stop it afterward
tools/opener-tests.sh -- config  # only tests matching "config"
```

The tests share UDP port 2222, so they run one at a time.

## Differences from the C++ library

API:

- Errors are returned as `eipscanner::Error` instead of thrown. A non-success
  CIP status stays a normal `MessageRouterResponse`; call `.check()` to turn it
  into an error.
- Objects and managers take the session per call (`&dyn Session`) instead of a
  `shared_ptr`. Test code can implement the `Session` trait to fake a device.
- `forward_open` takes its callbacks as an argument, so no data arrives before
  they are set, and returns a handle instead of a `weak_ptr`.
- `Yaskawa_MessageRouter`/`Yaskawa_EPath` are replaced by
  `MessageRouter::with_8bit_path_segments()`, which uses 8-bit segments when
  the ID fits.
- `DPIFaultManager` returns the faults it read. The `resetDevice` flag and
  tripped-device listener, unused in the C++ code, are gone.
- `IdentityObject`, `ParameterObject`, and the PowerFlex types are plain
  structs with public fields rather than getter/setter classes.

Behavior (bugs fixed in the port):

- T->O data is parsed as sequence count, then run/idle header, as the spec
  defines. The C++ code read them in the opposite order.
- `IdentityObject` reads the requested instance; the C++ code always read
  instance 1.
- Reading a CIP SHORT_STRING returns the string. The C++ `Buffer` operator
  read nothing.
- O->T connection IDs for multicast used a 16-bit counter shifted left by 16,
  which was always 0 in the high bits; both directions now share one 32-bit
  counter.
- The connection timeout multiplier no longer overflows 8 bits.
- Timing uses microsecond `Instant`s instead of milliseconds.
- T->O packets are accepted only from the target's IP address.
- One UDP socket on port 2222 sends and receives, instead of one socket per
  connection plus a bound receive socket.
- A File object upload returns to the loaded state when it ends, and stops on
  an error reply instead of retrying.
- Discovery collects replies for a total timeout, instead of until one
  receive times out.
- Unknown PowerFlex fault codes give `fault_description: None` rather than an
  exception.

## License

MIT, as the original. See [LICENSE](LICENSE).
