use std::{
	env,
	fmt::Display,
	io::{self, BufRead as _, BufReader, ErrorKind, Read, Write},
	net::TcpListener,
	path::{Path, PathBuf},
	str,
};

use crate::{config::ServiceConfig, orchestrator, prelude::eyre, runtime, state::StateStore};
use clap::ValueEnum;
use serde::Deserialize;
use serde_json::{self, Value};

mod control;
mod http;
mod planning;
mod prompts;
mod resources;
mod tools;

#[cfg(test)] use self::http::{McpHttpHandler, McpHttpSessions, http_header_end};
#[cfg(test)]
use self::resources::{
	ResourceContent, mcp_activity_tail_resource, mcp_pr_review_state_resource,
	mcp_public_post_review_lane, mcp_run_activity_summary, mcp_run_resource,
};
use self::{
	http::{
		McpHttpAuthorization, serve_streamable_http_with_profile,
		validate_mcp_http_capability_profile, validate_mcp_http_listen_address,
	},
	resources::{
		mcp_public_lane_inspect_resource, mcp_status_live_resource,
		sanitize_mcp_observability_value,
	},
};

/// Safe default listen address for Streamable HTTP MCP.
pub(crate) const DEFAULT_MCP_HTTP_LISTEN_ADDRESS: &str = "127.0.0.1:8193";

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const SERVER_NAME: &str = "decodex";
const RESOURCE_NOT_FOUND_CODE: i64 = -32_002;
const DEFAULT_MCP_STATUS_LIMIT: usize = 10;
const TOOL_OBSERVE: &str = "decodex_observe";
const TOOL_PLAN: &str = "decodex_plan";
const TOOL_RESEARCH_COMPILE: &str = "research_compile";
const TOOL_RESEARCH_PROMOTE: &str = "research_promote";
const TOOL_INTAKE_GOAL: &str = "intake_goal";
const TOOL_AUTONOMY_DRAFT_OBJECTIVE: &str = "autonomy_draft_objective";
const TOOL_AUTONOMY_ACCEPT_OBJECTIVE: &str = "autonomy_accept_objective";
const TOOL_AUTONOMY_SUBMIT_SIGNAL: &str = "autonomy_submit_signal";
const TOOL_AUTONOMY_COMPILE_PROPOSAL: &str = "autonomy_compile_proposal";
const TOOL_AUTONOMY_CHALLENGE_PROPOSAL: &str = "autonomy_challenge_proposal";
const TOOL_AUTONOMY_REQUEST_PROMOTION: &str = "autonomy_request_promotion";
const TOOL_LANE_CONTROL: &str = "decodex_lane_control";
const TOOL_PROJECT_CONTROL: &str = "decodex_project_control";
const MCP_HTTP_ENDPOINT_PATH: &str = "/mcp";
const MCP_SESSION_HEADER: &str = "Mcp-Session-Id";

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
	fn as_str(self) -> &'static str {
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
	const ALL: [Self; 4] = [Self::Observe, Self::Plan, Self::Operate, Self::Admin];

	fn as_str(self) -> &'static str {
		match self {
			Self::Observe => "observe",
			Self::Plan => "plan",
			Self::Operate => "operate",
			Self::Admin => "admin",
		}
	}

	fn allows(self, required: Self) -> bool {
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

struct McpServer {
	context: McpContext,
	capability_profile: McpCapabilityProfile,
	transport: McpTransport,
}
impl McpServer {
	fn handle_line(&self, line: &str, emit_progress: bool) -> Vec<Value> {
		let parsed = serde_json::from_str::<Value>(line);
		let value = match parsed {
			Ok(value) => value,
			Err(_) => return vec![json_rpc_error(Value::Null, -32_700, "Parse error")],
		};
		let request = match serde_json::from_value::<JsonRpcRequest>(value) {
			Ok(request) => request,
			Err(_) => return vec![json_rpc_error(Value::Null, -32_600, "Invalid Request")],
		};

		self.handle_request(request, emit_progress)
	}

	fn handle_request(&self, request: JsonRpcRequest, emit_progress: bool) -> Vec<Value> {
		let Some(id) = request.id else {
			return Vec::new();
		};

		if request.jsonrpc.as_deref() != Some("2.0") {
			return vec![json_rpc_error(id, -32_600, "Invalid Request")];
		}

		let Some(method) = request.method else {
			return vec![json_rpc_error(id, -32_600, "Invalid Request")];
		};
		let progress_token =
			emit_progress.then(|| progress_token_from_params(request.params.as_ref())).flatten();
		let result = match method.as_str() {
			"initialize" => Ok(self.initialize()),
			"ping" => Ok(serde_json::json!({})),
			"logging/setLevel" => Ok(serde_json::json!({})),
			"resources/list" => self.list_resources(),
			"resources/read" => self.read_resource(request.params),
			"resources/templates/list" => Ok(self.list_resource_templates()),
			"prompts/list" => Ok(self.list_prompts()),
			"prompts/get" => self.get_prompt(request.params),
			"tools/list" => Ok(self.list_tools()),
			"tools/call" => self.call_tool(request.params),
			_ => Err(McpError::method_not_found()),
		};
		let mut responses = Vec::new();

		if method == "tools/call"
			&& result.as_ref().is_ok_and(tool_call_result_allows_progress)
			&& let Some(token) = progress_token
		{
			responses.push(progress_notification(
				token,
				1,
				Some(2),
				"Decodex MCP tool request accepted.",
			));
		}

		responses.push(match result {
			Ok(result) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
			Err(error) => json_rpc_error(id, error.code, &error.message),
		});

		responses
	}

	fn initialize(&self) -> Value {
		serde_json::json!({
			"protocolVersion": MCP_PROTOCOL_VERSION,
			"capabilities": {
				"resources": {},
				"prompts": {},
				"tools": {},
				"logging": {},
				"experimental": {
					"decodex": {
						"capabilityProfile": self.capability_profile.as_str(),
						"capabilityProfiles": McpCapabilityProfile::ALL
							.into_iter()
							.map(McpCapabilityProfile::as_str)
							.collect::<Vec<_>>(),
						"transport": self.transport.as_str(),
						"remoteControl": {
							"safeDefaultProfile": "observe",
							"httpTransport": "streamable-http",
							"httpEndpoint": MCP_HTTP_ENDPOINT_PATH,
							"sessionHeader": MCP_SESSION_HEADER,
							"sseResponses": true,
							"originValidation": true,
							"operateAdminTools": "inspect_first_guarded",
							"mutatingToolsRequireAuthority": true,
							"privateEvidencePayloadsExposed": false
						}
					}
				}
			},
			"serverInfo": {
				"name": SERVER_NAME,
				"version": env!("CARGO_PKG_VERSION")
			}
		})
	}

	fn list_tools(&self) -> Value {
		let tools = tools::mcp_tools()
			.into_iter()
			.filter(|tool| self.capability_profile.allows(tool.required_profile))
			.map(|tool| tool.value)
			.collect::<Vec<_>>();

		serde_json::json!({ "tools": tools })
	}

	fn call_tool(&self, params: Option<Value>) -> crate::prelude::Result<Value, McpError> {
		let params = params.ok_or_else(McpError::invalid_params)?;
		let params = serde_json::from_value::<CallToolParams>(params)
			.map_err(|_| McpError::invalid_params())?;
		let arguments = params.arguments.unwrap_or_else(|| serde_json::json!({}));
		let Some(required_profile) = tools::tool_required_profile(&params.name) else {
			return Ok(tool_refusal(
				"unknown_tool",
				format!("Decodex MCP tool `{}` is not registered.", params.name),
			));
		};

		if !self.capability_profile.allows(required_profile) {
			return Ok(capability_profile_refusal(
				&params.name,
				self.capability_profile,
				required_profile,
			));
		}

		match params.name.as_str() {
			TOOL_OBSERVE => Ok(self.call_observe_tool(arguments)),
			TOOL_PLAN => Ok(planning::call_plan_tool(arguments)),
			TOOL_RESEARCH_COMPILE => Ok(self.call_research_compile_tool(arguments)),
			TOOL_RESEARCH_PROMOTE => Ok(self.call_research_promote_tool(arguments)),
			TOOL_INTAKE_GOAL => Ok(self.call_intake_goal_tool(arguments)),
			TOOL_AUTONOMY_DRAFT_OBJECTIVE => Ok(self.call_autonomy_draft_objective_tool(arguments)),
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE =>
				Ok(self.call_autonomy_accept_objective_tool(arguments)),
			TOOL_AUTONOMY_SUBMIT_SIGNAL => Ok(self.call_autonomy_submit_signal_tool(arguments)),
			TOOL_AUTONOMY_COMPILE_PROPOSAL =>
				Ok(self.call_autonomy_compile_proposal_tool(arguments)),
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL =>
				Ok(self.call_autonomy_challenge_proposal_tool(arguments)),
			TOOL_AUTONOMY_REQUEST_PROMOTION =>
				Ok(self.call_autonomy_request_promotion_tool(arguments)),
			TOOL_LANE_CONTROL => Ok(self.call_lane_control_tool(arguments, required_profile)),
			TOOL_PROJECT_CONTROL => Ok(self.call_project_control_tool(arguments, required_profile)),
			_ => Ok(tool_refusal("unknown_tool", "Decodex MCP tool is not registered.")),
		}
	}

	fn call_observe_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<ObserveToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_OBSERVE,
					"`issue`, `runId`, and `limit` are the only supported observe arguments.",
				);
			},
		};
		let limit = params.limit.unwrap_or(DEFAULT_MCP_STATUS_LIMIT);

		if limit == 0 {
			return tool_refusal("invalid_limit", "`limit` must be greater than zero.");
		}

		let observability_result = if params.issue.as_deref().is_some() {
			orchestrator::build_mcp_lane_control_resource(
				self.context.config_path.as_deref(),
				params.issue.as_deref(),
				params.run_id.as_deref(),
				limit,
			)
			.map(mcp_public_lane_inspect_resource)
		} else {
			orchestrator::build_mcp_status_resource(self.context.config_path.as_deref(), limit)
				.map(mcp_status_live_resource)
		};
		let mut value = match observability_result {
			Ok(value) => value,
			Err(_) => {
				return tool_refusal(
					"observability_unavailable",
					"Decodex observability requires a registered project config or --config.",
				);
			},
		};

		sanitize_mcp_observability_value(&mut value);

		tool_success(serde_json::json!({
			"schema": "decodex.mcp.observe_result/1",
			"status": "ok",
			"capability_profile": "observe",
			"observability": value
		}))
	}
}

