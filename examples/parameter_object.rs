//! Reads Parameter objects: the number of parameters from the class, then each
//! parameter with full attributes.

mod common;

use bytes::Buf;
use clap::Parser;
use eipscanner::cip::{EPath, ServiceCode};
use eipscanner::objects::ParameterObject;
use eipscanner::{MessageRouter, SessionInfo};

/// Reads the number of parameters (class attribute 2) and prints each one.
#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    device: common::Device,
    /// Read at most this many parameters
    #[arg(long, default_value_t = 10)]
    max: u16,
}

fn main() -> eipscanner::Result<()> {
    let cli = Cli::parse();
    common::init_logging();
    let session = SessionInfo::connect(&cli.device.host, cli.device.port)?;
    let router = MessageRouter::new();

    let reply = router
        .send_request(
            &session,
            ServiceCode::GET_ATTRIBUTE_SINGLE,
            &EPath::attribute(ParameterObject::CLASS_ID, 0, 2),
            &[],
        )?
        .check("read number of parameters")?;
    let count = (&reply.data[..]).get_u16_le();
    println!("device has {count} parameters");

    for id in 1..=count.min(cli.max) {
        let p = ParameterObject::read(&session, &router, id, true)?;
        let value = match p.raw_value().len() {
            1 => p.eng_value::<u8>(),
            2 => p.eng_value::<u16>(),
            4 => p.eng_value::<u32>(),
            _ => Ok(f64::NAN),
        }?;
        println!(
            "#{id:<4} {:<24} {value} {} ({})",
            p.name, p.units, p.data_type
        );
    }
    Ok(())
}
