//! IPC client for communicating with the Impulse daemon over Unix socket.

mod client;
mod types;

pub use client::DaemonClient;
pub use types::*;
