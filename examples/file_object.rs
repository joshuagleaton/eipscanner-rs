//! Uploads a file from a File object, e.g. an EDS file stored on the device.

mod common;

use std::path::PathBuf;

use clap::Parser;
use eipscanner::objects::FileObject;
use eipscanner::{MessageRouter, SessionInfo};

/// Uploads a file from a device's File object.
#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    device: common::Device,
    /// File object instance (0xC8 is the EDS file)
    #[arg(long, default_value_t = 0xC8)]
    instance: u16,
    /// Where to write the file [default: only print its size]
    #[arg(long)]
    out: Option<PathBuf>,
}

fn main() -> eipscanner::Result<()> {
    let cli = Cli::parse();
    common::init_logging();
    let session = SessionInfo::connect(&cli.device.host, cli.device.port)?;

    let mut file = FileObject::new(&session, MessageRouter::new(), cli.instance)?;
    let content = file.upload(&session)?;
    match cli.out {
        Some(path) => {
            std::fs::write(&path, &content)?;
            println!("wrote {} bytes to {}", content.len(), path.display());
        }
        None => println!("uploaded {} bytes", content.len()),
    }
    Ok(())
}
