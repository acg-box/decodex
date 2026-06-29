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

#[cfg(test)]
mod tests {
	use std::{fs, io::Cursor, path::Path, process, str};

	use serde_json::Value;
	use tempfile::TempDir;

	use crate::{
		autonomy_objective::{
			AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
		},
		autonomy_proposal::AutonomyProposalCompileInput,
		autonomy_signal::{
			AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
			AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
			AutonomySignalSourceType,
		},
		loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
		mcp::{
			self, DEFAULT_MCP_HTTP_LISTEN_ADDRESS, McpCapabilityProfile, McpContext,
			McpHttpAuthorization, McpHttpHandler, McpHttpSessions, McpServer, McpTransport,
			ResourceContent,
		},
		runtime,
		state::{self, ProtocolActivityEventSummary, ProtocolActivitySummary, StateStore},
		test_support::TestEnvVarGuard,
	};

	struct ParsedHttpResponse {
		status: String,
		headers: Vec<(String, String)>,
		body: Vec<u8>,
	}

	impl ParsedHttpResponse {
		fn parse(response: &[u8]) -> Self {
			let header_end =
				mcp::http_header_end(response).expect("response should include headers");
			let headers = str::from_utf8(&response[..header_end]).expect("headers should be utf-8");
			let mut lines = headers.split("\r\n");
			let status = lines.next().expect("status line should exist").to_owned();
			let headers = lines
				.filter_map(|line| {
					let (name, value) = line.split_once(':')?;

					Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
				})
				.collect();

			Self { status, headers, body: response[(header_end + 4)..].to_vec() }
		}

		fn header(&self, name: &str) -> Option<&str> {
			self.headers
				.iter()
				.find(|(header, _)| header == &name.to_ascii_lowercase())
				.map(|(_, value)| value.as_str())
		}

		fn json_body(&self) -> Value {
			serde_json::from_slice(&self.body).expect("HTTP body should be JSON")
		}

		fn body_text(&self) -> &str {
			str::from_utf8(&self.body).expect("HTTP body should be utf-8")
		}
	}

