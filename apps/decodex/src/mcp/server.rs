use std::io::{BufRead as _, BufReader, ErrorKind, Read, Write};

use serde::Deserialize;
use serde_json::{self, Value};

use crate::orchestrator;

use super::{
	DEFAULT_MCP_STATUS_LIMIT, MCP_HTTP_ENDPOINT_PATH, MCP_PROTOCOL_VERSION, MCP_SESSION_HEADER,
	McpCapabilityProfile, McpContext, McpError, McpTransport, SERVER_NAME,
	TOOL_AUTONOMY_ACCEPT_OBJECTIVE, TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
	TOOL_AUTONOMY_COMPILE_PROPOSAL, TOOL_AUTONOMY_DRAFT_OBJECTIVE, TOOL_AUTONOMY_REQUEST_PROMOTION,
	TOOL_AUTONOMY_SUBMIT_SIGNAL, TOOL_INTAKE_GOAL, TOOL_LANE_CONTROL, TOOL_OBSERVE, TOOL_PLAN,
	TOOL_PROJECT_CONTROL, TOOL_RESEARCH_COMPILE, TOOL_RESEARCH_PROMOTE, capability_profile_refusal,
	invalid_tool_arguments, planning,
	resources::{
		mcp_public_lane_inspect_resource, mcp_status_live_resource,
		sanitize_mcp_observability_value,
	},
	tool_call_result_allows_progress, tool_refusal, tool_success, tools,
};

pub(super) struct McpServer {
	pub(super) context: McpContext,
	pub(super) capability_profile: McpCapabilityProfile,
	pub(super) transport: McpTransport,
}
impl McpServer {
	pub(super) fn handle_line(&self, line: &str, emit_progress: bool) -> Vec<Value> {
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

#[derive(Deserialize)]
struct JsonRpcRequest {
	jsonrpc: Option<String>,
	id: Option<Value>,
	method: Option<String>,
	params: Option<Value>,
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

pub(super) fn serve_stdio_with_profile<R, W>(
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

pub(super) fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
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
