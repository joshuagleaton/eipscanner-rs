//! Integration tests against the OpENer sample adapter. They are ignored by
//! default; `tools/opener-tests.sh` starts the adapter and runs them.
//!
//! Environment:
//! - `EIP_TEST_ADAPTER`: adapter address (default 10.44.18.3)
//! - `EIP_TEST_EDS`: OpENer's sample EDS (default ../OpENer/data/opener_sample_app.eds)

use std::net::Ipv4Addr;
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eipscanner::cip::connection_manager::{ConnectionConfig, ConnectionPath, Direction};
use eipscanner::cip::{EPath, GeneralStatusCode, ServiceCode};
use eipscanner::eds::Eds;
use eipscanner::io::{ConnectionManager, IoCallbacks, IoConnectionHandle};
use eipscanner::objects::IdentityObject;
use eipscanner::{DiscoveryManager, Error, MessageRouter, SessionInfo};

const ASSEMBLY: u16 = 0x04;
const INPUT: u16 = 100;
const OUTPUT: u16 = 150;
const CONFIG: u16 = 151;
const INPUT_ONLY: u16 = 152;
const EXPLICIT: u16 = 154;
const RPI: Duration = Duration::from_millis(10);

/// Every test binds UDP port 2222 through its ConnectionManager, so they run one
/// at a time.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn adapter() -> String {
    std::env::var("EIP_TEST_ADAPTER").unwrap_or_else(|_| "10.44.18.3".into())
}

fn session() -> SessionInfo {
    SessionInfo::connect(&adapter(), 44818).expect("connect to the adapter; is it running?")
}

fn exclusive_owner() -> ConnectionConfig {
    let mut config = ConnectionConfig::new(
        ConnectionPath::assembly(CONFIG, OUTPUT, INPUT),
        Direction::new(32, RPI).with_run_idle_header(),
        Direction::new(32, RPI),
    );
    config.originator_vendor_id = 342;
    config.originator_serial_number = 0x0E1B_0001;
    config
}

/// Opens `config` with a receive callback that forwards each input.
fn open(
    manager: &ConnectionManager,
    session: &SessionInfo,
    config: &ConnectionConfig,
) -> eipscanner::Result<(IoConnectionHandle, mpsc::Receiver<Vec<u8>>)> {
    let (tx, rx) = mpsc::channel();
    let callbacks = IoCallbacks::new().on_receive(move |input| {
        let _ = tx.send(input.data.to_vec());
    });
    Ok((manager.forward_open(session, config, callbacks)?, rx))
}

/// Waits for an input equal to `expected`, returning how many inputs arrived.
fn wait_for(rx: &mpsc::Receiver<Vec<u8>>, expected: &[u8], within: Duration) -> usize {
    let deadline = Instant::now() + within;
    let mut seen = 0;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(data) if data == expected => return seen + 1,
            Ok(_) => seen += 1,
            Err(_) => break,
        }
    }
    panic!("no input equal to {expected:02x?} within {within:?} ({seen} other inputs)");
}

fn read_assembly(session: &SessionInfo, instance: u16) -> Vec<u8> {
    MessageRouter::new()
        .send_request(
            session,
            ServiceCode::GET_ATTRIBUTE_SINGLE,
            &EPath::attribute(ASSEMBLY, instance, 3),
            &[],
        )
        .unwrap()
        .check("read assembly")
        .unwrap()
        .data
}

#[test]
#[ignore = "needs an OpENer adapter; run tools/opener-tests.sh"]
fn identity() {
    let _guard = serial();
    let id = IdentityObject::read(&session(), &MessageRouter::new(), 1).unwrap();
    assert_eq!(id.vendor_id, 1);
    assert_eq!(id.product_code, 65001);
    assert_eq!(id.product_name, "OpENer PC");
}

#[test]
#[ignore = "needs an OpENer adapter; run tools/opener-tests.sh"]
fn discovery_finds_adapter() {
    let _guard = serial();
    let adapter: Ipv4Addr = adapter().parse().unwrap();
    let devices = DiscoveryManager::with_timeout(Duration::from_millis(2500))
        .discover()
        .unwrap();
    let found = devices
        .iter()
        .find(|d| *d.socket_address.ip() == adapter)
        .unwrap_or_else(|| panic!("{adapter} not among {devices:?}"));
    assert_eq!(found.identity.product_name, "OpENer PC");
}

#[test]
#[ignore = "needs an OpENer adapter; run tools/opener-tests.sh"]
fn explicit_assembly_write_and_read() {
    let _guard = serial();
    let session = session();
    let data: Vec<u8> = (0..32).map(|i| 0xC0 ^ i).collect();
    MessageRouter::new()
        .send_request(
            &session,
            ServiceCode::SET_ATTRIBUTE_SINGLE,
            &EPath::attribute(ASSEMBLY, EXPLICIT, 3),
            &data,
        )
        .unwrap()
        .check("write assembly")
        .unwrap();
    assert_eq!(read_assembly(&session, EXPLICIT), data);

    let reply = MessageRouter::new()
        .send_request(
            &session,
            ServiceCode::GET_ATTRIBUTE_SINGLE,
            &EPath::attribute(ASSEMBLY, 999, 3),
            &[],
        )
        .unwrap();
    assert_eq!(
        reply.general_status,
        GeneralStatusCode::PATH_DESTINATION_UNKNOWN
    );
}