	#[test]
	fn initialize_exposes_protocol_primitive_capabilities() {
		let repo = test_repo();
		let responses =
			run_stdio(repo.path(), r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
		let response = response_at(&responses, 0);
		let result = response.get("result").and_then(Value::as_object).expect("result object");
		let capabilities =
			result.get("capabilities").and_then(Value::as_object).expect("capabilities object");

		assert!(capabilities.contains_key("resources"));
		assert!(capabilities.contains_key("prompts"));
		assert!(capabilities.contains_key("tools"));
		assert!(capabilities.contains_key("logging"));
		assert_eq!(capabilities["experimental"]["decodex"]["capabilityProfile"], "admin");
	}

	#[test]
	fn logging_set_level_is_stdio_compatible() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"logging/setLevel","params":{"level":"debug"}}"#,
		);
		let result = response_at(&responses, 0)["result"].as_object().expect("result object");

		assert!(result.is_empty());
	}

	#[test]
	fn resources_list_includes_docs_decisions_and_research_concepts() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
		);
		let resources =
			response_at(&responses, 0)["result"]["resources"].as_array().expect("resources array");
		let uris = resources
			.iter()
			.filter_map(|resource| resource.get("uri").and_then(Value::as_str))
			.collect::<Vec<_>>();

		assert!(uris.contains(&"decodex://docs/index"));
		assert!(uris.contains(&"decodex://docs/spec/runtime"));
		assert!(uris.contains(&"decodex://docs/decisions/mcp-gateway"));
		assert!(uris.contains(&"decodex://research/sample-report"));
	}

	#[test]
	fn resources_list_includes_runtime_decision_contracts() {
		let repo = test_repo();
		let state_store = StateStore::open_in_memory().expect("state store should open");

		state_store
			.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
			.expect("decision contract should persist");

		let responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(state_store),
			},
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
		);
		let resources =
			response_at(&responses, 0)["result"]["resources"].as_array().expect("resources array");
		let uris = resources
			.iter()
			.filter_map(|resource| resource.get("uri").and_then(Value::as_str))
			.collect::<Vec<_>>();

		assert!(uris.contains(&"decodex://decision-contracts/research-x-loop-contract"));
	}

	#[test]
	fn resources_read_returns_runtime_decision_contract() {
		let repo = test_repo();
		let state_store = StateStore::open_in_memory().expect("state store should open");

		state_store
			.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
			.expect("decision contract should persist");

		let responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(state_store),
			},
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://decision-contracts/research-x-loop-contract"}}"#,
		);
		let contents =
			response_at(&responses, 0)["result"]["contents"].as_array().expect("contents array");
		let text = contents[0]["text"].as_str().expect("text content");
		let content: Value = serde_json::from_str(text).expect("decision contract should be json");

		assert_eq!(content["project_id"], "decodex");
		assert_eq!(content["decision_contract"]["contract_id"], "research-x-loop-contract");
		assert!(
			content["decision_contract"]["evidence_boundary"]["private_evidence_refs"].is_null()
		);
		assert!(content["decision_contract"]["links"]["execution_program_node_ids"].is_null());
		assert!(!text.contains("research-x-run"));
	}

	#[test]
	fn resources_read_returns_checked_in_doc_text() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://docs/spec/runtime"}}"#,
		);
		let contents =
			response_at(&responses, 0)["result"]["contents"].as_array().expect("contents array");
		let text = contents[0]["text"].as_str().expect("text content");

		assert_eq!(text, "# Runtime\n\nSpec body.\n");
	}

	#[test]
	fn resources_read_returns_checked_in_research_markdown() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://research/sample-report"}}"#,
		);
		let contents =
			response_at(&responses, 0)["result"]["contents"].as_array().expect("contents array");
		let text = contents[0]["text"].as_str().expect("text content");

		assert_eq!(contents[0]["mimeType"], "text/markdown");
		assert_eq!(text, "# Sample Research\n");
	}

	#[test]
	fn observability_sanitizer_strips_private_operator_fields() {
		let mut value = sensitive_observability_fixture();

		mcp::sanitize_mcp_observability_value(&mut value);

		assert_observability_is_sanitized(&value);
	}

	#[test]
	fn observability_resource_content_strips_private_operator_fields() {
		let content = ResourceContent::mcp_observability_json(
			"decodex://projects/decodex/status",
			sensitive_observability_fixture(),
		)
		.expect("observability content should serialize");
		let value: Value = serde_json::from_str(&content.text).expect("content should be json");

		assert_eq!(content.mime_type, "application/json");

		assert_observability_is_sanitized(&value);
	}

	#[test]
	fn resources_templates_list_exposes_parameterized_resources() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/templates/list","params":{}}"#,
		);
		let templates = response_at(&responses, 0)["result"]["resourceTemplates"]
			.as_array()
			.expect("resource templates array");
		let uri_templates = templates
			.iter()
			.filter_map(|template| template.get("uriTemplate").and_then(Value::as_str))
			.collect::<Vec<_>>();

		assert!(uri_templates.contains(&"decodex://docs/spec/{topic}"));
		assert!(uri_templates.contains(&"decodex://research/{concept}"));
		assert!(uri_templates.contains(&"decodex://projects/{project_id}/lane-control/{issue}"));
		assert!(uri_templates.contains(&"decodex://projects/{project_id}/status_live"));
		assert!(uri_templates.contains(&"decodex://projects/{project_id}/activity_tail"));
		assert!(uri_templates.contains(&"decodex://projects/{project_id}/lane_inspect/{issue}"));
		assert!(uri_templates.contains(&"decodex://projects/{project_id}/runs/{run_id}/events"));
		assert!(
			uri_templates
				.contains(&"decodex://projects/{project_id}/runs/{run_id}/protocol_activity")
		);
		assert!(
			uri_templates
				.contains(&"decodex://projects/{project_id}/runs/{run_id}/child_agent_activity")
		);
		assert!(
			uri_templates
				.contains(&"decodex://projects/{project_id}/runs/{run_id}/progress_diagnostics")
		);
		assert!(uri_templates.contains(&"decodex://projects/{project_id}/pr_review_state"));

		for uri_template in [
			"decodex://projects/{project_id}/runs/{run_id}/events",
			"decodex://projects/{project_id}/runs/{run_id}/protocol_activity",
			"decodex://projects/{project_id}/runs/{run_id}/child_agent_activity",
			"decodex://projects/{project_id}/runs/{run_id}/progress_diagnostics",
		] {
			let template = templates
				.iter()
				.find(|template| {
					template.get("uriTemplate").and_then(Value::as_str) == Some(uri_template)
				})
				.expect("run-scoped resource template should exist");
			let description = template["description"].as_str().expect("description should exist");

			assert!(description.contains("current/recent status snapshot"));
		}
	}

	#[test]
	fn observability_projection_resources_expose_activity_without_private_payloads() {
		let snapshot = observability_snapshot_fixture();
		let live = super::mcp_status_live_resource(snapshot.clone());
		let activity = super::mcp_activity_tail_resource(snapshot.clone());
		let events = super::mcp_run_resource(&snapshot, "run-1", "events")
			.expect("run events should project");
		let protocol = super::mcp_run_resource(&snapshot, "run-1", "protocol_activity")
			.expect("protocol activity should project");
		let child = super::mcp_run_resource(&snapshot, "run-1", "child_agent_activity")
			.expect("child-agent activity should project");
		let progress = super::mcp_run_resource(&snapshot, "run-1", "progress_diagnostics")
			.expect("progress diagnostics should project");
		let review = super::mcp_pr_review_state_resource(snapshot);
		let mut combined = serde_json::json!({
			"live": live,
			"activity": activity,
			"events": events,
			"protocol": protocol,
			"child": child,
			"progress": progress,
			"review": review
		});

		mcp::sanitize_mcp_observability_value(&mut combined);

		assert_eq!(combined["live"]["schema"], "decodex.mcp.status_live/1");
		assert_eq!(combined["live"]["current_lanes"][0]["run_id"], "run-1");
		assert_eq!(combined["live"]["current_lanes"][0]["status"], "running");
		assert_eq!(combined["live"]["current_lanes"][0]["current_operation"], "model_execution");
		assert_eq!(combined["live"]["current_lanes"][0]["event_count"], 6);
		assert_eq!(
			combined["live"]["current_lanes"][0]["lane_control_next_action"],
			"inspect_or_interrupt_orphaned_live_thread"
		);
		assert_eq!(combined["activity"]["activity"][0]["run_id"], "run-1");
		assert_eq!(combined["activity"]["activity"].as_array().expect("activity array").len(), 1);
		assert_eq!(combined["events"]["event_count"], 6);
		assert_eq!(combined["protocol"]["protocol_activity"]["waiting_reason"], "model_execution");
		assert_eq!(
			combined["protocol"]["protocol_activity"]["recent_events"][1]["detail"],
			"redacted_reasoning"
		);
		assert_eq!(
			combined["protocol"]["protocol_activity"]["recent_events"][2]["detail"],
			"redacted_sensitive_detail"
		);
		assert_eq!(
			combined["protocol"]["protocol_activity"]["recent_events"][3]["detail"],
			"redacted_sensitive_detail"
		);
		assert_eq!(
			combined["protocol"]["protocol_activity"]["recent_events"][4]["detail"],
			"redacted_sensitive_detail"
		);
		assert_eq!(
			combined["protocol"]["protocol_activity"]["recent_events"][5]["detail"],
			"redacted_sensitive_detail"
		);
		assert_eq!(
			combined["protocol"]["protocol_activity"]["recent_events"][6]["detail"],
			"redacted_sensitive_detail"
		);
		assert_eq!(combined["child"]["child_agent_activity"]["event_count"], 2);
		assert_eq!(combined["progress"]["progress_diagnostic"], "protocol_only_activity");
		assert_eq!(
			combined["live"]["current_lanes"][0]["phase_acceptance"]["decision"],
			"accepted"
		);
		assert!(
			combined["live"]["current_lanes"][0]["phase_acceptance"]["changed_surfaces"].is_null()
		);
		assert!(combined["live"]["current_lanes"][0]["loop_review"].is_null());
		assert_eq!(combined["review"]["post_review_lanes"][0]["pr_url"], "https://example/pr/1");
		assert!(combined["review"]["post_review_lanes"][0]["branch_name"].is_null());
		assert!(combined["review"]["post_review_lanes"][0]["loop_status"].is_null());
		assert_eq!(
			combined["review"]["current_lane_reviews"].as_array().expect("review array").len(),
			0
		);

		assert_no_sensitive_observability_content(&combined);
	}

	#[test]
	fn pr_review_state_ignores_recent_run_reviews_without_current_lane() {
		let snapshot = serde_json::json!({
			"schema": "decodex.mcp.status_resource/1",
			"project_id": "decodex",
			"current_lanes": [],
			"recent_runs": [
				{
					"run_id": "run-stale",
					"issue_id": "issue-stale",
					"issue_identifier": "XY-995",
					"loop_status": {
						"review": {
							"status": "stale_recent_finding"
						}
					}
				}
			],
			"post_review_lanes": []
		});
		let review = super::mcp_pr_review_state_resource(snapshot);
		let serialized = serde_json::to_string(&review).expect("review should serialize");

		assert_eq!(review["schema"], "decodex.mcp.pr_review_state/1");
		assert_eq!(review["current_lane_reviews"].as_array().expect("review array").len(), 0);
		assert!(!serialized.contains("stale_recent_finding"));
	}

	#[test]
	fn pr_review_state_includes_object_current_lane_review() {
		let snapshot = serde_json::json!({
			"schema": "decodex.mcp.status_resource/1",
			"project_id": "decodex",
			"current_lanes": [
				{
					"run_id": "run-review",
					"issue_id": "issue-review",
					"issue_identifier": "XY-1095",
					"loop_status": {
						"review": observability_review_status_fixture(
							"private-head-sha",
							"fingerprint-private",
							"stop-fingerprint-private",
							3
						)
					}
				}
			],
			"post_review_lanes": []
		});
		let review = super::mcp_pr_review_state_resource(snapshot);
		let current_lane_reviews = review["current_lane_reviews"].as_array().expect("review array");

		assert_eq!(current_lane_reviews.len(), 1);
		assert_eq!(current_lane_reviews[0]["run_id"], "run-review");
		assert_eq!(current_lane_reviews[0]["review"]["status"], "pending");
		assert_eq!(current_lane_reviews[0]["review"]["checkpoint"]["round"], 3);
		assert!(current_lane_reviews[0]["review"]["checkpoint"]["active_fingerprints"].is_null());
	}

	#[test]
	fn mcp_review_surfaces_ignore_null_loop_review_status() {
		let snapshot = serde_json::json!({
			"schema": "decodex.mcp.status_resource/1",
			"project_id": "decodex",
			"current_lanes": [
				{
					"run_id": "run-null-review",
					"issue_id": "issue-null-review",
					"issue_identifier": "XY-1095",
					"loop_status": {
						"review": null
					}
				}
			],
			"post_review_lanes": [
				{
					"project_id": "decodex",
					"issue_id": "issue-null-review",
					"issue_identifier": "XY-1095",
					"loop_status": {
						"review": null
					}
				}
			]
		});
		let review = super::mcp_pr_review_state_resource(snapshot.clone());
		let activity = super::mcp_run_activity_summary(&snapshot["current_lanes"][0]);
		let post_review_lane =
			super::mcp_public_post_review_lane(&snapshot["post_review_lanes"][0]);

		assert_eq!(review["current_lane_reviews"].as_array().expect("review array").len(), 0);
		assert!(activity["loop_review"].is_null());
		assert!(post_review_lane["loop_review"].is_null());
	}

	#[test]
	fn resources_read_exposes_bounded_live_activity_and_recent_run_readback() {
		let repo = test_repo();
		let _runtime_home_guard = isolated_mcp_runtime_home(&repo);
		let config_path = repo.path().join("project.toml");

		seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

		let context = McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: Some(config_path.clone()),
			project_id: Some(String::from("pubfi")),
			state_store: None,
		};
		let responses = run_stdio_with_context(
			context,
			&[
				r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/status_live"}}"#,
				r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/activity_tail"}}"#,
				r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/runs/run-12/events"}}"#,
				r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/runs/run-01/events"}}"#,
				r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/lane_inspect/PUB-012"}}"#,
				r#"{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/lane-control/PUB-012"}}"#,
				r#"{"jsonrpc":"2.0","id":7,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/lane-control"}}"#,
				r#"{"jsonrpc":"2.0","id":8,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/pr_review_state"}}"#,
				r#"{"jsonrpc":"2.0","id":9,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/runs/run-12/protocol_activity"}}"#,
			]
			.join("\n"),
		);
		let status_live = resource_response_json(&responses, 0);
		let activity_tail = resource_response_json(&responses, 1);
		let run_events = resource_response_json(&responses, 2);
		let hidden_run_error = response_error(&responses, 3);
		let lane_inspect = resource_response_json(&responses, 4);
		let lane_control_issue = resource_response_json(&responses, 5);
		let lane_control = resource_response_json(&responses, 6);
		let pr_review_state = resource_response_json(&responses, 7);
		let protocol_activity = resource_response_json(&responses, 8);

		assert_eq!(status_live["schema"], "decodex.mcp.status_live/1");
		assert_eq!(activity_tail["schema"], "decodex.mcp.activity_tail/1");
		assert_eq!(
			activity_tail["activity"].as_array().expect("activity array").len(),
			super::DEFAULT_MCP_STATUS_LIMIT
		);
		assert_eq!(run_events["schema"], "decodex.mcp.run_events/1");
		assert_eq!(run_events["run_id"], "run-12");
		assert_eq!(run_events["event_count"], 6);
		assert_eq!(hidden_run_error["code"], super::RESOURCE_NOT_FOUND_CODE);

		assert_public_lane_inspect_resource(&lane_inspect);
		assert_public_lane_inspect_resource(&lane_control_issue);
		assert_public_lane_control_readback(&lane_control);

		assert_eq!(pr_review_state["schema"], "decodex.mcp.pr_review_state/1");

		let current_lane_reviews =
			pr_review_state["current_lane_reviews"].as_array().expect("review array");

		assert!(
			current_lane_reviews.is_empty(),
			"unexpected current lane reviews: {current_lane_reviews:?}"
		);
		assert_eq!(protocol_activity["schema"], "decodex.mcp.protocol_activity/1");
		assert_eq!(protocol_activity["run_id"], "run-12");
		assert!(
			serde_json::to_string(&protocol_activity)
				.expect("protocol activity should serialize")
				.contains("redacted_sensitive_detail")
		);

		assert_no_sensitive_observability_content(&serde_json::json!({
			"status_live": status_live,
			"activity_tail": activity_tail,
			"lane_inspect": lane_inspect,
			"lane_control_issue": lane_control_issue,
			"lane_control": lane_control,
			"pr_review_state": pr_review_state,
			"protocol_activity": protocol_activity
		}));
	}

	#[test]
	fn streamable_http_resources_read_exposes_observability_resources() {
		let repo = test_repo();
		let _runtime_home_guard = isolated_mcp_runtime_home(&repo);
		let config_path = repo.path().join("project.toml");

		seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

		let context = McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: Some(config_path),
			project_id: Some(String::from("pubfi")),
			state_store: None,
		};
		let mut handler =
			http_handler_with_context(context, McpCapabilityProfile::Observe, Vec::new());
		let initialize = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
		let status_live = http_resource_read_json(
			&mut handler,
			&session_id,
			2,
			"decodex://projects/pubfi/status_live",
		);
		let activity_tail = http_resource_read_json(
			&mut handler,
			&session_id,
			3,
			"decodex://projects/pubfi/activity_tail",
		);
		let pr_review_state = http_resource_read_json(
			&mut handler,
			&session_id,
			4,
			"decodex://projects/pubfi/pr_review_state",
		);
		let lane_inspect = http_resource_read_json(
			&mut handler,
			&session_id,
			5,
			"decodex://projects/pubfi/lane_inspect/PUB-012",
		);
		let lane_control_issue = http_resource_read_json(
			&mut handler,
			&session_id,
			6,
			"decodex://projects/pubfi/lane-control/PUB-012",
		);
		let lane_control = http_resource_read_json(
			&mut handler,
			&session_id,
			7,
			"decodex://projects/pubfi/lane-control",
		);
		let protocol_activity = http_resource_read_json(
			&mut handler,
			&session_id,
			8,
			"decodex://projects/pubfi/runs/run-12/protocol_activity",
		);
		let hidden_run = http_json_rpc(
			&mut handler,
			&session_id,
			r#"{"jsonrpc":"2.0","id":9,"method":"resources/read","params":{"uri":"decodex://projects/pubfi/runs/run-01/events"}}"#,
		);

		assert_eq!(status_live["schema"], "decodex.mcp.status_live/1");
		assert_eq!(activity_tail["schema"], "decodex.mcp.activity_tail/1");
		assert_eq!(
			activity_tail["activity"].as_array().expect("activity array").len(),
			super::DEFAULT_MCP_STATUS_LIMIT
		);
		assert_eq!(pr_review_state["schema"], "decodex.mcp.pr_review_state/1");

		assert_public_lane_inspect_resource(&lane_inspect);
		assert_public_lane_inspect_resource(&lane_control_issue);
		assert_public_lane_control_readback(&lane_control);

		assert_eq!(protocol_activity["schema"], "decodex.mcp.protocol_activity/1");

		let current_lane_reviews =
			pr_review_state["current_lane_reviews"].as_array().expect("review array");

		assert!(
			current_lane_reviews.is_empty(),
			"unexpected current lane reviews: {current_lane_reviews:?}"
		);
		assert!(
			serde_json::to_string(&protocol_activity)
				.expect("protocol activity should serialize")
				.contains("redacted_sensitive_detail")
		);
		assert_eq!(hidden_run["error"]["code"], super::RESOURCE_NOT_FOUND_CODE);

		assert_no_sensitive_observability_content(&serde_json::json!({
			"status_live": status_live,
			"activity_tail": activity_tail,
			"pr_review_state": pr_review_state,
			"lane_inspect": lane_inspect,
			"lane_control_issue": lane_control_issue,
			"lane_control": lane_control,
			"protocol_activity": protocol_activity
		}));
	}

	fn assert_public_lane_inspect_resource(value: &Value) {
		assert_eq!(value["schema"], "decodex.mcp.lane_inspect/1");
		assert_eq!(value["projectId"], "pubfi");
		assert_eq!(value["issue"], "PUB-012");
		assert_eq!(value["matchedRunCount"], 1);

		let run = &value["runs"][0];

		assert_eq!(run["runId"], "run-12");
		assert!(run["status"].as_str().is_some());
		assert!(run["phase"].as_str().is_some());
		assert!(run["currentOperation"].as_str().is_some());
		assert!(run["laneControlNextAction"].as_str().is_some());
		assert!(run["eventCount"].as_i64().is_some());

		assert_no_lane_runtime_identifiers(value);
	}

	fn assert_public_lane_control_readback(value: &Value) {
		assert_eq!(value["schema"], "decodex.mcp.lane_control_readback/1");
		assert_eq!(value["project_id"], "pubfi");
		assert_eq!(value["read_only"], true);

		let run = find_public_lane_control_run(value, "run-12");

		assert_eq!(run["run_id"], "run-12");
		assert!(run["status"].as_str().is_some());
		assert!(run["phase"].as_str().is_some());
		assert!(run["current_operation"].as_str().is_some());
		assert!(run["lane_control_next_action"].as_str().is_some());
		assert!(run["event_count"].as_i64().is_some());

		assert_no_lane_runtime_identifiers(value);
	}

	fn find_public_lane_control_run<'a>(value: &'a Value, run_id: &str) -> &'a Value {
		for key in ["current_lanes", "recent_runs"] {
			if let Some(run) = value[key]
				.as_array()
				.into_iter()
				.flatten()
				.find(|run| run.get("run_id").and_then(Value::as_str) == Some(run_id))
			{
				return run;
			}
		}

		panic!("public lane-control readback should include run {run_id}");
	}

	fn assert_no_lane_runtime_identifiers(value: &Value) {
		let serialized = serde_json::to_string(value).expect("value should serialize");

		for sensitive in [
			"threadId",
			"turnId",
			"threadStatus",
			"processId",
			"processAlive",
			"processLivenessReason",
			"thread_id",
			"turn_id",
			"thread_status",
			"process_id",
			"process_alive",
			"process_liveness_reason",
			"worktreePath",
			"worktree_path",
			"thread-12",
			"turn-12",
		] {
			assert!(!serialized.contains(sensitive), "lane inspect leaked {sensitive}");
		}
	}

	#[test]
	fn prompts_list_and_get_return_prompt_messages() {
		let repo = test_repo();
		let list_responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list","params":{}}"#,
		);
		let prompts =
			response_at(&list_responses, 0)["result"]["prompts"].as_array().expect("prompts array");
		let prompt_names = prompts
			.iter()
			.filter_map(|prompt| prompt.get("name").and_then(Value::as_str))
			.collect::<Vec<_>>();

		assert!(prompt_names.contains(&"decodex_validation_ready"));

		let get_responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"decodex_validation_ready","arguments":{"issue":"XY-994"}}}"#,
		);
		let messages = response_at(&get_responses, 0)["result"]["messages"]
			.as_array()
			.expect("messages array");
		let text = messages[0]["content"]["text"].as_str().expect("prompt text");

		assert!(text.contains("XY-994"));
		assert!(text.contains("validation-ready"));
	}

	#[test]
	fn prompts_get_rejects_missing_required_arguments() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"prompts/get","params":{"name":"decodex_validation_ready","arguments":{}}}"#,
		);
		let error = response_at(&responses, 0).get("error").expect("error response");

		assert_eq!(error["code"], -32_602);
	}

	#[test]
	fn tools_list_exposes_schema_bound_tools() {
		let repo = test_repo();
		let responses =
			run_stdio(repo.path(), r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#);
		let tools = response_at(&responses, 0)["result"]["tools"].as_array().expect("tools array");
		let tool_names = tools
			.iter()
			.filter_map(|tool| tool.get("name").and_then(Value::as_str))
			.collect::<Vec<_>>();
		let plan = tools
			.iter()
			.find(|tool| tool.get("name").and_then(Value::as_str) == Some("decodex_plan"))
			.expect("plan tool should be listed");

		for tool_name in ["research_compile", "research_promote", "intake_goal"] {
			assert!(tool_names.contains(&tool_name), "{tool_name} should be listed");

			let tool = tools
				.iter()
				.find(|tool| tool.get("name").and_then(Value::as_str) == Some(tool_name))
				.expect("planning tool should exist");

			assert!(tool.get("inputSchema").is_some());
			assert!(tool.get("outputSchema").is_some());
			assert_eq!(tool["_meta"]["decodex/capabilityProfile"], "plan");
		}

		assert!(plan.get("inputSchema").is_some());
		assert!(plan.get("outputSchema").is_some());
		assert_eq!(plan["_meta"]["decodex/capabilityProfile"], "plan");

		assert_tool_output_schema_variant(plan, "decodex.mcp.plan_result/1", Some("next_action"));
		assert_tool_output_schema_variant(plan, "decodex.mcp.refusal/1", Some("reason"));
		assert_tool_output_schema_variant(
			plan,
			"decodex.mcp.tool_validation_error/1",
			Some("tool"),
		);
		assert_tool_output_schema_variant(
			tools
				.iter()
				.find(|tool| tool.get("name").and_then(Value::as_str) == Some("research_compile"))
				.expect("research_compile tool should exist"),
			"decodex.mcp.research_compile_result/1",
			Some("contract_id"),
		);
		assert_tool_output_schema_variant(
			tools
				.iter()
				.find(|tool| tool.get("name").and_then(Value::as_str) == Some("research_promote"))
				.expect("research_promote tool should exist"),
			"decodex.mcp.research_promote_result/1",
			Some("contract_id"),
		);
		assert_tool_output_schema_variant(
			tools
				.iter()
				.find(|tool| tool.get("name").and_then(Value::as_str) == Some("intake_goal"))
				.expect("intake_goal tool should exist"),
			"decodex.mcp.intake_goal_result/1",
			Some("issues"),
		);
	}

	#[test]
	fn tools_list_filters_by_active_capability_profile() {
		let repo = test_repo();
		let responses = run_stdio_with_profile(
			repo.path(),
			McpCapabilityProfile::Observe,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
		);
		let tools = response_at(&responses, 0)["result"]["tools"].as_array().expect("tools array");
		let tool_names = tools
			.iter()
			.filter_map(|tool| tool.get("name").and_then(Value::as_str))
			.collect::<Vec<_>>();

		assert_eq!(tool_names, vec!["decodex_observe"]);
	}

	#[test]
	fn tools_call_refuses_tools_above_active_capability_profile() {
		let repo = test_repo();
		let responses = run_stdio_with_profile(
			repo.path(),
			McpCapabilityProfile::Observe,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
		);
		let structured = &response_at(&responses, 0)["result"]["structuredContent"];

		assert_eq!(structured["schema"], "decodex.mcp.refusal/1");
		assert_eq!(structured["reason"], "insufficient_capability_profile");
		assert_eq!(structured["capability_profile"], "observe");
		assert_eq!(structured["required_capability_profile"], "plan");
	}

	#[test]
	fn tools_call_returns_structured_content() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready","issue":"XY-994"}}}"#,
		);
		let structured = &response_at(&responses, 0)["result"]["structuredContent"];

		assert_eq!(structured["schema"], "decodex.mcp.plan_result/1");
		assert_eq!(structured["status"], "ok");
		assert_eq!(structured["issue"], "XY-994");
	}

	#[test]
	fn tools_call_research_compile_dry_run_returns_structured_contract() {
		let repo = test_repo();
		let context = McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: None,
		};
		let responses = run_stdio_with_context(
			context,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_compile","arguments":{"mode":"dry_run","intent":"research schema-bound MCP planning","outcome":"not_decision_ready"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];
		let structured = &result["structuredContent"];

		assert_eq!(result["isError"], false);
		assert_eq!(structured["schema"], "decodex.mcp.research_compile_result/1");
		assert_eq!(structured["status"], "ok");
		assert_eq!(structured["mode"], "dry_run");
		assert_eq!(structured["persisted"], false);
		assert_eq!(structured["contract_status"], "draft_latent");
		assert_eq!(structured["execution_authority_granted"], false);
	}

	#[test]
	fn tools_call_research_compile_apply_requires_authority() {
		let repo = test_repo();
		let context = McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: None,
		};
		let responses = run_stdio_with_context(
			context,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_compile","arguments":{"mode":"apply","intent":"research schema-bound MCP planning","outcome":"not_decision_ready"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
		assert_eq!(result["structuredContent"]["reason"], "missing_authority");
		assert_eq!(result["structuredContent"]["tool"], "research_compile");
	}

	#[test]
	fn tools_call_research_promote_defaults_to_dry_run() {
		let repo = test_repo();
		let state_store = StateStore::open_in_memory().expect("state store should open");

		state_store
			.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
			.expect("decision contract should persist");

		let context = McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		};
		let responses = run_stdio_with_context(
			context,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_promote","arguments":{"contractId":"research-x-loop-contract"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];
		let structured = &result["structuredContent"];

		assert_eq!(result["isError"], false);
		assert_eq!(structured["schema"], "decodex.mcp.research_promote_result/1");
		assert_eq!(structured["mode"], "dry_run");
		assert_eq!(structured["persisted"], false);
		assert_eq!(structured["contract_id"], "research-x-loop-contract");
	}

	#[test]
	fn tools_call_research_promote_apply_requires_authority() {
		let repo = test_repo();
		let context = McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open_in_memory().expect("state store should open")),
		};
		let responses = run_stdio_with_context(
			context,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"research_promote","arguments":{"mode":"apply","contractId":"research-design-contract"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
		assert_eq!(result["structuredContent"]["reason"], "missing_authority");
		assert_eq!(result["structuredContent"]["tool"], "research_promote");
	}

	#[test]
	fn tools_call_intake_goal_dry_run_does_not_persist_program_intake() {
		let repo = test_repo();
		let db_path = repo.path().join("runtime.sqlite3");
		let seed_store = StateStore::open(&db_path).expect("state store should open");

		seed_store
			.upsert_decision_contract("decodex", Some("XY-852"), accepted_mcp_goal_contract())
			.expect("contract should persist");

		let config_path = repo.path().join("project.toml");

		write_decodex_project_config(&config_path, repo.path());
		write_decodex_workflow(repo.path());

		let context = McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: Some(config_path),
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		};
		let responses = run_stdio_with_context(
			context,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"intake_goal","arguments":{"mode":"dry_run","contractId":"mcp-goal-contract"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];
		let structured = &result["structuredContent"];

		assert_eq!(result["isError"], false);
		assert_eq!(structured["schema"], "decodex.mcp.intake_goal_result/1");
		assert_eq!(structured["mode"], "dry_run");
		assert_eq!(structured["persisted"], false);
		assert_eq!(structured["issue_count"], 1);
		assert_eq!(structured["issues"][0]["action"], "would_create");
		assert!(structured["issues"][0].get("node_id").is_none());
		assert!(structured.get("program_id").is_none());

		let readback = StateStore::open(&db_path).expect("state store should reopen");

		assert!(
			readback
				.list_program_intake_plans("decodex")
				.expect("program intake plans should list")
				.is_empty()
		);
	}

	#[test]
	fn tools_call_intake_goal_apply_requires_authority() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"intake_goal","arguments":{"mode":"apply","contractId":"mcp-goal-contract"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
		assert_eq!(result["structuredContent"]["reason"], "missing_authority");
		assert_eq!(result["structuredContent"]["tool"], "intake_goal");
	}

	#[test]
	fn autonomy_resources_expose_summaries_without_private_payloads() {
		let repo = test_repo();
		let db_path = repo.path().join("runtime.sqlite3");
		let state_store = StateStore::open(&db_path).expect("state store should open");
		let proposal_id = seed_autonomy_mcp_state(&state_store);
		let signal_id = state_store
			.recent_autonomy_signals_for_project("decodex", 1)
			.expect("signals should list")[0]
			.signal_id()
			.to_owned();
		let context = McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		};
		let responses = run_stdio_with_context(
			context,
			&[
				r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy"}}"#,
				r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy/objectives/quality-autonomy/current"}}"#,
				&format!(
					r#"{{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{{"uri":"decodex://projects/decodex/autonomy/signals/{signal_id}"}}}}"#
				),
				&format!(
					r#"{{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{{"uri":"decodex://projects/decodex/autonomy/proposals/{proposal_id}"}}}}"#
				),
				r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy/evidence"}}"#,
			]
			.join("\n"),
		);
		let summary = resource_response_json(&responses, 0);
		let objective = resource_response_json(&responses, 1);
		let signal = resource_response_json(&responses, 2);
		let proposal = resource_response_json(&responses, 3);
		let evidence = resource_response_json(&responses, 4);
		let combined = serde_json::json!({
			"summary": summary,
			"objective": objective,
			"signal": signal,
			"proposal": proposal,
			"evidence": evidence
		});
		let serialized = serde_json::to_string(&combined).expect("resources should serialize");

		assert_eq!(combined["summary"]["schema"], "decodex.mcp.autonomy_summary/1");
		assert_eq!(combined["objective"]["objective"]["state"], "accepted");
		assert_eq!(combined["signal"]["signal"]["kind"], "runtime_health");
		assert_eq!(combined["proposal"]["proposal"]["state"], "decision_candidate");
		assert_eq!(combined["evidence"]["evidence"]["signal_count"], 1);
		assert!(serialized.contains("access_boundary_only"));
		assert!(!serialized.contains("private evidence payload"));
		assert!(!serialized.contains("raw_payload"));

		assert_no_sensitive_observability_content(&combined);
	}

	#[test]
	fn autonomy_resources_redact_local_private_signal_refs() {
		let repo = test_repo();
		let state_store = StateStore::open_in_memory().expect("state store should open");

		state_store
			.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture())
			.expect("objective draft should persist");
		state_store
			.accept_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				1,
				AutonomyObjectiveAcceptance::new(
					"operator",
					AutonomyObjectiveActorKind::User,
					"2026-06-23T00:00:00Z",
					"conversation",
				)
				.expect("acceptance should validate"),
			)
			.expect("objective should accept");

		let signal = AutonomySignal::runtime_health(AutonomySignalInput {
			project_id: String::from("decodex"),
			objective_id: String::from("quality-autonomy"),
			objective_version: 1,
			source_type: AutonomySignalSourceType::Memory,
			source_refs: vec![
				String::from("memory:private:alpha"),
				String::from("report:private:beta"),
			],
			primary_source_refs: vec![String::from("memory:private:primary")],
			issue_id: Some(String::from("XY-1090")),
			run_id: None,
			attempt_id: None,
			head_sha: None,
			captured_at: String::from("2026-06-23T00:01:00Z"),
			freshness: AutonomySignalFreshness::Fresh,
			summary: String::from("Private memory signal is summarized."),
			evidence: vec![String::from("private evidence summarized")],
			evidence_class: AutonomySignalEvidenceClass::Inference,
			contradictions: Vec::new(),
			gaps: Vec::new(),
			confidence: AutonomySignalConfidence::Medium,
			privacy: AutonomySignalPrivacy::LocalPrivate,
			observed_counts: std::collections::BTreeMap::new(),
			review_evidence: None,
			proposal_only: true,
			created_at: String::from("2026-06-23T00:01:05Z"),
		})
		.expect("local private signal should validate");
		let signal_id = signal.id().to_owned();

		state_store.record_autonomy_signal("decodex", signal).expect("signal should persist");

		let responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(state_store),
			},
			&[
				r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy"}}"#,
				&format!(
					r#"{{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{{"uri":"decodex://projects/decodex/autonomy/signals/{signal_id}"}}}}"#
				),
			]
			.join("\n"),
		);
		let summary = resource_response_json(&responses, 0);
		let signal = resource_response_json(&responses, 1);
		let combined = serde_json::json!({
			"summary": summary,
			"signal": signal
		});
		let serialized = serde_json::to_string(&combined).expect("resources should serialize");

		for private_ref in ["memory:private:alpha", "report:private:beta", "memory:private:primary"]
		{
			assert!(!serialized.contains(private_ref), "local-private ref leaked: {private_ref}");
		}

		assert_eq!(combined["signal"]["signal"]["source_refs"], serde_json::json!([]));
		assert_eq!(combined["signal"]["signal"]["source_ref_count"], 2);
		assert_eq!(combined["signal"]["signal"]["primary_source_refs"], serde_json::json!([]));
		assert_eq!(combined["signal"]["signal"]["primary_source_ref_count"], 1);
		assert_eq!(combined["signal"]["signal"]["redaction_level"], "local_private");
		assert_eq!(combined["summary"]["signals"][0]["source_refs"], serde_json::json!([]));
		assert_eq!(combined["summary"]["signals"][0]["source_ref_count"], 2);
	}

	#[test]
	fn autonomy_tools_are_plan_profile_and_apply_requires_authority() {
		let repo = test_repo();
		let observe_responses = run_stdio_with_profile(
			repo.path(),
			McpCapabilityProfile::Observe,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_submit_signal","arguments":{"kind":"runtime_health","signal":{}}}}"#,
		);
		let observe_structured = &response_at(&observe_responses, 0)["result"]["structuredContent"];

		assert_eq!(observe_structured["reason"], "insufficient_capability_profile");
		assert_eq!(observe_structured["required_capability_profile"], "plan");

		let observe_accept_responses = run_stdio_with_profile(
			repo.path(),
			McpCapabilityProfile::Observe,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"objectiveId":"quality-autonomy","objectiveVersion":1}}}"#,
		);
		let observe_accept_structured =
			&response_at(&observe_accept_responses, 0)["result"]["structuredContent"];

		assert_eq!(observe_accept_structured["reason"], "insufficient_capability_profile");
		assert_eq!(observe_accept_structured["required_capability_profile"], "plan");

		let state_store = StateStore::open_in_memory().expect("state store should open");
		let context = McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		};
		let responses = run_stdio_with_context(
			context,
			r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"autonomy_draft_objective","arguments":{"mode":"apply","objective":{"schema":"decodex.autonomy_objective/1","record_version":1,"project_id":"decodex","id":"quality-autonomy","version":1,"state":"draft","summary":"Improve quality.","goals":["Reduce churn."],"non_goals":["Do not bypass authority."],"metrics":["Validation retry count."],"allowed_surfaces":["apps/decodex/src"],"allowed_signal_kinds":["runtime_health"],"validation_gates":["cargo make check"],"review_policy":"review required","memory_policy":"source-linked only","report_policy":"public-safe only"}}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
		assert_eq!(result["structuredContent"]["reason"], "missing_authority");
		assert_eq!(result["structuredContent"]["tool"], "autonomy_draft_objective");
	}

	#[test]
	fn autonomy_accept_objective_accepts_draft_without_execution_authority() {
		let repo = test_repo();
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let draft_call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_draft_objective","arguments":{"mode":"apply","objective":{"schema":"decodex.autonomy_objective/1","record_version":1,"project_id":"decodex","id":"self-iteration-pilot","version":1,"state":"draft","summary":"Pilot Decodex self-iteration only on the decodex project.","goals":["Reduce repeated operator intervention.","Convert Decodex-only feedback into evidence-backed proposals."],"non_goals":["Do not touch other projects.","Do not bypass review, landing, install, restart, or plugin-sync gates."],"metrics":["Manual-attention count.","Validated proposal replay completeness."],"allowed_surfaces":["apps/decodex/src","automations/decodex","docs","plugins/decodex","plugins/knowledge"],"allowed_signal_kinds":["runtime_health","protocol_drift","execution_friction","docs_skill_drift","validation_regression","user_feedback_cluster"],"validation_gates":["cargo make check-docs","cargo test -p decodex mcp --lib"],"review_policy":"challenge required before promotion","memory_policy":"source-linked evidence only","report_policy":"public-safe source refs with known gaps"},"authority":{"source":"mcp-test","reason":"store draft objective"}}}}"#;
		let accept_missing_authority_call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"mode":"apply","objectiveId":"self-iteration-pilot","objectiveVersion":1}}}"#;
		let accept_call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"mode":"apply","objectiveId":"self-iteration-pilot","objectiveVersion":1,"authority":{"acceptedBy":"operator","acceptedByKind":"user","acceptedAt":"2026-06-27T00:00:00Z","acceptanceSource":"conversation"}}}}"#;
		let read_call = r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy/objectives/self-iteration-pilot/current"}}"#;
		let responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(state_store),
			},
			&format!("{draft_call}\n{accept_missing_authority_call}\n{accept_call}\n{read_call}"),
		);
		let draft_result = &response_at(&responses, 0)["result"]["structuredContent"];
		let missing_authority_result = &response_at(&responses, 1)["result"];
		let accept_result = &response_at(&responses, 2)["result"]["structuredContent"];
		let read_result = &response_at(&responses, 3)["result"]["contents"][0]["text"];
		let read_json: serde_json::Value =
			serde_json::from_str(read_result.as_str().expect("resource text should parse"))
				.expect("resource should be json");

		assert_eq!(draft_result["schema"], "decodex.mcp.autonomy_objective_result/1");
		assert_eq!(draft_result["objective"]["state"], "draft");
		assert_eq!(draft_result["persisted"], true);
		assert_eq!(missing_authority_result["isError"], true);
		assert_eq!(missing_authority_result["structuredContent"]["reason"], "missing_authority");
		assert_eq!(accept_result["schema"], "decodex.mcp.autonomy_objective_result/1");
		assert_eq!(accept_result["objective"]["state"], "accepted");
		assert_eq!(accept_result["objective"]["acceptance_present"], true);
		assert_eq!(accept_result["authority_effect"], "accepted_objective_no_execution_authority");
		assert_eq!(read_json["objective"]["objective_id"], "self-iteration-pilot");
		assert_eq!(read_json["objective"]["state"], "accepted");
	}

	#[test]
	fn autonomy_accept_objective_refuses_caller_supplied_runtime_policy_authority() {
		let repo = test_repo();
		let state_store = StateStore::open_in_memory().expect("state store should open");

		state_store
			.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture())
			.expect("objective draft should persist");

		let responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(state_store),
			},
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"mode":"apply","objectiveId":"quality-autonomy","objectiveVersion":1,"authority":{"acceptedBy":"policy:auto","acceptedByKind":"runtime_policy","acceptanceSource":"caller-supplied-policy"}}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
		assert_eq!(result["structuredContent"]["reason"], "objective_acceptance_refused");
		assert!(
			result["structuredContent"]["message"]
				.as_str()
				.expect("refusal message should be text")
				.contains("trusted Decodex authority state")
		);
	}

	fn seed_autonomy_challenged_proposal() -> (TempDir, std::path::PathBuf, String) {
		let repo = test_repo();
		let db_path = repo.path().join("runtime.sqlite3");
		let state_store = StateStore::open(&db_path).expect("state store should open");

		state_store
			.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture())
			.expect("objective draft should persist");
		state_store
			.accept_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				1,
				AutonomyObjectiveAcceptance::new(
					"operator",
					AutonomyObjectiveActorKind::User,
					"2026-06-23T00:00:00Z",
					"conversation",
				)
				.expect("acceptance should validate"),
			)
			.expect("objective should accept");

		let signal_call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_submit_signal","arguments":{"mode":"apply","kind":"runtime_health","signal":{"objectiveId":"quality-autonomy","objectiveVersion":1,"sourceType":"runtime","sourceRefs":["status:XY-1090"],"freshness":"fresh","summary":"Runtime status is consistent.","evidence":["status readback summarized"],"evidenceClass":"live_readback","confidence":"high","privacy":"team"},"authority":{"source":"mcp-test","reason":"submit evidence"}}}}"#;
		let signal_responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
			},
			signal_call,
		);
		let signal_result = &response_at(&signal_responses, 0)["result"]["structuredContent"];
		let signal_id = signal_result["signal"]["signal_id"].as_str().expect("signal id");
		let proposal_call = format!(
			r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"autonomy_compile_proposal","arguments":{{"mode":"apply","signalIds":["{signal_id}"],"proposal":{{"objectiveId":"quality-autonomy","objectiveVersion":1,"sourceFamily":"runtime_health","intendedSurface":"apps/decodex/src/mcp.rs","summary":"Expose autonomy MCP surface.","challengeRequirements":["independent challenge"],"rollbackPath":"Revert MCP autonomy surface."}},"authority":{{"source":"mcp-test","reason":"compile proposal evidence"}}}}}}}}"#
		);
		let proposal_responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
			},
			&proposal_call,
		);
		let proposal_result = &response_at(&proposal_responses, 0)["result"]["structuredContent"];
		let proposal_id = proposal_result["proposal"]["proposal_id"].as_str().expect("proposal id");
		let challenge_call = format!(
			r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"autonomy_challenge_proposal","arguments":{{"mode":"apply","proposalId":"{proposal_id}","challenge":{{"source":"inline_skeptic","actor":"skeptic","summary":"Challenge recorded.","objections":[]}},"authority":{{"source":"mcp-test","reason":"record challenge"}}}}}}}}"#
		);
		let challenge_responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
			},
			&challenge_call,
		);
		let challenge_result = &response_at(&challenge_responses, 0)["result"]["structuredContent"];

		assert_eq!(signal_result["schema"], "decodex.mcp.autonomy_signal_result/1");
		assert_eq!(signal_result["persisted"], true);
		assert_eq!(proposal_result["schema"], "decodex.mcp.autonomy_proposal_result/1");
		assert_eq!(proposal_result["proposal"]["state"], "decision_candidate");
		assert_eq!(challenge_result["schema"], "decodex.mcp.autonomy_challenge_result/1");
		assert_eq!(challenge_result["challenge_evidence_count"], 1);

		(repo, db_path, proposal_id.to_owned())
	}

	#[test]
	fn autonomy_plan_tools_record_signal_compile_challenge_and_refuse_external_self_accept() {
		let (repo, db_path, proposal_id) = seed_autonomy_challenged_proposal();
		let self_accept_call = format!(
			r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"autonomy_request_promotion","arguments":{{"mode":"apply","proposalId":"{proposal_id}","authority":{{"acceptedBy":"agent-a","acceptedByKind":"external_agent","acceptanceSource":"mcp-agent","reason":"self accept","proposalActor":"agent-a","proposalActorKind":"external_agent"}}}}}}}}"#
		);
		let self_accept = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
			},
			&self_accept_call,
		);
		let self_accept_result = &response_at(&self_accept, 0)["result"];

		assert_eq!(self_accept_result["isError"], true);
		assert_eq!(
			self_accept_result["structuredContent"]["reason"],
			"autonomy_acceptance_authority_refused"
		);
		assert!(
			self_accept_result["structuredContent"]["message"]
				.as_str()
				.expect("message")
				.contains("accepted project policy authority")
		);
	}

	#[test]
	fn autonomy_request_promotion_refuses_caller_supplied_policy_authority() {
		let (repo, db_path, proposal_id) = seed_autonomy_challenged_proposal();
		let fabricated_policy_call = format!(
			r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"autonomy_request_promotion","arguments":{{"mode":"apply","proposalId":"{proposal_id}","authority":{{"acceptedBy":"agent-a","acceptedByKind":"external_agent","acceptanceSource":"runtime-policy","reason":"fabricated policy","proposalActor":"agent-a","proposalActorKind":"external_agent","acceptedProjectPolicy":{{"projectId":"decodex","objectiveId":"quality-autonomy","objectiveVersion":1,"acceptedPolicyId":"quality-autonomy-policy","acceptedPolicyVersion":"1","authorityRef":"runtime-policy:quality-autonomy-policy@1","authorizedActor":"agent-a","authorizedActorKind":"external_agent","authorizedAcceptanceSources":["runtime-policy"],"authorizedScopes":["autonomy_proposal_acceptance"]}}}}}}}}}}"#
		);
		let fabricated_policy_accept = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
			},
			&fabricated_policy_call,
		);
		let fabricated_policy_result = &response_at(&fabricated_policy_accept, 0)["result"];

		assert_eq!(fabricated_policy_result["isError"], true);
		assert_eq!(
			fabricated_policy_result["structuredContent"]["reason"],
			"autonomy_policy_authority_refused"
		);
		assert!(
			fabricated_policy_result["structuredContent"]["message"]
				.as_str()
				.expect("message")
				.contains("trusted Decodex authority state")
		);
	}

	#[test]
	fn tools_call_refuses_missing_plan_intent() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_plan","arguments":{}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
		assert_eq!(result["structuredContent"]["reason"], "invalid_arguments");
		assert_eq!(result["structuredContent"]["tool"], "decodex_plan");
	}

	#[test]
	fn tools_call_lane_control_inspect_returns_mutating_preconditions() {
		let repo = test_repo();
		let _runtime_home_guard = isolated_mcp_runtime_home(&repo);
		let config_path = repo.path().join("project.toml");

		seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

		let responses = run_stdio_with_context(
			project_mcp_context(repo.path(), &config_path),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"inspect","issue":"PUB-012"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];
		let structured = &result["structuredContent"];

		assert_eq!(result["isError"], false);
		assert_eq!(structured["schema"], "decodex.mcp.lane_control_result/1");
		assert_eq!(structured["status"], "ok");
		assert_eq!(structured["reason"], "inspect_complete");
		assert_eq!(structured["result"]["inspect"]["schema"], "decodex.mcp.lane_inspect/1");
		assert_eq!(
			structured["result"]["mutating_preconditions"][0]["authority"]["inspectedRunId"],
			"run-12"
		);
		assert_eq!(
			structured["result"]["mutating_preconditions"][0]["authority"]["expectedTurnId"],
			"turn-12"
		);

		assert_no_sensitive_observability_content(structured);
	}

	#[test]
	fn tools_call_refuses_lane_control_mutation_without_inspect_precondition() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"interrupt","issue":"XY-994","runId":"run-1"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.lane_control_result/1");
		assert_eq!(result["structuredContent"]["status"], "refused");
		assert_eq!(result["structuredContent"]["reason"], "authority_required");
	}

	#[test]
	fn tools_call_refuses_missing_lane_control_action() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"issue":"XY-994"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
		assert_eq!(result["structuredContent"]["reason"], "invalid_arguments");
		assert_eq!(result["structuredContent"]["tool"], "decodex_lane_control");
	}

	#[test]
	fn tools_call_lane_control_refuses_stale_expected_turn_id() {
		let repo = test_repo();
		let _runtime_home_guard = isolated_mcp_runtime_home(&repo);
		let config_path = repo.path().join("project.toml");

		seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

		let responses = run_stdio_with_context(
			project_mcp_context(repo.path(), &config_path),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"steer","projectId":"pubfi","issue":"PUB-012","runId":"run-12","expectedTurnId":"turn-old","message":"Please stop after the current safe point.","authority":{"reason":"operator requested steer","source":"mcp-test","inspectedRunId":"run-12","expectedTurnId":"turn-old"}}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];
		let structured = &result["structuredContent"];

		assert_eq!(result["isError"], true);
		assert_eq!(structured["status"], "refused");
		assert_eq!(structured["reason"], "stale_expected_turn_id");
		assert_eq!(structured["result"]["failureClass"], "stale_expected_turn_id");
		assert_eq!(structured["result"]["currentTurnId"], "turn-12");

		assert_no_sensitive_observability_content(structured);
	}

	#[test]
	fn tools_call_lane_control_steer_audits_and_queues_without_raw_message() {
		let repo = test_repo();
		let _runtime_home_guard = isolated_mcp_runtime_home(&repo);
		let config_path = repo.path().join("project.toml");

		seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

		let responses = run_stdio_with_context(
			project_mcp_context(repo.path(), &config_path),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"steer","projectId":"pubfi","issue":"PUB-012","runId":"run-12","expectedTurnId":"turn-12","message":"Please stop after the current safe point.","authority":{"reason":"operator requested steer","source":"mcp-test","inspectedRunId":"run-12","expectedTurnId":"turn-12"}}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];
		let structured = &result["structuredContent"];
		let serialized = serde_json::to_string(structured).expect("structured should serialize");

		assert_eq!(result["isError"], false);
		assert_eq!(structured["status"], "queued");
		assert_eq!(structured["result"]["deliveryStatus"], "queued");
		assert_eq!(structured["result"]["messageLineCount"], 1);
		assert!(!serialized.contains("Please stop after the current safe point."));

		assert_no_sensitive_observability_content(structured);

		let state_store = runtime::open_runtime_store().expect("runtime store should open");
		let events = state_store
			.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
			.expect("private events should read");

		assert!(events.iter().any(|event| event.event_type() == "control_action"));
		assert!(events.iter().any(|event| event.event_type() == "lane_control/steer/requested"));
	}

	#[test]
	fn tools_call_lane_control_soft_interrupt_accepts_and_force_requires_ack() {
		let repo = test_repo();
		let _runtime_home_guard = isolated_mcp_runtime_home(&repo);
		let config_path = repo.path().join("project.toml");

		seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

		let force_refusal = run_stdio_with_context(
			project_mcp_context(repo.path(), &config_path),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"interrupt","projectId":"pubfi","issue":"PUB-012","runId":"run-12","force":true,"authority":{"reason":"operator requested hard fallback","source":"mcp-test","inspectedRunId":"run-12"}}}}"#,
		);
		let force_structured = &response_at(&force_refusal, 0)["result"]["structuredContent"];

		assert_eq!(force_structured["status"], "refused");
		assert_eq!(force_structured["reason"], "hard_fallback_authority_missing");

		let soft_acceptance = run_stdio_with_context(
			project_mcp_context(repo.path(), &config_path),
			r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"interrupt","projectId":"pubfi","issue":"PUB-012","runId":"run-12","authority":{"reason":"operator requested soft interrupt","source":"mcp-test","inspectedRunId":"run-12"}}}}"#,
		);
		let soft_result = &response_at(&soft_acceptance, 0)["result"];
		let soft_structured = &soft_result["structuredContent"];

		assert_eq!(soft_result["isError"], false);
		assert_eq!(soft_structured["status"], "queued");
		assert_eq!(
			soft_structured["result"]["softInterrupt"]["classification"],
			"soft_interrupt_pending"
		);
		assert_eq!(soft_structured["result"]["hardInterrupt"], Value::Null);

		assert_no_sensitive_observability_content(soft_structured);
	}

	#[test]
	fn tools_call_project_control_pauses_future_dispatch_only() {
		let repo = test_repo();
		let _runtime_home_guard = isolated_mcp_runtime_home(&repo);
		let config_path = repo.path().join("project.toml");

		seed_project_runtime_for_mcp_resources(repo.path(), &config_path);
		seed_mcp_test_private_control_evidence();

		let responses = run_stdio_with_context(
			project_mcp_context(repo.path(), &config_path),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_project_control","arguments":{"action":"pause","projectId":"pubfi","authority":{"reason":"operator pause","source":"mcp-test","acknowledgeFutureDispatchOnly":true}}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];
		let structured = &result["structuredContent"];

		assert_eq!(result["isError"], false);
		assert_eq!(structured["schema"], "decodex.mcp.project_control_result/1");
		assert_eq!(structured["status"], "ok");
		assert_eq!(structured["project_id"], "pubfi");
		assert_eq!(structured["future_dispatch_only"], true);
		assert_eq!(structured["result"]["enabled"], false);
		assert_eq!(structured["result"]["active_lanes_killed"], false);

		let state_store = runtime::open_runtime_store().expect("runtime store should open");
		let projects = state_store.list_projects().expect("projects should list");
		let project = projects
			.iter()
			.find(|project| project.service_id() == "pubfi")
			.expect("pubfi should remain registered");
		let events = state_store
			.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
			.expect("private events should read");

		assert!(!project.enabled());
		assert!(!events.is_empty(), "pause should not remove active lane evidence");
	}

	#[test]
	fn tools_call_project_control_scan_refuses_without_operator_loop() {
		let repo = test_repo();
		let responses = run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("pubfi")),
				state_store: None,
			},
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_project_control","arguments":{"action":"scan","projectId":"pubfi"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.project_control_result/1");
		assert_eq!(result["structuredContent"]["reason"], "operator_control_loop_required");
	}

	#[test]
	fn tools_call_refuses_missing_project_control_action() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_project_control","arguments":{}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
		assert_eq!(result["structuredContent"]["reason"], "invalid_arguments");
		assert_eq!(result["structuredContent"]["tool"], "decodex_project_control");
	}

	#[test]
	fn tools_call_returns_structured_refusal_for_invalid_observe_arguments() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_observe","arguments":{"limit":0}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["status"], "refused");
		assert_eq!(result["structuredContent"]["reason"], "invalid_limit");
	}

	#[test]
	fn tools_call_emits_json_rpc_progress_notification_when_requested() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
		);

		assert_eq!(responses[0]["method"], "notifications/progress");
		assert_eq!(responses[0]["params"]["progressToken"], "progress-1");
		assert_eq!(responses[1]["result"]["structuredContent"]["status"], "ok");
	}

	#[test]
	fn tools_call_does_not_emit_progress_for_invalid_params() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"}}}"#,
		);

		assert_eq!(responses.len(), 1);
		assert_eq!(responses[0]["id"], 1);
		assert_eq!(responses[0]["error"]["code"], -32_602);
	}

	#[test]
	fn tools_call_does_not_emit_progress_for_structured_validation_error() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(responses.len(), 1);
		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
	}

	#[test]
	fn tools_call_does_not_emit_progress_for_structured_refusal() {
		let repo = test_repo();
		let responses = run_stdio_with_profile(
			repo.path(),
			McpCapabilityProfile::Observe,
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(responses.len(), 1);
		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
		assert_eq!(result["structuredContent"]["reason"], "insufficient_capability_profile");
	}

	#[test]
	fn streamable_http_json_post_initializes_session() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193"), ("Accept", "application/json")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let body = response.json_body();

		assert_eq!(response.status, "HTTP/1.1 200 OK");
		assert_eq!(response.header("content-type"), Some("application/json"));
		assert!(response.header("mcp-session-id").is_some());
		assert_eq!(response.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));
		assert_eq!(response.header("access-control-expose-headers"), Some("Mcp-Session-Id"));
		assert_eq!(
			body["result"]["capabilities"]["experimental"]["decodex"]["transport"],
			"streamable-http"
		);
		assert_eq!(
			body["result"]["capabilities"]["experimental"]["decodex"]["remoteControl"]["httpTransport"],
			"streamable-http"
		);
	}

	#[test]
	fn streamable_http_allows_cors_preflight_for_trusted_origin() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
		let response = run_http(
			&mut handler,
			http_options(
				"/mcp",
				[
					("Origin", "http://127.0.0.1:8193"),
					("Access-Control-Request-Method", "POST"),
					("Access-Control-Request-Headers", "Content-Type, Mcp-Session-Id"),
				],
			),
		);

		assert_eq!(response.status, "HTTP/1.1 204 No Content");
		assert_eq!(response.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));
		assert_eq!(response.header("access-control-allow-methods"), Some("POST, DELETE, OPTIONS"));
		assert_eq!(
			response.header("access-control-allow-headers"),
			Some("Content-Type, Accept, Mcp-Session-Id, Authorization")
		);
	}

	#[test]
	fn streamable_http_bearer_auth_challenges_missing_or_invalid_authorization() {
		let repo = test_repo();
		let mut handler = http_handler_with_authorization(
			repo.path(),
			McpCapabilityProfile::Observe,
			McpHttpAuthorization::from_token_for_test("secret-token"),
		);

		for headers in [
			vec![("Origin", "http://127.0.0.1:8193")],
			vec![("Origin", "http://127.0.0.1:8193"), ("Authorization", "Bearer wrong-token")],
		] {
			let response = run_http(
				&mut handler,
				http_post(
					"/mcp",
					headers,
					r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
				),
			);
			let body = response.json_body();

			assert_eq!(response.status, "HTTP/1.1 401 Unauthorized");
			assert_eq!(response.header("www-authenticate"), Some("Bearer realm=\"decodex-mcp\""));
			assert_eq!(body["error"]["message"], "Unauthorized");
			assert!(!response.body_text().contains("secret-token"));
		}
	}

	#[test]
	fn streamable_http_bearer_auth_accepts_valid_authorization() {
		let repo = test_repo();
		let mut handler = http_handler_with_authorization(
			repo.path(),
			McpCapabilityProfile::Observe,
			McpHttpAuthorization::from_token_for_test("secret-token"),
		);
		let preflight = run_http(
			&mut handler,
			http_options(
				"/mcp",
				[
					("Origin", "http://127.0.0.1:8193"),
					("Access-Control-Request-Method", "POST"),
					("Access-Control-Request-Headers", "Authorization, Content-Type"),
				],
			),
		);
		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193"), ("Authorization", "Bearer secret-token")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let body = response.json_body();

		assert_eq!(preflight.status, "HTTP/1.1 204 No Content");
		assert_eq!(
			preflight.header("access-control-allow-headers"),
			Some("Content-Type, Accept, Mcp-Session-Id, Authorization")
		);
		assert_eq!(response.status, "HTTP/1.1 200 OK");
		assert!(response.header("mcp-session-id").is_some());
		assert_eq!(
			body["result"]["capabilities"]["experimental"]["decodex"]["capabilityProfile"],
			"observe"
		);
		assert!(!response.body_text().contains("secret-token"));
	}

	#[test]
	fn streamable_http_allows_configured_origin() {
		let repo = test_repo();
		let mut handler = http_handler_with_allowed_origins(
			repo.path(),
			McpCapabilityProfile::Admin,
			vec![String::from("https://relay.example")],
		);
		let preflight = run_http(
			&mut handler,
			http_options(
				"/mcp",
				[("Origin", "https://relay.example"), ("Access-Control-Request-Method", "POST")],
			),
		);
		let initialize = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "https://relay.example")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);

		assert_eq!(preflight.status, "HTTP/1.1 204 No Content");
		assert_eq!(preflight.header("access-control-allow-origin"), Some("https://relay.example"));
		assert_eq!(initialize.status, "HTTP/1.1 200 OK");
		assert!(initialize.header("mcp-session-id").is_some());
		assert_eq!(initialize.header("access-control-allow-origin"), Some("https://relay.example"));
	}

	#[test]
	fn streamable_http_sse_response_includes_progress_notification() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
		let initialize = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[
					("Origin", "http://127.0.0.1:8193"),
					("Accept", "text/event-stream"),
					("Mcp-Session-Id", session_id.as_str()),
				],
				r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"_meta":{"progressToken":"progress-1"},"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
			),
		);
		let body = response.body_text();

		assert_eq!(response.status, "HTTP/1.1 200 OK");
		assert_eq!(response.header("content-type"), Some("text/event-stream"));
		assert_eq!(response.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));
		assert_eq!(response.header("access-control-expose-headers"), Some("Mcp-Session-Id"));
		assert!(body.contains("event: message"));
		assert!(body.contains("\"method\":\"notifications/progress\""));
		assert!(body.contains("\"id\":2"));
	}

	#[test]
	fn streamable_http_initialize_notification_does_not_create_session() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193")],
				r#"{"jsonrpc":"2.0","method":"initialize","params":{}}"#,
			),
		);

		assert_eq!(response.status, "HTTP/1.1 202 Accepted");
		assert_eq!(response.header("mcp-session-id"), None);

		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[
					("Origin", "http://127.0.0.1:8193"),
					("Mcp-Session-Id", "decodex-mcp-session-0000000000000001"),
				],
				r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
			),
		);
		let body = response.json_body();

		assert_eq!(response.status, "HTTP/1.1 404 Not Found");
		assert_eq!(body["error"]["message"], "Unknown MCP session");
	}

	#[test]
	fn streamable_http_invalid_initialize_does_not_create_session() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193")],
				r#"{"jsonrpc":"1.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let body = response.json_body();

		assert_eq!(response.status, "HTTP/1.1 200 OK");
		assert_eq!(response.header("mcp-session-id"), None);
		assert_eq!(body["error"]["message"], "Invalid Request");
	}

	#[test]
	fn streamable_http_rejects_disallowed_origin() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "https://example.invalid")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let body = response.json_body();

		assert_eq!(response.status, "HTTP/1.1 403 Forbidden");
		assert_eq!(body["error"]["message"], "Forbidden origin");
	}

	#[test]
	fn streamable_http_delete_invalidates_session() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);
		let initialize = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
		let delete = run_http(
			&mut handler,
			http_delete(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id.as_str())],
			),
		);

		assert_eq!(delete.status, "HTTP/1.1 202 Accepted");
		assert_eq!(delete.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));

		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id.as_str())],
				r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
			),
		);
		let body = response.json_body();

		assert_eq!(response.status, "HTTP/1.1 404 Not Found");
		assert_eq!(body["error"]["message"], "Unknown MCP session");
	}

	#[test]
	fn streamable_http_bind_guard_requires_loopback_or_allowed_origin() {
		assert!(
			mcp::validate_mcp_http_listen_address(
				DEFAULT_MCP_HTTP_LISTEN_ADDRESS,
				&[],
				&McpHttpAuthorization::disabled()
			)
			.is_ok()
		);
		assert!(
			mcp::validate_mcp_http_listen_address(
				"0.0.0.0:8193",
				&[],
				&McpHttpAuthorization::disabled()
			)
			.is_err()
		);
		assert!(
			mcp::validate_mcp_http_listen_address(
				"0.0.0.0:8193",
				&[String::from("https://relay.example")],
				&McpHttpAuthorization::disabled()
			)
			.is_err()
		);
		assert!(
			mcp::validate_mcp_http_listen_address(
				"0.0.0.0:8193",
				&[String::from("https://relay.example")],
				&McpHttpAuthorization::from_token_for_test("secret-token")
			)
			.is_ok()
		);
	}

	#[test]
	fn streamable_http_elevated_profile_requires_bearer_authorization() {
		assert!(
			mcp::validate_mcp_http_capability_profile(
				McpCapabilityProfile::Observe,
				&McpHttpAuthorization::disabled()
			)
			.is_ok()
		);

		for profile in
			[McpCapabilityProfile::Plan, McpCapabilityProfile::Operate, McpCapabilityProfile::Admin]
		{
			assert!(
				mcp::validate_mcp_http_capability_profile(
					profile,
					&McpHttpAuthorization::disabled()
				)
				.is_err()
			);
			assert!(
				mcp::validate_mcp_http_capability_profile(
					profile,
					&McpHttpAuthorization::from_token_for_test("secret-token")
				)
				.is_ok()
			);
		}
	}

	#[test]
	fn streamable_http_requires_known_session_after_initialize() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Admin);

		run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);

		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193")],
				r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
			),
		);
		let body = response.json_body();

		assert_eq!(response.status, "HTTP/1.1 428 Precondition Required");
		assert_eq!(body["error"]["message"], "Missing MCP session");
	}

	#[test]
	fn streamable_http_observe_profile_exposes_only_observe_tool() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Observe);
		let initialize = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();
		let response = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id.as_str())],
				r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
			),
		);
		let body = response.json_body();
		let tool_names = body["result"]["tools"]
			.as_array()
			.expect("tools array")
			.iter()
			.filter_map(|tool| tool.get("name").and_then(Value::as_str))
			.collect::<Vec<_>>();

		assert_eq!(tool_names, vec!["decodex_observe"]);
	}

	#[test]
	fn streamable_http_observe_profile_refuses_operate_and_admin_calls() {
		let repo = test_repo();
		let mut handler = http_handler(repo.path(), McpCapabilityProfile::Observe);
		let initialize = run_http(
			&mut handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193")],
				r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			),
		);
		let session_id = initialize.header("mcp-session-id").expect("session id").to_owned();

		for (tool, required_profile, arguments) in [
			("decodex_lane_control", "operate", r#"{"action":"inspect"}"#),
			("decodex_project_control", "admin", r#"{"action":"status","projectId":"pubfi"}"#),
		] {
			let response = run_http(
				&mut handler,
				http_post(
					"/mcp",
					[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id.as_str())],
					&format!(
						r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{tool}","arguments":{arguments}}}}}"#
					),
				),
			);
			let body = response.json_body();
			let structured = &body["result"]["structuredContent"];

			assert_eq!(structured["schema"], "decodex.mcp.refusal/1");
			assert_eq!(structured["reason"], "insufficient_capability_profile");
			assert_eq!(structured["capability_profile"], "observe");
			assert_eq!(structured["required_capability_profile"], required_profile);
		}
	}

	#[test]
	fn resources_read_rejects_invalid_resource_uri() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"file:///etc/passwd"}}"#,
		);
		let error = response_at(&responses, 0).get("error").expect("error response");

		assert_eq!(error["code"], -32_602);
	}

	#[test]
	fn stdio_output_contains_only_json_rpc_responses() {
		let repo = test_repo();
		let input = [
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":3,"method":"resources/templates/list","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":4,"method":"prompts/list","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":5,"method":"prompts/get","params":{"name":"decodex_validation_ready","arguments":{"issue":"XY-994"}}}"#,
			r#"{"jsonrpc":"2.0","id":6,"method":"tools/list","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
		]
		.join("\n");
		let output = run_stdio_raw(repo.path(), &input);
		let lines = output.lines().collect::<Vec<_>>();

		assert_eq!(lines.len(), 7);

		for line in lines {
			let value = serde_json::from_str::<Value>(line).expect("stdout line should be JSON");

			assert_eq!(value["jsonrpc"], "2.0");
		}
	}

	fn run_stdio(repo_root: &Path, input: &str) -> Vec<Value> {
		run_stdio_raw(repo_root, input)
			.lines()
			.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
			.collect()
	}

	fn run_stdio_with_context(context: McpContext, input: &str) -> Vec<Value> {
		run_stdio_raw_with_context(context, input)
			.lines()
			.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
			.collect()
	}

	fn run_stdio_with_profile(
		repo_root: &Path,
		capability_profile: McpCapabilityProfile,
		input: &str,
	) -> Vec<Value> {
		let context = McpContext {
			repo_root: repo_root.to_path_buf(),
			config_path: None,
			project_id: None,
			state_store: None,
		};

		run_stdio_raw_with_profile(context, capability_profile, input)
			.lines()
			.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
			.collect()
	}

	fn project_mcp_context(repo_root: &Path, config_path: &Path) -> McpContext {
		McpContext {
			repo_root: repo_root.to_path_buf(),
			config_path: Some(config_path.to_path_buf()),
			project_id: Some(String::from("pubfi")),
			state_store: None,
		}
	}

	fn run_stdio_raw(repo_root: &Path, input: &str) -> String {
		let context = McpContext {
			repo_root: repo_root.to_path_buf(),
			config_path: None,
			project_id: None,
			state_store: None,
		};

		run_stdio_raw_with_context(context, input)
	}

	fn run_stdio_raw_with_context(context: McpContext, input: &str) -> String {
		run_stdio_raw_with_profile(context, McpCapabilityProfile::Admin, input)
	}

	fn run_stdio_raw_with_profile(
		context: McpContext,
		capability_profile: McpCapabilityProfile,
		input: &str,
	) -> String {
		let mut output = Vec::new();

		mcp::serve_stdio_with_profile(
			Cursor::new(format!("{input}\n")),
			&mut output,
			context,
			capability_profile,
		)
		.expect("stdio server should run");

		String::from_utf8(output).expect("stdout should be utf-8")
	}

	fn response_at(responses: &[Value], index: usize) -> &Value {
		responses.get(index).expect("response should exist")
	}

	fn response_error(responses: &[Value], index: usize) -> &Value {
		response_at(responses, index).get("error").expect("error response")
	}

	fn resource_response_json(responses: &[Value], index: usize) -> Value {
		let contents = response_at(responses, index)["result"]["contents"]
			.as_array()
			.expect("resource contents array");
		let text = contents[0]["text"].as_str().expect("resource text should exist");

		serde_json::from_str(text).expect("resource text should be JSON")
	}

	fn assert_tool_output_schema_variant(tool: &Value, schema: &str, required_field: Option<&str>) {
		let variants = tool["outputSchema"]["oneOf"].as_array().expect("oneOf variants");
		let variant = variants
			.iter()
			.find(|variant| {
				variant["properties"]["schema"]["enum"]
					.as_array()
					.expect("schema enum")
					.iter()
					.any(|value| value.as_str() == Some(schema))
			})
			.expect("schema variant should exist");

		if let Some(required_field) = required_field {
			assert!(
				variant["required"]
					.as_array()
					.expect("required array")
					.iter()
					.any(|value| value.as_str() == Some(required_field))
			);
		}
	}

	fn sensitive_observability_fixture() -> Value {
		serde_json::json!({
			"schema": "decodex.operator.snapshot/1",
			"project": {
				"repoRoot": "/private/repo",
				"config_path": "/private/project.toml",
				"visible": "kept"
			},
			"runs": [
				{
					"issue": "XY-994",
					"effective_cwd": "/private/worktree",
					"private_evidence": {
						"read_command": "decodex evidence --config /private/project.toml --issue XY-994"
					},
					"github_cli_authority": {
						"github_command_path": "/private/bin/gh",
						"github_token_env_var": "GITHUB_PAT_Y"
					},
					"nested": {
						"readCommand": "decodex evidence --config /private/project.toml",
						"privateEvidenceRef": "private-ref",
						"safe": "kept"
					}
				}
			]
		})
	}

	fn observability_snapshot_fixture() -> Value {
		serde_json::json!({
			"schema": "decodex.mcp.status_resource/1",
			"project_id": "decodex",
			"status_source": "live",
			"run_limit": 10,
			"current_lanes": [observability_current_lane_fixture()],
			"recent_runs": [observability_recent_run_fixture()],
			"post_review_lanes": [observability_post_review_lane_fixture()]
		})
	}

	fn observability_current_lane_fixture() -> Value {
		serde_json::json!({
			"run_id": "run-1",
			"issue_id": "issue-1",
			"issue_identifier": "XY-996",
			"attempt_number": 2,
			"status": "running",
			"attempt_status": "starting",
			"phase": "implementing",
			"run_phase": "implement_to_validation_ready",
			"wait_reason": "model_execution",
			"current_operation": "model_execution",
			"lane_control_next_action": "inspect_or_interrupt_orphaned_live_thread",
			"event_count": 6,
			"last_event_type": "turn/delta",
			"last_event_at": "2026-06-18T00:00:00Z",
			"last_protocol_activity_at": "2026-06-18T00:00:01Z",
			"last_progress_at": "2026-06-18T00:00:02Z",
			"progress_diagnostic": "protocol_only_activity",
			"suspected_stall": false,
			"protocol_activity": observability_protocol_activity_fixture(),
			"child_agent_activity": {
				"event_count": 2,
				"current_bucket": "protocol_activity",
				"path": "/private/activity-marker"
			},
			"phase_acceptance": observability_phase_acceptance_fixture(),
			"private_evidence": {
				"raw": "hidden"
			},
			"worktree_path": "/private/worktree"
		})
	}

	fn observability_protocol_activity_fixture() -> Value {
		serde_json::json!({
			"turn_status": "running",
			"waiting_reason": "model_execution",
			"recent_events": [
				{
					"event_type": "turn/delta",
					"category": "work_progress",
					"detail": "diff updated",
					"private_evidence": "private-ref"
				},
				{
					"event_type": "response/reasoning/summary",
					"category": "reasoning",
					"detail": "hidden chain of thought",
					"text": "private reasoning text",
					"summary": "private reasoning summary",
					"body": "private reasoning body"
				},
				{
					"event_type": "configWarning",
					"category": "warning",
					"detail": "config at /private/worktree using GITHUB_PAT_Y"
				},
				{
					"event_type": "error",
					"category": "protocol_error",
					"detail": "failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK"
				},
				{
					"event_type": "configWarning",
					"category": "warning",
					"detail": "state marker under /srv/decodex/runtime"
				},
				{
					"event_type": "error",
					"category": "protocol_error",
					"detail": "upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456"
				},
				{
					"event_type": "error",
					"category": "protocol_error",
					"detail": "upstream auth failed for 8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U"
				}
			]
		})
	}

	fn observability_phase_acceptance_fixture() -> Value {
		serde_json::json!({
			"phase": "handoff_evidence",
			"decision": "accepted",
			"reason_code": "phase_goal_satisfied",
			"objective_covered": true,
			"effective_delta_present": true,
			"changed_surfaces": ["phase-private-surface"],
			"non_goal_passed": true,
			"validation_passed": true,
			"recorded_at": "2026-06-18T00:00:03Z",
			"run_id": "phase-private-run",
			"attempt_number": 2,
			"next_action": "request_review"
		})
	}

	fn observability_review_status_fixture(
		head_sha: &str,
		active_fingerprint: &str,
		stop_fingerprint: &str,
		round: i64,
	) -> Value {
		serde_json::json!({
			"phase": "handoff",
			"status": "pending",
			"checkpoint": {
				"head_sha": head_sha,
				"round": round,
				"nonclean_rounds": 2,
				"active_fingerprints": [active_fingerprint],
				"stop_fingerprint": stop_fingerprint,
				"updated_at": "2026-06-18T00:00:04Z"
			},
			"privateEvidenceRef": "private-review-ref"
		})
	}

	fn observability_recent_run_fixture() -> Value {
		serde_json::json!({
			"run_id": "run-1",
			"issue_id": "issue-1",
			"issue_identifier": "XY-996",
			"status": "running",
			"loop_status": {
				"review": {
					"status": "duplicate_recent"
				}
			}
		})
	}

	fn observability_post_review_lane_fixture() -> Value {
		serde_json::json!({
			"project_id": "decodex",
			"issue_id": "issue-1",
			"issue_identifier": "XY-996",
			"issue_state": "In Review",
			"branch_name": "private-branch-name",
			"worktree_path": "/private/review-worktree",
			"classification": "review_pending",
			"reason": "external_review_pending",
			"pr_url": "https://example/pr/1",
			"pr_head_sha": "private-pr-head",
			"pr_state": "OPEN",
			"review_state": "pending",
			"review_decision": "REVIEW_REQUIRED",
			"mergeable": "MERGEABLE",
			"check_state": "PENDING",
			"unresolved_review_threads": 1,
			"shadowed_by_current_lane": false,
			"readback_warning": "none",
			"readback_root_cause": "none",
			"loop_status": {
				"review": observability_review_status_fixture(
					"private-lane-head-sha",
					"lane-fingerprint-private",
					"lane-stop-fingerprint-private",
					4
				)
			},
			"private_evidence_ref": "private-pr-ref"
		})
	}

	fn assert_observability_is_sanitized(value: &Value) {
		let serialized = serde_json::to_string(value).expect("value should serialize");

		for sensitive in [
			"repoRoot",
			"config_path",
			"effective_cwd",
			"private_evidence",
			"privateEvidenceRef",
			"read_command",
			"readCommand",
			"github_cli_authority",
			"github_command_path",
			"github_token_env_var",
			"/private",
			"GITHUB_PAT_Y",
		] {
			assert!(!serialized.contains(sensitive), "sanitized value leaked {sensitive}");
		}

		assert!(serialized.contains("kept"));
	}

	fn assert_no_sensitive_observability_content(value: &Value) {
		let serialized = serde_json::to_string(value).expect("value should serialize");

		for sensitive in [
			"/private",
			"/Users/x",
			"private_evidence",
			"privateEvidenceRef",
			"private_evidence_ref",
			"private-ref",
			"private-review-ref",
			"private-pr-ref",
			"worktree_path",
			"worktreePath",
			"raw",
			"hidden chain of thought",
			"private reasoning text",
			"private reasoning summary",
			"private reasoning body",
			"GITHUB_PAT_Y",
			"LINEAR_API_KEY_HACKINK",
			"/srv/decodex/runtime",
			"ghp_abcdefghijklmnopqrstuvwxyz123456",
			"8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U",
			"active_fingerprints",
			"stop_fingerprint",
			"head_sha",
			"changed_surfaces",
			"recorded_at",
			"phase-private-surface",
			"phase-private-run",
			"private-head-sha",
			"fingerprint-private",
			"stop-fingerprint-private",
			"private-branch-name",
			"private-pr-head",
			"private-lane-head-sha",
			"lane-fingerprint-private",
			"lane-stop-fingerprint-private",
		] {
			assert!(!serialized.contains(sensitive), "sanitized value leaked {sensitive}");
		}
	}

	fn autonomy_objective_fixture() -> AutonomyObjectiveContract {
		serde_json::from_value(serde_json::json!({
			"schema": "decodex.autonomy_objective/1",
			"record_version": 1,
			"project_id": "decodex",
			"id": "quality-autonomy",
			"version": 1,
			"state": "draft",
			"summary": "Improve Decodex autonomy quality under explicit authority.",
			"goals": ["Reduce repeated validation and review churn."],
			"non_goals": ["Do not bypass Decision Contract authority."],
			"metrics": ["Validation retry count stays below objective tolerance."],
			"allowed_surfaces": ["apps/decodex/src/mcp.rs", "docs/spec/autonomy-control-plane.md"],
			"allowed_signal_kinds": ["runtime_health", "docs_skill_drift"],
			"validation_gates": ["cargo test -p decodex mcp --lib"],
			"review_policy": "independent current-head review required",
			"memory_policy": "source-linked read-only memory only",
			"report_policy": "public-safe summaries only"
		}))
		.expect("autonomy objective fixture should deserialize")
	}

	fn seed_autonomy_mcp_state(state_store: &StateStore) -> String {
		state_store
			.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture())
			.expect("objective draft should persist");
		state_store
			.accept_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				1,
				AutonomyObjectiveAcceptance::new(
					"operator",
					AutonomyObjectiveActorKind::User,
					"2026-06-23T00:00:00Z",
					"conversation",
				)
				.expect("acceptance should validate"),
			)
			.expect("objective should accept");

		let signal = AutonomySignal::runtime_health(AutonomySignalInput {
			project_id: String::from("decodex"),
			objective_id: String::from("quality-autonomy"),
			objective_version: 1,
			source_type: AutonomySignalSourceType::Runtime,
			source_refs: vec![String::from("status:XY-1090")],
			primary_source_refs: Vec::new(),
			issue_id: Some(String::from("XY-1090")),
			run_id: None,
			attempt_id: None,
			head_sha: None,
			captured_at: String::from("2026-06-23T00:01:00Z"),
			freshness: AutonomySignalFreshness::Fresh,
			summary: String::from("Runtime status is consistent."),
			evidence: vec![String::from("status readback summarized")],
			evidence_class: AutonomySignalEvidenceClass::LiveReadback,
			contradictions: Vec::new(),
			gaps: Vec::new(),
			confidence: AutonomySignalConfidence::High,
			privacy: AutonomySignalPrivacy::Team,
			observed_counts: std::collections::BTreeMap::new(),
			review_evidence: None,
			proposal_only: true,
			created_at: String::from("2026-06-23T00:01:05Z"),
		})
		.expect("runtime signal should validate");
		let signal_id = signal.id().to_owned();

		state_store.record_autonomy_signal("decodex", signal).expect("signal should persist");

		let proposal = state_store
			.compile_autonomy_proposal_dry_run(
				AutonomyProposalCompileInput {
					project_id: String::from("decodex"),
					objective_id: String::from("quality-autonomy"),
					objective_version: 1,
					source_family: String::from("runtime_health"),
					intended_surface: String::from("apps/decodex/src/mcp.rs"),
					affected_identifiers: vec![String::from("XY-1090")],
					summary: String::from("Expose autonomy MCP surface."),
					challenge_requirements: vec![String::from("independent challenge")],
					rejected_alternatives: Vec::new(),
					rollback_path: String::from("Revert MCP autonomy surface."),
					weakened_validation_or_review: Vec::new(),
					created_at: String::from("2026-06-23T00:02:00Z"),
				},
				&[signal_id],
			)
			.expect("proposal should compile");
		let proposal_id = proposal.id().to_owned();

		state_store.record_autonomy_proposal("decodex", proposal).expect("proposal should persist");

		proposal_id
	}

	fn http_handler(repo_root: &Path, capability_profile: McpCapabilityProfile) -> McpHttpHandler {
		http_handler_with_allowed_origins(repo_root, capability_profile, Vec::new())
	}

	fn http_handler_with_authorization(
		repo_root: &Path,
		capability_profile: McpCapabilityProfile,
		authorization: McpHttpAuthorization,
	) -> McpHttpHandler {
		let context = McpContext {
			repo_root: repo_root.to_path_buf(),
			config_path: None,
			project_id: None,
			state_store: None,
		};

		http_handler_with_context_and_authorization(
			context,
			capability_profile,
			Vec::new(),
			authorization,
		)
	}

	fn http_handler_with_allowed_origins(
		repo_root: &Path,
		capability_profile: McpCapabilityProfile,
		allowed_origins: Vec<String>,
	) -> McpHttpHandler {
		let context = McpContext {
			repo_root: repo_root.to_path_buf(),
			config_path: None,
			project_id: None,
			state_store: None,
		};

		http_handler_with_context(context, capability_profile, allowed_origins)
	}

	fn http_handler_with_context(
		context: McpContext,
		capability_profile: McpCapabilityProfile,
		allowed_origins: Vec<String>,
	) -> McpHttpHandler {
		http_handler_with_context_and_authorization(
			context,
			capability_profile,
			allowed_origins,
			McpHttpAuthorization::disabled(),
		)
	}

	fn http_handler_with_context_and_authorization(
		context: McpContext,
		capability_profile: McpCapabilityProfile,
		allowed_origins: Vec<String>,
		authorization: McpHttpAuthorization,
	) -> McpHttpHandler {
		McpHttpHandler {
			server: McpServer {
				context,
				capability_profile,
				transport: McpTransport::StreamableHttp,
			},
			sessions: McpHttpSessions::default(),
			allowed_origins,
			listen_address: Some(String::from(DEFAULT_MCP_HTTP_LISTEN_ADDRESS)),
			authorization,
		}
	}

	fn run_http(handler: &mut McpHttpHandler, request: Vec<u8>) -> ParsedHttpResponse {
		let response =
			handler.handle_request_bytes(&request).expect("HTTP handler should return response");

		ParsedHttpResponse::parse(&response)
	}

	fn http_json_rpc(handler: &mut McpHttpHandler, session_id: &str, body: &str) -> Value {
		let response = run_http(
			handler,
			http_post(
				"/mcp",
				[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id)],
				body,
			),
		);

		assert_eq!(response.status, "HTTP/1.1 200 OK");
		assert_eq!(response.header("content-type"), Some("application/json"));
		assert_eq!(response.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));

		response.json_body()
	}

	fn http_resource_read_json(
		handler: &mut McpHttpHandler,
		session_id: &str,
		id: u64,
		uri: &str,
	) -> Value {
		let request = serde_json::json!({
			"jsonrpc": "2.0",
			"id": id,
			"method": "resources/read",
			"params": {
				"uri": uri
			}
		})
		.to_string();
		let response = http_json_rpc(handler, session_id, &request);
		let contents = response["result"]["contents"].as_array().expect("resource contents array");
		let text = contents[0]["text"].as_str().expect("resource text should exist");

		serde_json::from_str(text).expect("resource text should be JSON")
	}

	fn http_post<'a>(
		path: &str,
		headers: impl IntoIterator<Item = (&'a str, &'a str)>,
		body: &str,
	) -> Vec<u8> {
		let mut request = format!(
			"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:8193\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
			body.len()
		);

		for (name, value) in headers {
			request.push_str(name);
			request.push_str(": ");
			request.push_str(value);
			request.push_str("\r\n");
		}

		request.push_str("\r\n");
		request.push_str(body);

		request.into_bytes()
	}

	fn http_delete<'a>(
		path: &str,
		headers: impl IntoIterator<Item = (&'a str, &'a str)>,
	) -> Vec<u8> {
		let mut request =
			format!("DELETE {path} HTTP/1.1\r\nHost: 127.0.0.1:8193\r\nContent-Length: 0\r\n");

		for (name, value) in headers {
			request.push_str(name);
			request.push_str(": ");
			request.push_str(value);
			request.push_str("\r\n");
		}

		request.push_str("\r\n");

		request.into_bytes()
	}

	fn http_options<'a>(
		path: &str,
		headers: impl IntoIterator<Item = (&'a str, &'a str)>,
	) -> Vec<u8> {
		let mut request =
			format!("OPTIONS {path} HTTP/1.1\r\nHost: 127.0.0.1:8193\r\nContent-Length: 0\r\n");

		for (name, value) in headers {
			request.push_str(name);
			request.push_str(": ");
			request.push_str(value);
			request.push_str("\r\n");
		}

		request.push_str("\r\n");

		request.into_bytes()
	}

	fn test_repo() -> TempDir {
		let repo = TempDir::new().expect("temp repo should exist");

		write_file(repo.path().join("Cargo.toml"), "[workspace]\n");
		write_file(repo.path().join("docs/index.md"), "# Docs\n");
		write_file(repo.path().join("docs/policy.md"), "# Policy\n");
		write_file(repo.path().join("docs/spec/runtime.md"), "# Runtime\n\nSpec body.\n");
		write_file(repo.path().join("docs/decisions/mcp-gateway.md"), "# MCP\n");
		write_file(repo.path().join("docs/research/sample-report.md"), "# Sample Research\n");

		repo
	}

	fn isolated_mcp_runtime_home(repo: &TempDir) -> TestEnvVarGuard {
		let runtime_home = repo.path().join("operator-home");
		let runtime_home = runtime_home.to_string_lossy().into_owned();

		TestEnvVarGuard::set_many([
			("CODEX_HOME".to_owned(), runtime_home.clone()),
			("HOME".to_owned(), runtime_home),
		])
	}

	#[test]
	fn mcp_project_fixture_runtime_store_stays_under_isolated_home() {
		let operator_runtime_db =
			runtime::runtime_db_path().expect("operator runtime path should resolve");
		let repo = test_repo();
		let _runtime_home_guard = isolated_mcp_runtime_home(&repo);
		let config_path = repo.path().join("project.toml");

		seed_project_runtime_for_mcp_resources(repo.path(), &config_path);
		run_stdio_with_context(
			project_mcp_context(repo.path(), &config_path),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"steer","projectId":"pubfi","issue":"PUB-012","runId":"run-12","expectedTurnId":"turn-12","message":"Please stop after the current safe point.","authority":{"reason":"operator requested steer","source":"mcp-test","inspectedRunId":"run-12","expectedTurnId":"turn-12"}}}}"#,
		);

		let fixture_runtime_db =
			runtime::runtime_db_path().expect("fixture runtime path should resolve");
		let state_store = runtime::open_runtime_store().expect("fixture runtime store should open");
		let events = state_store
			.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
			.expect("fixture private evidence should read");

		assert_ne!(fixture_runtime_db, operator_runtime_db);
		assert!(fixture_runtime_db.starts_with(repo.path()));
		assert!(!events.is_empty());
		assert!(
			events
				.iter()
				.all(|event| event.payload().get("source").and_then(Value::as_str)
					== Some("mcp-test")),
			"mcp fixture private evidence should stay in isolated runtime store"
		);
	}

	fn seed_project_runtime_for_mcp_resources(repo_root: &Path, config_path: &Path) {
		let state_store = runtime::open_runtime_store().expect("runtime store should open");

		write_project_config(config_path, repo_root);
		write_project_workflow(repo_root);

		runtime::register_project_config(&state_store, config_path, true)
			.expect("project should register");

		for index in 1..=12 {
			let issue_id = format!("PUB-{index:03}");
			let run_id = format!("run-{index:02}");
			let worktree_path = repo_root.join(format!("worktrees/{issue_id}"));
			let attempt_status = if index == 12 { "running" } else { "succeeded" };

			state_store
				.upsert_worktree(
					"pubfi",
					&issue_id,
					&format!("x/pubfi-{index:03}"),
					&worktree_path.display().to_string(),
				)
				.expect("worktree should record");
			state_store
				.record_run_attempt(&run_id, &issue_id, 1, attempt_status)
				.expect("run attempt should record");
			state_store
				.append_event(&run_id, 1, "turn/completed", r#"{"status":"completed"}"#)
				.expect("event should record");

			if index == 12 {
				seed_mcp_lane_runtime_markers(&state_store, &worktree_path, &run_id);
				seed_mcp_lane_runtime_activity(&state_store, &run_id);
			}
		}
	}

	fn seed_mcp_test_private_control_evidence() {
		let state_store = runtime::open_runtime_store().expect("runtime store should open");

		state_store
			.append_private_execution_event(
				"pubfi",
				"PUB-012",
				"run-12",
				1,
				"control_action",
				serde_json::json!({
					"schema": "decodex.run_control_action/v1",
					"source": "mcp-test",
					"action": "project_control_fixture"
				}),
			)
			.expect("mcp-test private evidence should record");
	}

	fn seed_mcp_lane_runtime_activity(state_store: &StateStore, run_id: &str) {
		state_store
			.append_event(
				run_id,
				2,
				"configWarning",
				r#"{"summary":"config at /private/worktree using GITHUB_PAT_Y"}"#,
			)
			.expect("warning event should record");
		state_store
			.append_event(
				run_id,
				3,
				"error",
				r#"{"error":{"codexErrorInfo":"failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK"}}"#,
			)
			.expect("error event should record");
		state_store
			.append_event(
				run_id,
				4,
				"configWarning",
				r#"{"summary":"state marker under /srv/decodex/runtime"}"#,
			)
			.expect("generic path warning event should record");
		state_store
				.append_event(
					run_id,
					5,
					"error",
					r#"{"error":{"codexErrorInfo":"upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456"}}"#,
				)
				.expect("token-shaped error event should record");
		state_store
			.append_event(
				run_id,
				6,
				"error",
				r#"{"error":{"codexErrorInfo":"upstream auth failed for 8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U"}}"#,
			)
			.expect("bare token-shaped error event should record");

		let protocol_activity = ProtocolActivitySummary {
			turn_status: Some(String::from("completed")),
			waiting_reason: Some(String::from("turn_completed")),
			rate_limit_status: None,
			recent_events: vec![
				ProtocolActivityEventSummary {
					event_type: String::from("configWarning"),
					category: String::from("warning"),
					detail: Some(String::from("config at /private/worktree using GITHUB_PAT_Y")),
				},
				ProtocolActivityEventSummary {
					event_type: String::from("error"),
					category: String::from("protocol_error"),
					detail: Some(String::from(
						"failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK",
					)),
				},
				ProtocolActivityEventSummary {
					event_type: String::from("configWarning"),
					category: String::from("warning"),
					detail: Some(String::from("state marker under /srv/decodex/runtime")),
				},
				ProtocolActivityEventSummary {
					event_type: String::from("error"),
					category: String::from("protocol_error"),
					detail: Some(String::from(
						"upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456",
					)),
				},
				ProtocolActivityEventSummary {
					event_type: String::from("error"),
					category: String::from("protocol_error"),
					detail: Some(String::from(
						"upstream auth failed for 8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U",
					)),
				},
			],
		};

		state_store
			.record_run_activity_summary(run_id, 1, None, Some(&protocol_activity))
			.expect("activity summary should record");
	}

	fn seed_mcp_lane_runtime_markers(state_store: &StateStore, worktree_path: &Path, run_id: &str) {
		fs::create_dir_all(worktree_path).expect("worktree path should exist");

		let control_dir = worktree_path.join(".decodex-run-control");
		let channel_path = control_dir.join("run-12-1.channel");

		fs::create_dir_all(&control_dir).expect("run-control channel dir should exist");
		fs::write(&channel_path, "ready\n").expect("run-control channel should write");

		state_store
			.upsert_lease("pubfi", "PUB-012", run_id, "In Progress")
			.expect("lease should record");
		state_store.update_run_thread(run_id, "thread-12").expect("thread should record");
		state_store.update_run_turn(run_id, "turn-12").expect("turn should record");
		state_store
			.publish_run_control_channel_for_active_attempt(run_id, 1, &channel_path, "local_file")
			.expect("control channel should publish")
			.expect("active control channel should exist");

		state::write_run_activity_marker_for_process(worktree_path, run_id, 1, process::id())
			.expect("activity marker should record process");
		state::write_run_thread_marker(worktree_path, run_id, 1, "thread-12")
			.expect("thread marker should record");
		state::write_run_turn_marker(worktree_path, run_id, 1, "turn-12")
			.expect("turn marker should record");
	}

	fn write_project_config(config_path: &Path, repo_root: &Path) {
		write_file(
			config_path.to_path_buf(),
			&format!(
				r#"
service_id = "pubfi"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "PATH"

[paths]
repo_root = "{}"
"#,
				repo_root.display()
			),
		);
	}

	fn write_project_workflow(repo_root: &Path) {
		write_file(
			repo_root.join("WORKFLOW.md"),
			r#"
+++
version = 1
max_turns = 1

[tracker]
queued_state = "Todo"
in_progress_state = "In Progress"
success_state = "Done"
terminal_states = ["Done", "Canceled"]

[tools]
comment = "issue_comment"
transition = "issue_transition"
label = "issue_label"
progress_checkpoint = "issue_progress_checkpoint"
review_checkpoint = "issue_review_checkpoint"
review_handoff = "issue_review_handoff"
terminal_finalize = "issue_terminal_finalize"
+++
"#,
		);
	}

	fn write_decodex_project_config(config_path: &Path, repo_root: &Path) {
		write_file(
			config_path.to_path_buf(),
			&format!(
				r#"
service_id = "decodex"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "PATH"

[codex]
review = "standard"

[paths]
repo_root = "{}"
worktree_root = ".worktrees"
"#,
				repo_root.display()
			),
		);
	}

	fn write_decodex_workflow(repo_root: &Path) {
		write_file(
			repo_root.join("WORKFLOW.md"),
			r#"+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 3
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
"#,
		);
	}

	fn accepted_mcp_goal_contract() -> DecisionContract {
		let mut contract: DecisionContract = serde_json::from_value(serde_json::json!({
			"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
			"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
			"contract_id": "mcp-goal-contract",
			"status": "draft_latent",
			"source_intent": {
				"summary": "Expose MCP planning tools.",
				"user_utterance": "arrange MCP planning tools",
				"source_issue_identifier": "XY-852"
			},
			"research_provenance": [
				{
					"kind": "spec",
					"reference": "docs/spec/runtime.md",
					"summary": "MCP planning tools are schema-bound."
				}
			],
			"research_evidence": [
				{
					"claim": "Goal intake can preview generated issue briefs.",
					"support": "Program Intake dry-run renders public-safe issue plans.",
					"source_ref": "docs/spec/loop-runtime.md"
				}
			],
			"research_options": [
				{
					"option": "Expose a small schema-bound planning facade.",
					"status": "selected",
					"tradeoffs": ["Keeps internal graph mechanics out of tool output."]
				}
			],
			"accepted_authority": {
				"accepted_objectives": ["Expose schema-bound MCP planning tools."],
				"non_goals": ["Do not expose raw Program graph mutation."],
				"constraints": ["Dry-run must not persist Program Intake rows."],
				"assumptions": ["The promoted contract owns issue shaping."],
				"objections": ["Apply must require explicit authority."],
				"stop_conditions": ["Stop when authority is missing."]
			},
			"execution_readiness": {
				"summary": "Ready for issue shaping.",
				"ready_for_issue_shaping": true,
				"missing_decisions": [],
				"validation_expectations": ["MCP intake dry-run returns public-safe issue rows."],
				"risk_notes": ["Do not expose internal Program node ids."],
				"proposed_issues": [
					{
						"key": "mcp-planning-tools",
						"title": "Expose schema-bound MCP planning tools.",
						"objective": "Expose schema-bound MCP planning tools.",
						"stage": "runtime",
						"dependencies": [],
						"conflict_domains": ["module:decodex-research-intake-tools"],
						"acceptance": ["Planning tools are listed through tools/list."],
						"validation": ["cargo test -p decodex mcp::tests -- --nocapture"],
						"risk": ["Do not expose internal graph mechanics."],
						"queue_intent": "ready_to_queue"
					}
				],
				"conflict_domains": ["module:decodex-research-intake-tools"]
			},
			"links": {
				"generated_issue_ids": [],
				"generated_issue_identifiers": [],
				"execution_program_node_ids": []
			},
			"evidence_boundary": {
				"private_evidence_refs": [],
				"public_projection_refs": [],
				"public_summary": "MCP planning tools are ready for issue shaping."
			}
		}))
		.expect("contract should deserialize");

		contract
			.promote(
				DecisionPromotion::new(
					"operator",
					DecisionPromotionActorKind::User,
					"2026-06-18T00:00:00Z",
					"test",
					Some(String::from("Accepted for MCP intake dry-run testing.")),
				)
				.expect("promotion should build"),
			)
			.expect("contract should promote");

		contract
	}

	fn write_file(path: std::path::PathBuf, contents: &str) {
		let parent = path.parent().expect("test path should have parent");

		fs::create_dir_all(parent).expect("parent directory should exist");
		fs::write(path, contents).expect("test file should write");
	}

	fn latent_decision_contract_fixture() -> DecisionContract {
		serde_json::from_str(include_str!(concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/fixtures/decision_contract/research_x_latent_contract.json"
		)))
		.expect("research X latent contract fixture should deserialize")
	}
}
