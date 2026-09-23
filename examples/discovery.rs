//! Broadcasts ListIdentity and prints the devices that answer.

mod common;

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use clap::Parser;
use eipscanner::{BroadcastTarget, DiscoveryManager, EIP_DEFAULT_EXPLICIT_PORT};

/// Broadcasts an EtherNet/IP ListIdentity request and prints each reply.
#[derive(Parser)]
struct Cli {
    /// Broadcast address [default: the broadcast address of every IPv4
    /// interface, including Docker bridges]
    #[arg(long)]
    broadcast: Option<Ipv4Addr>,
    /// How long to collect replies, ms. Devices may delay broadcast replies by up to 2 s.
    #[arg(long, default_value_t = 2500)]
    timeout_ms: u64,
}

fn main() -> eipscanner::Result<()> {
    let cli = Cli::parse();
    common::init_logging();
    let target = match cli.broadcast {
        Some(ip) => BroadcastTarget::Address(SocketAddrV4::new(ip, EIP_DEFAULT_EXPLICIT_PORT)),
        None => BroadcastTarget::AllInterfaces,
    };

    let devices =
        DiscoveryManager::new(target, Duration::from_millis(cli.timeout_ms)).discover()?;
    if devices.is_empty() {
        eprintln!("no devices answered");
    }
    for device in devices {
        let id = &device.identity;
        println!(
            "{}  {}  vendor={} type={} product={} rev={} serial={:#010x}",
            device.socket_address,
            id.product_name,
            id.vendor_id,
            id.device_type,
            id.product_code,
            id.revision,
            id.serial_number
        );
    }
    Ok(())
}
