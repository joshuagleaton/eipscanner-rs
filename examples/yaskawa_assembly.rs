//! Writes and reads back an assembly on a Yaskawa MP3300iec, which needs 8-bit
//! path segments.

mod common;

use clap::Parser;
use eipscanner::SessionInfo;
use eipscanner::cip::{EPath, ServiceCode};
use eipscanner::vendor::yaskawa::mp3300iec::{ASSEMBLY_OBJECT, message_router};

/// Writes 0xDE to every byte of an assembly on a Yaskawa MP3300iec, then reads it back.
#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    device: common::Device,
    /// Assembly instance
    #[arg(long, default_value_t = 101)]
    instance: u16,
    /// Assembly size in bytes
    #[arg(long, default_value_t = 32)]
    size: usize,
}

fn main() -> eipscanner::Result<()> {
    let cli = Cli::parse();
    common::init_logging();
    let session = SessionInfo::connect(&cli.device.host, cli.device.port)?;
    let router = message_router();
    let path = EPath::attribute(ASSEMBLY_OBJECT, cli.instance, 3);

    router
        .send_request(
            &session,
            ServiceCode::SET_ATTRIBUTE_SINGLE,
            &path,
            &vec![0xDE; cli.size],
        )?
        .check("write assembly")?;
    let reply = router
        .send_request(&session, ServiceCode::GET_ATTRIBUTE_SINGLE, &path, &[])?
        .check("read assembly")?;
    println!("assembly {}: {:02x?}", cli.instance, reply.data);
    Ok(())
}