struct McpContext {
	repo_root: PathBuf,
	config_path: Option<PathBuf>,
	project_id: Option<String>,
	state_store: Option<StateStore>,
}
impl McpContext {
	fn for_process(config_path: Option<&Path>) -> crate::prelude::Result<Self> {
		let state_store = runtime::open_runtime_store_lazy().ok();
		let config_path = resolve_context_config_path(config_path, state_store.as_ref())?;
		let config = config_path.as_ref().map(ServiceConfig::from_path).transpose()?;
		let repo_root = config
			.as_ref()
			.map(|config| config.repo_root().to_path_buf())
			.or_else(|| discover_repo_root_from_current_dir().ok().flatten())
			.ok_or_else(|| {
				eyre::eyre!(
					"Failed to find the Decodex repository root for MCP docs resources; start from a checkout or pass --config."
				)
		})?;
		let project_id = config.map(|config| config.service_id().to_owned());

		Ok(Self { repo_root, config_path, project_id, state_store })
	}

	fn project_id(&self) -> Option<&str> {
		self.project_id.as_deref()
	}
}

#[derive(Deserialize)]
struct JsonRpcRequest {
	jsonrpc: Option<String>,
	id: Option<Value>,
	method: Option<String>,
	params: Option<Value>,
}

#[derive(Deserialize)]
struct ReadResourceParams {
	uri: String,
}

