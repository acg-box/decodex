use std::{fmt::Display, path::Path};

use clap::ValueEnum;
use serde::Deserialize;
use serde_json::Value;

use crate::mcp::RESOURCE_NOT_FOUND_CODE;

/// MCP transport supported by the native Decodex gateway.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum McpTransport {
	/// JSON-RPC messages over stdin/stdout.
	Stdio,
	/// MCP Streamable HTTP endpoint for remote-capable clients.
	StreamableHttp,
}
impl McpTransport {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::Stdio => "stdio",
			Self::StreamableHttp => "streamable-http",
		}
	}

	pub(crate) fn default_capability_profile(self) -> McpCapabilityProfile {
		match self {
			Self::Stdio => McpCapabilityProfile::Admin,
			Self::StreamableHttp => McpCapabilityProfile::Observe,
		}
	}
}

/// Capability profile exposed by the Decodex MCP gateway.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum McpCapabilityProfile {
	/// Public-safe local observability only.
	Observe,
	/// Observe plus planning and workflow prompt helpers.
	Plan,
	/// Observe, plan, and guarded lane-control operations.
	Operate,
	/// Full local operator profile for supported Decodex MCP tools.
	Admin,
}
impl McpCapabilityProfile {
	pub(super) const ALL: [Self; 4] = [Self::Observe, Self::Plan, Self::Operate, Self::Admin];

	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::Observe => "observe",
			Self::Plan => "plan",
			Self::Operate => "operate",
			Self::Admin => "admin",
		}
	}

	pub(super) fn allows(self, required: Self) -> bool {
		required <= self
	}
}

/// Request to start the native Decodex MCP gateway.
#[derive(Clone, Copy, Debug)]
pub(crate) struct McpServeRequest<'a> {
	pub(crate) transport: McpTransport,
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) capability_profile: McpCapabilityProfile,
	pub(crate) listen_address: &'a str,
	pub(crate) allowed_origins: &'a [String],
	pub(crate) bearer_token_env: Option<&'a str>,
}

#[derive(Deserialize)]
pub(super) struct ReadResourceParams {
	pub(super) uri: String,
}

pub(super) struct McpTool {
	pub(super) required_profile: McpCapabilityProfile,
	pub(super) value: Value,
}

#[derive(Debug)]
pub(super) struct McpError {
	pub(super) code: i64,
	pub(super) message: String,
}
impl McpError {
	pub(super) fn invalid_params() -> Self {
		Self { code: -32_602, message: String::from("Invalid params") }
	}

	pub(super) fn method_not_found() -> Self {
		Self { code: -32_601, message: String::from("Method not found") }
	}

	pub(super) fn resource_not_found() -> Self {
		Self { code: RESOURCE_NOT_FOUND_CODE, message: String::from("Resource not found") }
	}

	pub(super) fn internal(error: impl Display) -> Self {
		tracing::warn!(error = %error, "MCP resource read failed.");

		Self { code: -32_603, message: String::from("Internal error") }
	}
}
