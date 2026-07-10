mod autonomy_resources;
mod context;
mod control;
mod http;
mod observability;
mod planning;
mod prompts;
mod resources;
mod server;
mod tool_schemas;
mod tools;
mod types;

pub(crate) use self::types::{McpCapabilityProfile, McpServeRequest, McpTransport};

use std::{io, net::TcpListener, str};

use serde_json::{self, Value};

#[cfg(test)] use self::http::{McpHttpHandler, McpHttpSessions, http_header_end};
#[cfg(test)]
use self::observability::{
	mcp_activity_tail_resource, mcp_pr_review_state_resource, mcp_public_post_review_lane,
	mcp_run_activity_summary, mcp_run_resource, mcp_status_live_resource,
	sanitize_mcp_observability_value,
};
#[cfg(test)] use self::resources::ResourceContent;
use self::{
	context::McpContext,
	http::McpHttpAuthorization,
	server::{McpServer, json_rpc_error},
	types::{McpError, McpTool, ReadResourceParams},
};
use crate::prelude::{Result, eyre};

/// Safe default listen address for Streamable HTTP MCP.
pub(crate) const DEFAULT_MCP_HTTP_LISTEN_ADDRESS: &str = "127.0.0.1:8193";

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const SERVER_NAME: &str = "decodex";
const RESOURCE_NOT_FOUND_CODE: i64 = -32_002;
const DEFAULT_MCP_STATUS_LIMIT: usize = 10;
const TOOL_OBSERVE: &str = "decodex_observe";
const TOOL_PLAN: &str = "decodex_plan";
const TOOL_INTAKE_GOAL: &str = "intake_goal";
const TOOL_AUTONOMY_DRAFT_OBJECTIVE: &str = "autonomy_draft_objective";
const TOOL_AUTONOMY_ACCEPT_OBJECTIVE: &str = "autonomy_accept_objective";
const TOOL_AUTONOMY_SUBMIT_SIGNAL: &str = "autonomy_submit_signal";
const TOOL_AUTONOMY_COMPILE_PROPOSAL: &str = "autonomy_compile_proposal";
const TOOL_AUTONOMY_CHALLENGE_PROPOSAL: &str = "autonomy_challenge_proposal";
const TOOL_AUTONOMY_REQUEST_PROMOTION: &str = "autonomy_request_promotion";
const TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY: &str = "autonomy_accept_runtime_policy";
const TOOL_AUTONOMY_APPLY_RUNTIME_POLICY: &str = "autonomy_apply_runtime_policy";
const TOOL_LANE_CONTROL: &str = "decodex_lane_control";
const TOOL_PROJECT_CONTROL: &str = "decodex_project_control";
const MCP_HTTP_ENDPOINT_PATH: &str = "/mcp";
const MCP_SESSION_HEADER: &str = "Mcp-Session-Id";

/// Start the Decodex MCP gateway.
pub(crate) fn serve(request: McpServeRequest<'_>) -> Result<()> {
	match request.transport {
		McpTransport::Stdio => {
			let context = McpContext::for_process(request.config_path)?;
			let stdin = io::stdin();
			let stdout = io::stdout();

			self::server::serve_stdio_with_profile(
				stdin.lock(),
				stdout.lock(),
				context,
				request.capability_profile,
			)
		},
		McpTransport::StreamableHttp => {
			let authorization = McpHttpAuthorization::from_env_var_name(request.bearer_token_env)?;

			self::http::validate_mcp_http_listen_address(
				request.listen_address,
				request.allowed_origins,
				&authorization,
			)?;
			self::http::validate_mcp_http_capability_profile(
				request.capability_profile,
				&authorization,
			)?;

			let context = McpContext::for_process(request.config_path)?;
			let listener = TcpListener::bind(request.listen_address).map_err(|error| {
				eyre::eyre!(
					"Failed to bind Decodex MCP Streamable HTTP endpoint at {}: {error}",
					request.listen_address
				)
			})?;

			self::http::serve_streamable_http_with_profile(
				listener,
				context,
				request.capability_profile,
				request.allowed_origins.to_vec(),
				authorization,
			)
		},
	}
}

fn tool_success(value: Value) -> Value {
	tool_result(value, false)
}

fn tool_refusal(reason: &str, message: impl Into<String>) -> Value {
	tool_refusal_value(serde_json::json!({
		"schema": "decodex.mcp.refusal/1",
		"status": "refused",
		"reason": reason,
		"message": message.into()
	}))
}

fn invalid_tool_arguments(tool: &str, message: &str) -> Value {
	tool_refusal_value(serde_json::json!({
		"schema": "decodex.mcp.tool_validation_error/1",
		"status": "refused",
		"reason": "invalid_arguments",
		"tool": tool,
		"message": message
	}))
}

fn capability_profile_refusal(
	tool: &str,
	capability_profile: McpCapabilityProfile,
	required_profile: McpCapabilityProfile,
) -> Value {
	tool_refusal_value(serde_json::json!({
		"schema": "decodex.mcp.refusal/1",
		"status": "refused",
		"reason": "insufficient_capability_profile",
		"tool": tool,
		"capability_profile": capability_profile.as_str(),
		"required_capability_profile": required_profile.as_str(),
		"message": "The active Decodex MCP capability profile does not expose this tool."
	}))
}

fn tool_refusal_value(value: Value) -> Value {
	tool_result(value, true)
}

fn tool_result(value: Value, is_error: bool) -> Value {
	let text = serde_json::to_string_pretty(&value)
		.unwrap_or_else(|_| String::from("{\"status\":\"refused\"}"));

	serde_json::json!({
		"content": [
			{
				"type": "text",
				"text": text
			}
		],
		"structuredContent": value,
		"isError": is_error
	})
}

fn tool_call_result_allows_progress(result: &Value) -> bool {
	result.get("isError").and_then(Value::as_bool) == Some(false)
}

fn non_empty_string(value: Option<&str>) -> Option<&str> {
	value.map(str::trim).filter(|value| !value.is_empty())
}

fn safe_runtime_identifier(value: &str) -> bool {
	!value.is_empty()
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
		&& !value.contains("..")
}

fn safe_autonomy_record_identifier(value: &str) -> bool {
	!value.is_empty()
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
		&& !value.contains("..")
}

#[cfg(test)] mod tests;
