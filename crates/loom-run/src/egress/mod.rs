//! The run's way out (spec §5.3 steps 4 and 6, D4, R-b, R-c): one Squid per run, the only
//! thing on the run's internal network that also reaches the outside. It allows the public web
//! and blocks every private range, by the resolved address; every destination it sees becomes
//! an [`Egress`](loom_core::run::RunEvent::Egress) event.
//!
//! The config text and the log line reader are plain code, built and tested everywhere; the
//! proxy's lifecycle is Podman's, Linux only (D2).

pub mod access_log;
pub mod log_file;
#[cfg(target_os = "linux")]
pub(crate) mod proxy;
pub mod squid_conf;

pub use access_log::parse_access_line;
pub use squid_conf::squid_conf;