#[derive(Deserialize)]
struct CallToolParams {
	name: String,
	arguments: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObserveToolArgs {
	issue: Option<String>,
	run_id: Option<String>,
	limit: Option<usize>,
}

struct McpTool {
	required_profile: McpCapabilityProfile,
	value: Value,
}

#[derive(Debug)]
struct McpError {
	code: i64,
	message: String,
}
impl McpError {
	fn invalid_params() -> Self {
		Self { code: -32_602, message: String::from("Invalid params") }
	}

	fn method_not_found() -> Self {
		Self { code: -32_601, message: String::from("Method not found") }
	}

	fn resource_not_found() -> Self {
		Self { code: RESOURCE_NOT_FOUND_CODE, message: String::from("Resource not found") }
	}

	fn internal(error: impl Display) -> Self {
		tracing::warn!(error = %error, "MCP resource read failed.");

		Self { code: -32_603, message: String::from("Internal error") }
	}
}

/// Start the Decodex MCP gateway.
pub(crate) fn serve(request: McpServeRequest<'_>) -> crate::prelude::Result<()> {
	match request.transport {
		McpTransport::Stdio => {
			let context = McpContext::for_process(request.config_path)?;
			let stdin = io::stdin();
			let stdout = io::stdout();

			serve_stdio_with_profile(
				stdin.lock(),
				stdout.lock(),
				context,
				request.capability_profile,
			)
		},
		McpTransport::StreamableHttp => {
			let authorization = McpHttpAuthorization::from_env_var_name(request.bearer_token_env)?;

			validate_mcp_http_listen_address(
				request.listen_address,
				request.allowed_origins,
				&authorization,
			)?;
			validate_mcp_http_capability_profile(request.capability_profile, &authorization)?;

			let context = McpContext::for_process(request.config_path)?;
			let listener = TcpListener::bind(request.listen_address).map_err(|error| {
				eyre::eyre!(
					"Failed to bind Decodex MCP Streamable HTTP endpoint at {}: {error}",
					request.listen_address
				)
			})?;

			serve_streamable_http_with_profile(
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

fn serve_stdio_with_profile<R, W>(
	reader: R,
	mut writer: W,
	context: McpContext,
	capability_profile: McpCapabilityProfile,
) -> crate::prelude::Result<()>
where
	R: Read,
	W: Write,
{
	let server = McpServer { context, capability_profile, transport: McpTransport::Stdio };
	let reader = BufReader::new(reader);

	for line in reader.lines() {
		let line = line?;

		if line.trim().is_empty() {
			continue;
		}

		for response in server.handle_line(&line, true) {
			write_json_line(&mut writer, &response)?;
		}
	}

	Ok(())
}

fn write_json_line<W>(writer: &mut W, value: &Value) -> crate::prelude::Result<()>
where
	W: Write,
{
	let line = serde_json::to_string(value)?;

	match writer
		.write_all(line.as_bytes())
		.and_then(|()| writer.write_all(b"\n"))
		.and_then(|()| writer.flush())
	{
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
		Err(error) => Err(error.into()),
	}
}

fn resolve_context_config_path(
	explicit_path: Option<&Path>,
	state_store: Option<&StateStore>,
) -> crate::prelude::Result<Option<PathBuf>> {
	if let Some(path) = explicit_path {
		return Ok(Some(path.to_path_buf()));
	}

	let Some(state_store) = state_store else {
		return Ok(None);
	};

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)
}

fn discover_repo_root_from_current_dir() -> crate::prelude::Result<Option<PathBuf>> {
	let mut candidate = env::current_dir()?;

	loop {
		if candidate.join("docs/index.md").is_file() && candidate.join("Cargo.toml").is_file() {
			return Ok(Some(candidate));
		}
		if !candidate.pop() {
			return Ok(None);
		}
	}
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
	serde_json::json!({
		"jsonrpc": "2.0",
		"id": id,
		"error": {
			"code": code,
			"message": message
		}
	})
}

fn progress_token_from_params(params: Option<&Value>) -> Option<Value> {
	let token = params?.get("_meta")?.get("progressToken")?;

	if token.is_string() || token.is_i64() || token.is_u64() {
		return Some(token.clone());
	}

	None
}

fn progress_notification(
	progress_token: Value,
	progress: u64,
	total: Option<u64>,
	message: &str,
) -> Value {
	let mut params = serde_json::json!({
		"progressToken": progress_token,
		"progress": progress,
		"message": message
	});

	if let Some(total) = total {
		params["total"] = serde_json::json!(total);
	}

	serde_json::json!({
		"jsonrpc": "2.0",
		"method": "notifications/progress",
		"params": params
	})
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
