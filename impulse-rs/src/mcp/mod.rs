//! MCP server integration — exposes Impulse tools via Model Context Protocol.
//!
//! Wraps the dynamic [`ToolRegistry`](crate::tooling::ToolRegistry) in a
//! JSON-line MCP server that external coding agents can connect to over
//! stdio or TCP. Translates MCP `tools/call` requests into registry lookups,
//! capability checks, and tool execution.

pub mod server;

pub use server::McpServer;
