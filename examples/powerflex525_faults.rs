//! Reads the fault queue of a PowerFlex 525 drive.

mod common;

use clap::Parser;
use eipscanner::vendor::ra::powerflex525::DpiFaultManager;
use eipscanner::{MessageRouter, SessionInfo};

/// Reads the fault queue of a PowerFlex 525 drive.
#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    device: common::Device,
    /// Also read bus voltage, current, and frequency per fault
    #[arg(long)]
    details: bool,
    /// Clear the fault queue after reading it
    #[arg(long)]
    clear: bool,
}

fn main() -> eipscanner::Result<()> {
    let cli = Cli::parse();
    common::init_logging();
    let session = SessionInfo::connect(&cli.device.host, cli.device.port)?;

    let mut manager = DpiFaultManager::new(cli.clear, cli.details);
    let faults = manager.handle_fault_parameters(&session, &MessageRouter::new())?;
    if faults.is_empty() {
        println!("no faults");
    }
    for fault in faults {
        let d = &fault.fault_details;
        let text = fault
            .fault_description
            .map_or("unknown fault code", |f| f.text);
        println!(
            "#{} code {} ({text}): {:.0} V, {:.2} A, {:.2} Hz",
            d.fault_number, d.fault_code, d.bus_voltage, d.current, d.frequency
        );
    }
    Ok(())
}
