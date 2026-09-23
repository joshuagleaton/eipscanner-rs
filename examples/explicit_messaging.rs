//! Reads the vendor ID from the Identity object and writes 10 bytes to an
//! assembly, like the C++ ExplicitMessagingExample.

mod common;

use bytes::Buf;
use clap::Parser;
use eipscanner::cip::{EPath, ServiceCode};
use eipscanner::{MessageRouter, SessionInfo};

/// Reads Identity attribute 1 (vendor ID), then writes bytes 1..10 to
/// attribute 3 of an assembly instance.
#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    device: common::Device,
    /// Assembly instance to write
    #[arg(long, default_value_t = 151)]
    write_assembly: u16,
}

fn main() -> eipscanner::Result<()> {
    let cli = Cli::parse();
    common::init_logging();
    let session = SessionInfo::connect(&cli.device.host, cli.device.port)?;
    let router = MessageRouter::new();

    let reply = router.send_request(
        &session,
        ServiceCode::GET_ATTRIBUTE_SINGLE,
        &EPath::attribute(0x01, 1, 1),
        &[],
    )?;
    if reply.is_success() {
        println!("vendor ID: {}", (&reply.data[..]).get_u16_le());
    } else {
        eprintln!("read failed: {}", reply.general_status);
    }

    let data: Vec<u8> = (1..=10).collect();
    let reply = router.send_request(
        &session,
        ServiceCode::SET_ATTRIBUTE_SINGLE,
        &EPath::attribute(0x04, cli.write_assembly, 3),
        &data,
    )?;
    if reply.is_success() {
        println!("write to assembly {} succeeded", cli.write_assembly);
    } else {
        eprintln!("write failed: {}", reply.general_status);
    }
    Ok(())
}
