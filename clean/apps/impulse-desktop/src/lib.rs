//! `impulse-desktop` — Dioxus web + desktop host for the Impulse-RS orchestrator.
//!
//! The shell is a thin presentation layer over the runtime crate: it subscribes
//! to the orchestrator's event stream and renders the four canonical views
//! from the Rust contract:
//!
//! - **Terminal** — the live PTY pane (the centerpiece)
//! - **Workspaces** — registered project roots
//! - **Sessions** — active and historical coding-agent sessions
//! - **Health** — orchestrator + MCP status
//!
//! Compile with `--features web` for the wasm target, `--features desktop`
//! for the native desktop target, or `--features ssr` for server-side rendering.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

pub mod components;
pub mod state;
pub mod theme;

pub use components::{HealthView, SessionsView, TerminalView, WorkspacesView};
pub use state::{AppState, ViewKind};
