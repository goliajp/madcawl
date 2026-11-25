//! `madcawl-mcp`: an MCP server over stdio for an agent's command-line
//! program: reading pages, acting on them by reference and waiting for
//! them to settle. Each tool call becomes an agent protocol frame sent to a
//! madcawl host (marspot, or `madcawl-shell --serve`) over its Unix
//! socket; the host's reply, page content already in its envelope,
//! goes back as the tool's text. No dependencies: it reads and writes
//! its own JSON and frames.

mod act;
pub mod client;
mod extract;
pub mod frame;
pub mod json;
mod json_number;
pub mod server;
pub mod tools;
