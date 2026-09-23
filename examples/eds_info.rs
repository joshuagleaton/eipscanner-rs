//! Prints the device and the I/O connections described by an EDS file.

use std::path::PathBuf;

use clap::Parser;
use eipscanner::eds::{Eds, EdsDirection};

/// Prints the device and the I/O connections described by an EDS file.
#[derive(Parser)]
struct Cli {
    /// EDS file
    path: PathBuf,
}

fn main() -> eipscanner::Result<()> {
    let cli = Cli::parse();
    let eds = Eds::read(&cli.path)?;

    let d = eds.device()?;
    println!(
        "{} ({}), catalog {}",
        d.product_name, d.vendor_name, d.catalog
    );
    println!(
        "  vendor {} product type {} product code {} revision {}.{}",
        d.vendor_id, d.product_type, d.product_code, d.major_revision, d.minor_revision
    );

    for c in eds.connections()? {
        println!("\nConnection{} \"{}\"", c.number, c.name);
        println!("  path  {}", c.path);
        println!("  O->T  {}", direction(&c.o2t));
        println!("  T->O  {}", direction(&c.t2o));
        match c.to_connection_config() {
            Ok(config) => println!("  {config:?}"),
            Err(e) => println!("  cannot open: {e}"),
        }
    }
    Ok(())
}

fn direction(d: &EdsDirection) -> String {
    let assembly = d
        .assembly
        .map(|n| format!(" (Assem{n})"))
        .unwrap_or_default();
    let rpi = match d.rpi {
        Some(r) => format!(
            ", RPI default {} min {} max {} µs",
            opt(r.default),
            opt(r.min),
            opt(r.max)
        ),
        None => String::new(),
    };
    format!("{} bytes{assembly}{rpi}", d.size)
}

fn opt(v: Option<u32>) -> String {
    v.map_or("-".into(), |v| v.to_string())
}