/// The sample application copies outputs to inputs, so each output value
/// should come back on the input side, including after it changes.
#[test]
#[ignore = "needs an OpENer adapter; run tools/opener-tests.sh"]
fn exclusive_owner_echoes_outputs() {
    let _guard = serial();
    let session = session();
    let manager = ConnectionManager::new().unwrap();
    let (io, rx) = open(&manager, &session, &exclusive_owner()).unwrap();
    assert_eq!(io.o2t_api(), RPI);
    assert_eq!(io.t2o_api(), RPI);

    io.set_output(vec![0x5A; 32]).unwrap();
    wait_for(&rx, &[0x5A; 32], Duration::from_secs(1));
    io.set_output(vec![0xA5; 32]).unwrap();
    wait_for(&rx, &[0xA5; 32], Duration::from_secs(1));

    // One second at a 10 ms RPI.
    let start = Instant::now();
    let mut count = 0;
    while start.elapsed() < Duration::from_secs(1) {
        if rx.recv_timeout(Duration::from_millis(100)).is_ok() {
            count += 1;
        }
    }
    assert!((90..=110).contains(&count), "{count} inputs in 1 s");
    assert!(io.is_open());

    manager.forward_close(&session, io).unwrap();
    assert!(!manager.has_open_connections());
}

#[test]
#[ignore = "needs an OpENer adapter; run tools/opener-tests.sh"]
fn config_data_reaches_config_assembly() {
    let _guard = serial();
    let session = session();
    let manager = ConnectionManager::new().unwrap();

    let data: Vec<u8> = (0..10).map(|i| 0x30 + i).collect();
    let config = ConnectionConfig {
        config_data: data.clone(),
        ..exclusive_owner()
    };
    let (io, _rx) = open(&manager, &session, &config).unwrap();
    assert_eq!(read_assembly(&session, CONFIG), data);
    manager.forward_close(&session, io).unwrap();

    let wrong_size = ConnectionConfig {
        config_data: vec![1, 2, 3, 4],
        ..exclusive_owner()
    };
    match open(&manager, &session, &wrong_size) {
        Err(Error::Cip {
            status, additional, ..
        }) => {
            assert_eq!(status, GeneralStatusCode::CONNECTION_FAILURE);
            // Invalid configuration application path.
            assert_eq!(additional.first(), Some(&0x129));
        }
        other => panic!("expected a rejected Forward Open, got {other:?}"),
    }
}

/// An exclusive-owner and an input-only connection to the same device, both
/// running at once.
#[test]
#[ignore = "needs an OpENer adapter; run tools/opener-tests.sh"]
fn exclusive_owner_and_input_only_together() {
    let _guard = serial();
    let session = session();
    let manager = ConnectionManager::new().unwrap();
    let (owner, owner_rx) = open(&manager, &session, &exclusive_owner()).unwrap();
    owner.set_output(vec![0x77; 32]).unwrap();

    let mut input_only = ConnectionConfig::new(
        ConnectionPath::assembly(CONFIG, INPUT_ONLY, INPUT),
        Direction::new(0, RPI),
        Direction::new(32, RPI),
    );
    input_only.originator_vendor_id = 342;
    input_only.originator_serial_number = 0x0E1B_0001;
    let (listener, listener_rx) = open(&manager, &session, &input_only).unwrap();

    wait_for(&owner_rx, &[0x77; 32], Duration::from_secs(1));
    wait_for(&listener_rx, &[0x77; 32], Duration::from_secs(1));

    // Owner first: closing the input-only connection first can make OpENer
    // also drop the owner. Its UDP loop closes whichever connection it is on
    // when a receive fails (generic_networkhandler.c, CheckAndHandleConsumingUdpSocket),
    // and the input-only close can leave such a failure behind.
    manager.forward_close(&session, owner).unwrap();
    manager.forward_close(&session, listener).unwrap();
}

#[test]
#[ignore = "needs an OpENer adapter; run tools/opener-tests.sh"]
fn connections_from_eds() {
    let _guard = serial();
    let path = std::env::var("EIP_TEST_EDS").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../OpENer/data/opener_sample_app.eds"
        )
        .into()
    });
    let eds = Eds::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let session = session();
    let manager = ConnectionManager::new().unwrap();

    for number in [1, 2] {
        let mut config = eds
            .connection(number)
            .unwrap()
            .to_connection_config()
            .unwrap();
        config.originator_vendor_id = 342;
        config.originator_serial_number = 0x0E1B_0001;
        let (io, rx) = open(&manager, &session, &config).unwrap();
        rx.recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|_| panic!("no input on Connection{number}"));
        manager.forward_close(&session, io).unwrap();
    }
}

/// A size beyond the 16-bit Forward Open is sent as a Large Forward Open;
/// OpENer decodes it and rejects the size, which doesn't match assembly 150.
#[test]
#[ignore = "needs an OpENer adapter; run tools/opener-tests.sh"]
fn large_forward_open_is_decoded() {
    let _guard = serial();
    let session = session();
    let manager = ConnectionManager::new().unwrap();
    let config = ConnectionConfig {
        o2t: Direction::new(600, RPI).with_run_idle_header(),
        ..exclusive_owner()
    };
    match open(&manager, &session, &config) {
        Err(Error::Cip { additional, .. }) => {
            // Invalid O->T network connection size.
            assert_eq!(additional.first(), Some(&0x127));
        }
        other => panic!("expected a rejected Forward Open, got {other:?}"),
    }
}
