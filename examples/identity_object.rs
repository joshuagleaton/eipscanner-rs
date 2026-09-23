//! Reads and prints a device's Identity object.

mod common;

use clap::Parser;
use eipscanner::objects::IdentityObject;
use eipscanner::{MessageRouter, SessionInfo};

/// Reads and prints a device's Identity object.
#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    device: common::Device,
}

fn main() -> eipscanner::Result<()> {
    let cli = Cli::parse();
    common::init_logging();
    let session = SessionInfo::connect(&cli.device.host, cli.device.port)?;

    let id = IdentityObject::read(&session, &MessageRouter::new(), 1)?;
    println!("vendor ID:      {}", id.vendor_id);
    println!("device type:    {}", id.device_type);
    println!("product code:   {}", id.product_code);
    println!("revision:       {}", id.revision);
    println!("status:         {:#06x}", id.status);
    println!("serial number:  {:#010x}", id.serial_number);
    println!("product name:   {}", id.product_name);
    Ok(())
}
