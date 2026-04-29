//! `windbgr-mcp` — MCP server exposing Windows process discovery and cdb
//! debugging over stdio and Streamable HTTP.

pub mod app;
pub mod audit;
pub mod cdb;
pub mod config;
pub mod error;
pub mod mcp;
pub mod platform;
pub mod privilege;
pub mod process;
pub mod security;

pub use error::{Error, Result};
