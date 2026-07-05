//! Process-level smoke tests for the Decodex MCP stdio gateway.

#![allow(unused_crate_dependencies)]

mod mcp_stdio {
	pub(crate) mod http;
	pub(crate) mod process;
	pub(crate) mod project;
	pub(crate) mod support;
}
