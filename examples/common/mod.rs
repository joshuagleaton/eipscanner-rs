//! Setup shared by the examples.

use clap::Args;
use eipscanner::EIP_DEFAULT_EXPLICIT_PORT;

/// Address of the device to connect to.
#[derive(Debug, Args)]
pub struct Device {
    /// Device IP address or host name
    pub host: String,
    /// TCP port for explicit messaging
    #[arg(long, default_value_t = EIP_DEFAULT_EXPLICIT_PORT)]
    pub port: u16,
}

/// Logs to stderr; level from `RUST_LOG`, default `info`.
pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
}
