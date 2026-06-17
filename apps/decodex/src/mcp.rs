use std::{
	env,
	fmt::Display,
	fs,
	io::{self, BufRead as _, BufReader, ErrorKind, Read, Write},
	path::{Path, PathBuf},
};

use clap::ValueEnum;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use crate::{
	config::ServiceConfig,
	orchestrator,
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const SERVER_NAME: &str = "decodex";
const DOCS_HOST: &str = "docs";
const RESEARCH_HOST: &str = "research";
const DECISION_CONTRACTS_HOST: &str = "decision-contracts";
const PROJECTS_HOST: &str = "projects";
const RESOURCE_NOT_FOUND_CODE: i64 = -32_002;
const DEFAULT_MCP_STATUS_LIMIT: usize = 10;
const TOOL_OBSERVE: &str = "decodex_observe";
const TOOL_PLAN: &str = "decodex_plan";
const TOOL_LANE_CONTROL: &str = "decodex_lane_control";
const TOOL_ADMIN: &str = "decodex_admin";

/// MCP transport supported by the native Decodex gateway.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum McpTransport {
	/// JSON-RPC messages over stdin/stdout.
	Stdio,
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
}

struct McpServer {
	context: McpContext,
	capability_profile: McpCapabilityProfile,
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
						"transport": "stdio",
						"remoteControl": {
							"safeDefaultProfile": "observe",
							"httpTransport": "deferred_to_XY-995",
							"operateAdminTools": "deferred_to_XY-998",
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

	fn list_resources(&self) -> Result<Value, McpError> {
		let mut resources = self.context.docs_resources()?;

		resources.extend(self.context.decision_contract_resources()?);

		if let Some(project_id) = self.context.project_id() {
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/status"),
				format!("Project {project_id} status"),
				"Read-only local runtime status snapshot.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/lane-control"),
				format!("Project {project_id} lane-control readback"),
				"Read-only lane-control state for current and recent local lanes.",
			));
		}

		Ok(serde_json::json!({ "resources": resources }))
	}

	fn list_resource_templates(&self) -> Value {
		serde_json::json!({
			"resourceTemplates": [
				{
					"uriTemplate": "decodex://docs/spec/{topic}",
					"name": "Decodex specs",
					"description": "Checked-in normative Decodex specification concepts.",
					"mimeType": "text/markdown"
				},
				{
					"uriTemplate": "decodex://docs/runbook/{topic}",
					"name": "Decodex runbooks",
					"description": "Checked-in Decodex operator procedures.",
					"mimeType": "text/markdown"
				},
				{
					"uriTemplate": "decodex://docs/reference/{topic}",
					"name": "Decodex references",
					"description": "Checked-in Decodex implementation and current-state references.",
					"mimeType": "text/markdown"
				},
				{
					"uriTemplate": "decodex://docs/decisions/{topic}",
					"name": "Decodex decisions",
					"description": "Checked-in Decodex design-rationale concepts.",
					"mimeType": "text/markdown"
				},
				{
					"uriTemplate": "decodex://docs/research/{topic}",
					"name": "Decodex research concepts",
					"description": "Checked-in latent Markdown research concepts.",
					"mimeType": "text/markdown"
				},
				{
					"uriTemplate": "decodex://decision-contracts/{contract_id}",
					"name": "Runtime Decision Contracts",
					"description": "Local runtime Decision Contract readback by contract id.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/status",
					"name": "Project status",
					"description": "Local runtime project status readback.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/lane-control/{issue}",
					"name": "Lane-control readback",
					"description": "Inspect one local lane before requesting guarded lane-control actions.",
					"mimeType": "application/json"
				}
			]
		})
	}

	fn read_resource(&self, params: Option<Value>) -> Result<Value, McpError> {
		let params = params.ok_or_else(McpError::invalid_params)?;
		let params = serde_json::from_value::<ReadResourceParams>(params)
			.map_err(|_| McpError::invalid_params())?;
		let content = self.context.read_resource(&params.uri)?;

		Ok(serde_json::json!({
			"contents": [
				{
					"uri": content.uri,
					"mimeType": content.mime_type,
					"text": content.text
				}
			]
		}))
	}

	fn list_prompts(&self) -> Value {
		serde_json::json!({ "prompts": mcp_prompts() })
	}

	fn get_prompt(&self, params: Option<Value>) -> Result<Value, McpError> {
		let params = params.ok_or_else(McpError::invalid_params)?;
		let params = serde_json::from_value::<GetPromptParams>(params)
			.map_err(|_| McpError::invalid_params())?;
		let arguments = params.arguments.unwrap_or_default();

		if !prompt_required_arguments_are_present(&params.name, &arguments) {
			return Err(McpError::invalid_params());
		}

		mcp_prompt_result(&params.name, arguments).ok_or_else(McpError::invalid_params)
	}

	fn list_tools(&self) -> Value {
		let tools = mcp_tools()
			.into_iter()
			.filter(|tool| self.capability_profile.allows(tool.required_profile))
			.map(|tool| tool.value)
			.collect::<Vec<_>>();

		serde_json::json!({ "tools": tools })
	}

	fn call_tool(&self, params: Option<Value>) -> Result<Value, McpError> {
		let params = params.ok_or_else(McpError::invalid_params)?;
		let params = serde_json::from_value::<CallToolParams>(params)
			.map_err(|_| McpError::invalid_params())?;
		let arguments = params.arguments.unwrap_or_else(|| serde_json::json!({}));
		let Some(required_profile) = tool_required_profile(&params.name) else {
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
			TOOL_PLAN => Ok(call_plan_tool(arguments)),
			TOOL_LANE_CONTROL => Ok(lane_control_stub_result(arguments, required_profile)),
			TOOL_ADMIN => Ok(admin_stub_result(arguments, required_profile)),
			_ => Ok(tool_refusal("unknown_tool", "Decodex MCP tool is not registered.")),
		}
	}

	fn call_observe_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<ObserveToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_OBSERVE,
					"`issue`, `runId`, and `limit` are the only supported observe arguments.",
				),
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
		} else {
			orchestrator::build_mcp_status_resource(self.context.config_path.as_deref(), limit)
		};
		let mut value = match observability_result {
			Ok(value) => value,
			Err(_) =>
				return tool_refusal(
					"observability_unavailable",
					"Decodex observability requires a registered project config or --config.",
				),
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
	fn for_process(config_path: Option<&Path>) -> Result<Self> {
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

	fn docs_resources(&self) -> Result<Vec<McpResource>, McpError> {
		let mut resources = Vec::new();

		push_file_resource(
			&mut resources,
			self.repo_root.join("docs/index.md"),
			"decodex://docs/index",
			"Documentation index",
			"Checked-in Decodex documentation router.",
		);
		push_file_resource(
			&mut resources,
			self.repo_root.join("docs/policy.md"),
			"decodex://docs/policy",
			"Documentation policy",
			"Checked-in Decodex documentation policy.",
		);

		for lane in ["spec", "runbook", "reference", "decisions", "research"] {
			let docs_dir = self.repo_root.join("docs").join(lane);

			for entry in read_sorted_dir(&docs_dir)? {
				let Some(stem) = markdown_stem(&entry) else {
					continue;
				};

				resources.push(McpResource::markdown(
					format!("decodex://docs/{lane}/{stem}"),
					format!("docs/{lane}/{stem}.md"),
					"Checked-in Decodex documentation resource.",
				));
			}
		}
		for entry in read_sorted_dir(&self.repo_root.join("docs/research"))? {
			let Some(stem) = json_stem(&entry) else {
				continue;
			};

			resources.push(McpResource::json(
				format!("decodex://research/{stem}"),
				format!("docs/research/{stem}.json"),
				"Checked-in JSON research report.",
			));
		}

		Ok(resources)
	}

	fn decision_contract_resources(&self) -> Result<Vec<McpResource>, McpError> {
		let Some(project_id) = self.project_id.as_deref() else {
			return Ok(Vec::new());
		};
		let Some(state_store) = self.state_store.as_ref() else {
			return Ok(Vec::new());
		};
		let records = state_store
			.list_decision_contracts_for_project(project_id)
			.map_err(McpError::internal)?;

		Ok(records
			.into_iter()
			.map(|record| {
				McpResource::json(
					format!("decodex://decision-contracts/{}", record.contract_id()),
					format!("Decision Contract {}", record.contract_id()),
					"Read-only local runtime Decision Contract readback.",
				)
			})
			.collect())
	}

	fn read_resource(&self, uri: &str) -> Result<ResourceContent, McpError> {
		let resource_uri = ResourceUri::parse(uri)?;

		match resource_uri.host.as_str() {
			DOCS_HOST => self.read_docs_resource(&resource_uri),
			RESEARCH_HOST => self.read_research_resource(&resource_uri),
			DECISION_CONTRACTS_HOST => self.read_decision_contract_resource(&resource_uri),
			PROJECTS_HOST => self.read_project_resource(&resource_uri),
			_ => Err(McpError::resource_not_found()),
		}
	}

	fn read_docs_resource(&self, uri: &ResourceUri) -> Result<ResourceContent, McpError> {
		let path = match uri.segments.as_slice() {
			[segment] if segment == "index" => self.repo_root.join("docs/index.md"),
			[segment] if segment == "policy" => self.repo_root.join("docs/policy.md"),
			[lane, topic] if docs_lane_allowed(lane) && safe_resource_stem(topic) =>
				self.repo_root.join("docs").join(lane).join(format!("{topic}.md")),
			_ => return Err(McpError::resource_not_found()),
		};

		read_file_resource(&uri.raw, path, "text/markdown")
	}

	fn read_research_resource(&self, uri: &ResourceUri) -> Result<ResourceContent, McpError> {
		let [artifact] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if !safe_research_artifact(artifact) {
			return Err(McpError::resource_not_found());
		}

		let file_name = if artifact.ends_with(".json") {
			artifact.to_owned()
		} else {
			format!("{artifact}.json")
		};

		read_file_resource(
			&uri.raw,
			self.repo_root.join("docs/research").join(file_name),
			"application/json",
		)
	}

	fn read_decision_contract_resource(
		&self,
		uri: &ResourceUri,
	) -> Result<ResourceContent, McpError> {
		let [contract_id] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if !safe_runtime_identifier(contract_id) {
			return Err(McpError::resource_not_found());
		}

		let Some(project_id) = self.project_id.as_deref() else {
			return Err(McpError::resource_not_found());
		};
		let Some(state_store) = self.state_store.as_ref() else {
			return Err(McpError::resource_not_found());
		};
		let Some(record) =
			state_store.decision_contract(project_id, contract_id).map_err(McpError::internal)?
		else {
			return Err(McpError::resource_not_found());
		};
		let value = serde_json::json!({
			"schema": "decodex.mcp.decision_contract_resource/1",
			"project_id": record.project_id(),
			"source_issue_id": record.source_issue_id(),
			"status": record.status(),
			"created_at": record.created_at(),
			"updated_at": record.updated_at(),
			"decision_contract": record.contract()
		});

		ResourceContent::json(&uri.raw, value)
	}

	fn read_project_resource(&self, uri: &ResourceUri) -> Result<ResourceContent, McpError> {
		let [project_id, resource_kind, rest @ ..] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if Some(project_id.as_str()) != self.project_id.as_deref() {
			return Err(McpError::resource_not_found());
		}

		let Some(config_path) = self.config_path.as_deref() else {
			return Err(McpError::resource_not_found());
		};
		let value = match (resource_kind.as_str(), rest) {
			("status", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT),
			("lane-control", []) => orchestrator::build_mcp_lane_control_resource(
				Some(config_path),
				None,
				None,
				DEFAULT_MCP_STATUS_LIMIT,
			),
			("lane-control", [issue]) if safe_runtime_identifier(issue) =>
				orchestrator::build_mcp_lane_control_resource(
					Some(config_path),
					Some(issue),
					None,
					DEFAULT_MCP_STATUS_LIMIT,
				),
			_ => return Err(McpError::resource_not_found()),
		}
		.map_err(McpError::internal)?;

		ResourceContent::mcp_observability_json(&uri.raw, value)
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
struct GetPromptParams {
	name: String,
	arguments: Option<Value>,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanToolArgs {
	intent: String,
	issue: Option<String>,
	contract_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LaneControlToolArgs {
	action: String,
	issue: Option<String>,
	run_id: Option<String>,
	expected_turn_id: Option<String>,
	message: Option<String>,
	force: Option<bool>,
	authority: Option<LaneControlAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LaneControlAuthorityArgs {
	reason: Option<String>,
	source: Option<String>,
	inspected_run_id: Option<String>,
	expected_turn_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdminToolArgs {
	action: String,
}

struct McpTool {
	required_profile: McpCapabilityProfile,
	value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct McpResource {
	uri: String,
	name: String,
	description: String,
	mime_type: String,
}
impl McpResource {
	fn markdown(
		uri: impl Into<String>,
		name: impl Into<String>,
		description: impl Into<String>,
	) -> Self {
		Self {
			uri: uri.into(),
			name: name.into(),
			description: description.into(),
			mime_type: String::from("text/markdown"),
		}
	}

	fn json(
		uri: impl Into<String>,
		name: impl Into<String>,
		description: impl Into<String>,
	) -> Self {
		Self {
			uri: uri.into(),
			name: name.into(),
			description: description.into(),
			mime_type: String::from("application/json"),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResourceContent {
	uri: String,
	mime_type: String,
	text: String,
}
impl ResourceContent {
	fn json(uri: &str, value: Value) -> Result<Self, McpError> {
		let text = serde_json::to_string_pretty(&value).map_err(McpError::internal)?;

		Ok(Self { uri: uri.to_owned(), mime_type: String::from("application/json"), text })
	}

	fn mcp_observability_json(uri: &str, mut value: Value) -> Result<Self, McpError> {
		sanitize_mcp_observability_value(&mut value);

		Self::json(uri, value)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResourceUri {
	raw: String,
	host: String,
	segments: Vec<String>,
}
impl ResourceUri {
	fn parse(uri: &str) -> Result<Self, McpError> {
		let parsed = Url::parse(uri).map_err(|_| McpError::invalid_params())?;

		if parsed.scheme() != "decodex" {
			return Err(McpError::invalid_params());
		}

		let host = parsed.host_str().map(str::to_owned).ok_or_else(McpError::invalid_params)?;
		let segments = parsed
			.path_segments()
			.map(|segments| {
				segments
					.filter(|segment| !segment.is_empty())
					.map(str::to_owned)
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();

		Ok(Self { raw: uri.to_owned(), host, segments })
	}
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
pub(crate) fn serve(request: McpServeRequest<'_>) -> Result<()> {
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
	}
}

fn mcp_prompts() -> Vec<Value> {
	vec![
		serde_json::json!({
			"name": "decodex_research",
			"title": "Decodex Research",
			"description": "Frame bounded Decodex research as a latent Decision Contract candidate.",
			"arguments": [
				{
					"name": "intent",
					"description": "Natural-language research question or design uncertainty.",
					"required": true
				}
			]
		}),
		serde_json::json!({
			"name": "decodex_validation_ready",
			"title": "Decodex Validation Ready",
			"description": "Drive an implementation or repair lane to local validation-ready evidence.",
			"arguments": [
				{
					"name": "issue",
					"description": "Linear issue identifier for the lane.",
					"required": true
				},
				{
					"name": "phase",
					"description": "Current Decodex phase goal.",
					"required": false
				}
			]
		}),
		serde_json::json!({
			"name": "decodex_handoff",
			"title": "Decodex Handoff",
			"description": "Prepare a verified review handoff only after local validation and bounded review.",
			"arguments": [
				{
					"name": "issue",
					"description": "Linear issue identifier for the lane.",
					"required": true
				}
			]
		}),
		serde_json::json!({
			"name": "decodex_lane_control",
			"title": "Decodex Lane Control",
			"description": "Inspect first, then request guarded lane-control actions through existing Decodex authority gates.",
			"arguments": [
				{
					"name": "issue",
					"description": "Linear issue identifier or local tracker issue id.",
					"required": true
				},
				{
					"name": "runId",
					"description": "Current run id observed through lane inspect.",
					"required": false
				}
			]
		}),
	]
}

fn mcp_prompt_result(name: &str, arguments: Value) -> Option<Value> {
	let text = match name {
		"decodex_research" => format!(
			"Use Decodex research routing for this intent, keep the result latent until explicitly promoted, and preserve evidence, options, judgment, challenge, decision, validation expectations, and stop conditions.\n\nIntent: {}",
			prompt_argument(&arguments, "intent")?
		),
		"decodex_validation_ready" => format!(
			"Work only to Decodex validation-ready state for issue {}. Implement the smallest coherent code and docs change, run targeted validation, record a current-HEAD docs-impact checkpoint, then complete the active phase goal without push or PR handoff.\n\nPhase: {}",
			prompt_argument(&arguments, "issue")?,
			prompt_argument(&arguments, "phase").unwrap_or("implement_to_validation_ready")
		),
		"decodex_handoff" => format!(
			"Before handoff for issue {}, re-read the current diff and HEAD, run the repo-native bounded review method, require a clean current-head review checkpoint, then use the normal PR-backed Decodex handoff path.",
			prompt_argument(&arguments, "issue")?
		),
		"decodex_lane_control" => format!(
			"Inspect issue {} first. Mutating lane-control tool calls must include the observed run id, current turn preconditions when steering, and explicit authority fields; refuse instead of guessing missing authority.",
			prompt_argument(&arguments, "issue")?
		),
		_ => return None,
	};

	Some(serde_json::json!({
		"description": prompt_description(name),
		"messages": [
			{
				"role": "user",
				"content": {
					"type": "text",
					"text": text
				}
			}
		]
	}))
}

fn prompt_description(name: &str) -> &'static str {
	match name {
		"decodex_research" => "Contract-first bounded Decodex research prompt.",
		"decodex_validation_ready" => "Decodex implementation-phase validation-ready prompt.",
		"decodex_handoff" => "Decodex verified review-handoff prompt.",
		"decodex_lane_control" => "Decodex inspect-first lane-control prompt.",
		_ => "Decodex prompt.",
	}
}

fn prompt_argument<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
	arguments.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
}

fn prompt_required_arguments_are_present(name: &str, arguments: &Value) -> bool {
	let required: &[&str] = match name {
		"decodex_research" => &["intent"],
		"decodex_validation_ready" | "decodex_handoff" | "decodex_lane_control" => &["issue"],
		_ => return true,
	};

	required.iter().all(|key| prompt_argument(arguments, key).is_some())
}

fn mcp_tools() -> Vec<McpTool> {
	vec![
		McpTool {
			required_profile: McpCapabilityProfile::Observe,
			value: mcp_tool_value(
				TOOL_OBSERVE,
				"Decodex Observe",
				"Read public-safe local Decodex runtime observability without private evidence payloads.",
				McpCapabilityProfile::Observe,
				observe_tool_input_schema(),
				observe_tool_output_schema(),
				true,
			),
		},
		McpTool {
			required_profile: McpCapabilityProfile::Plan,
			value: mcp_tool_value(
				TOOL_PLAN,
				"Decodex Plan",
				"Return the Decodex prompt/resource route for a requested workflow intent.",
				McpCapabilityProfile::Plan,
				plan_tool_input_schema(),
				plan_tool_output_schema(),
				true,
			),
		},
		McpTool {
			required_profile: McpCapabilityProfile::Operate,
			value: mcp_tool_value(
				TOOL_LANE_CONTROL,
				"Decodex Lane Control",
				"Inspect a lane or request guarded soft lane-control actions with explicit authority.",
				McpCapabilityProfile::Operate,
				lane_control_tool_input_schema(),
				lane_control_tool_output_schema(),
				false,
			),
		},
		McpTool {
			required_profile: McpCapabilityProfile::Admin,
			value: mcp_tool_value(
				TOOL_ADMIN,
				"Decodex Admin",
				"Read the supported admin MCP policy surface; raw admin mutation is intentionally not exposed.",
				McpCapabilityProfile::Admin,
				admin_tool_input_schema(),
				admin_tool_output_schema(),
				true,
			),
		},
	]
}

fn mcp_tool_value(
	name: &str,
	title: &str,
	description: &str,
	profile: McpCapabilityProfile,
	input_schema: Value,
	output_schema: Value,
	read_only: bool,
) -> Value {
	serde_json::json!({
		"name": name,
		"title": title,
		"description": description,
		"inputSchema": input_schema,
		"outputSchema": output_schema,
		"annotations": {
			"readOnlyHint": read_only,
			"destructiveHint": false,
			"idempotentHint": read_only,
			"openWorldHint": false
		},
		"_meta": {
			"decodex/capabilityProfile": profile.as_str()
		}
	})
}

fn tool_required_profile(name: &str) -> Option<McpCapabilityProfile> {
	match name {
		TOOL_OBSERVE => Some(McpCapabilityProfile::Observe),
		TOOL_PLAN => Some(McpCapabilityProfile::Plan),
		TOOL_LANE_CONTROL => Some(McpCapabilityProfile::Operate),
		TOOL_ADMIN => Some(McpCapabilityProfile::Admin),
		_ => None,
	}
}

fn observe_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"issue": {
				"type": "string",
				"description": "Optional issue identifier or tracker id to inspect one lane."
			},
			"runId": {
				"type": "string",
				"description": "Optional run id used with issue-scoped lane inspection."
			},
			"limit": {
				"type": "integer",
				"minimum": 1,
				"description": "Maximum recent run count for project observability."
			}
		}
	})
}

fn plan_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"intent": {
				"type": "string",
				"enum": ["research", "validation_ready", "handoff", "lane_control"],
				"description": "Decodex workflow intent to route."
			},
			"issue": {
				"type": "string",
				"description": "Optional issue identifier for lane-scoped prompts."
			},
			"contractId": {
				"type": "string",
				"description": "Optional Decision Contract id for research or intake planning."
			}
		},
		"required": ["intent"]
	})
}

fn lane_control_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"action": {
				"type": "string",
				"enum": ["inspect", "interrupt", "steer"]
			},
			"issue": {
				"type": "string",
				"description": "Issue identifier or tracker issue id."
			},
			"runId": {
				"type": "string",
				"description": "Current run id observed through inspect."
			},
			"expectedTurnId": {
				"type": "string",
				"description": "Current turn id required for steer."
			},
			"message": {
				"type": "string",
				"description": "Operator-supplied steer message."
			},
			"force": {
				"type": "boolean",
				"description": "Hard interrupt fallback is not exposed through MCP and is refused when true."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"reason": {
						"type": "string",
						"description": "Explicit operator reason for a mutating lane-control request."
					},
					"source": {
						"type": "string",
						"description": "Remote client or operator source identifier."
					},
					"inspectedRunId": {
						"type": "string",
						"description": "Run id observed through a prior inspect call."
					},
					"expectedTurnId": {
						"type": "string",
						"description": "Turn id observed through inspect and required for steer."
					}
				}
			}
		},
		"required": ["action"]
	})
}

fn admin_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"action": {
				"type": "string",
				"enum": ["capabilities"],
				"description": "Only admin capability readback is exposed by this MCP tool."
			}
		},
		"required": ["action"]
	})
}

fn observe_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.observe_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok"]
			},
			"capability_profile": {
				"type": "string",
				"enum": ["observe"]
			},
			"observability": {
				"type": "object",
				"additionalProperties": true
			}
		},
		"required": ["schema", "status", "capability_profile", "observability"]
	}))
}

fn plan_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.plan_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok"]
			},
			"intent": {
				"type": "string",
				"enum": ["research", "validation_ready", "handoff", "lane_control"]
			},
			"prompt": {
				"type": "string"
			},
			"resource": {
				"type": "string"
			},
			"next_action": {
				"type": "string"
			},
			"issue": {
				"type": ["string", "null"]
			},
			"contract_id": {
				"type": ["string", "null"]
			}
		},
		"required": ["schema", "status", "intent", "prompt", "resource", "next_action"]
	}))
}

fn lane_control_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.lane_control_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["refused"]
			},
			"reason": {
				"type": "string",
				"enum": ["deferred_to_XY-998"]
			},
			"message": {
				"type": "string"
			},
			"capability_profile": {
				"type": "string",
				"enum": ["operate"]
			},
			"action": {
				"type": "string",
				"enum": ["inspect", "interrupt", "steer"]
			},
			"preconditions": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"issue_present": { "type": "boolean" },
					"run_id_present": { "type": "boolean" },
					"expected_turn_id_present": { "type": "boolean" },
					"message_present": { "type": "boolean" },
					"force_requested": { "type": "boolean" },
					"authority_reason_present": { "type": "boolean" },
					"authority_source_present": { "type": "boolean" },
					"authority_inspected_run_id_present": { "type": "boolean" },
					"authority_expected_turn_id_present": { "type": "boolean" }
				},
				"required": [
					"issue_present",
					"run_id_present",
					"expected_turn_id_present",
					"message_present",
					"force_requested",
					"authority_reason_present",
					"authority_source_present",
					"authority_inspected_run_id_present",
					"authority_expected_turn_id_present"
				]
			}
		},
		"required": [
			"schema",
			"status",
			"reason",
			"message",
			"capability_profile",
			"action",
			"preconditions"
		]
	}))
}

fn admin_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.admin_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["refused"]
			},
			"reason": {
				"type": "string",
				"enum": ["deferred_admin_control"]
			},
			"message": {
				"type": "string"
			},
			"capability_profile": {
				"type": "string",
				"enum": ["admin"]
			},
			"action": {
				"type": "string",
				"enum": ["capabilities"]
			},
			"supported_admin_actions": {
				"type": "array",
				"items": {
					"type": "string"
				}
			}
		},
		"required": [
			"schema",
			"status",
			"reason",
			"message",
			"capability_profile",
			"action",
			"supported_admin_actions"
		]
	}))
}

fn tool_output_schema(primary_schema: Value) -> Value {
	serde_json::json!({
		"oneOf": [
			primary_schema,
			tool_refusal_output_schema(),
			tool_validation_error_output_schema()
		]
	})
}

fn tool_refusal_output_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.refusal/1"]
			},
			"status": {
				"type": "string",
				"enum": ["refused"]
			},
			"reason": {
				"type": "string"
			},
			"message": {
				"type": "string"
			},
			"tool": {
				"type": "string"
			},
			"capability_profile": {
				"type": "string",
				"enum": ["observe", "plan", "operate", "admin"]
			},
			"required_capability_profile": {
				"type": "string",
				"enum": ["observe", "plan", "operate", "admin"]
			}
		},
		"required": ["schema", "status", "reason", "message"]
	})
}

fn tool_validation_error_output_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.tool_validation_error/1"]
			},
			"status": {
				"type": "string",
				"enum": ["refused"]
			},
			"reason": {
				"type": "string",
				"enum": ["invalid_arguments"]
			},
			"tool": {
				"type": "string"
			},
			"message": {
				"type": "string"
			}
		},
		"required": ["schema", "status", "reason", "tool", "message"]
	})
}

fn call_plan_tool(arguments: Value) -> Value {
	let params = match serde_json::from_value::<PlanToolArgs>(arguments) {
		Ok(params) => params,
		Err(_) =>
			return invalid_tool_arguments(
				TOOL_PLAN,
				"`intent` is required and must be one of research, validation_ready, handoff, or lane_control.",
			),
	};

	if !matches!(
		params.intent.as_str(),
		"research" | "validation_ready" | "handoff" | "lane_control"
	) {
		return invalid_tool_arguments(
			TOOL_PLAN,
			"`intent` must be one of research, validation_ready, handoff, or lane_control.",
		);
	}

	tool_success(plan_tool_result(&params))
}

fn plan_tool_result(params: &PlanToolArgs) -> Value {
	let (prompt, resource_hint, next_action) = match params.intent.as_str() {
		"research" => (
			"decodex_research",
			"decodex://docs/spec/loop-runtime",
			"Use the research prompt and keep output latent until explicit promotion.",
		),
		"handoff" => (
			"decodex_handoff",
			"decodex://docs/spec/review-orchestration",
			"Run bounded review and repo validation before PR-backed handoff.",
		),
		"lane_control" => (
			"decodex_lane_control",
			"decodex://docs/spec/lane-control",
			"Inspect first; mutating MCP lane-control remains deferred to XY-998.",
		),
		_ => (
			"decodex_validation_ready",
			"decodex://docs/reference/build-test-run",
			"Implement locally, run targeted validation, record docs impact, and complete the phase goal.",
		),
	};

	serde_json::json!({
		"schema": "decodex.mcp.plan_result/1",
		"status": "ok",
		"intent": params.intent.as_str(),
		"prompt": prompt,
		"resource": resource_hint,
		"next_action": next_action,
		"issue": params.issue.as_deref(),
		"contract_id": params.contract_id.as_deref()
	})
}

fn lane_control_stub_result(arguments: Value, profile: McpCapabilityProfile) -> Value {
	let params = match serde_json::from_value::<LaneControlToolArgs>(arguments) {
		Ok(params) => params,
		Err(_) =>
			return invalid_tool_arguments(
				TOOL_LANE_CONTROL,
				"`action` is required and must be one of inspect, interrupt, or steer.",
			),
	};

	if !matches!(params.action.as_str(), "inspect" | "interrupt" | "steer") {
		return invalid_tool_arguments(
			TOOL_LANE_CONTROL,
			"`action` must be one of inspect, interrupt, or steer.",
		);
	}

	tool_refusal_value(serde_json::json!({
		"schema": "decodex.mcp.lane_control_result/1",
		"status": "refused",
		"reason": "deferred_to_XY-998",
		"message": "MCP lane-control mutation is intentionally deferred; this lane only exposes core stdio protocol primitives.",
		"capability_profile": profile.as_str(),
		"action": params.action.as_str(),
		"preconditions": lane_control_preconditions(&params)
	}))
}

fn lane_control_preconditions(params: &LaneControlToolArgs) -> Value {
	let authority = params.authority.as_ref();

	serde_json::json!({
		"issue_present": non_empty_string(params.issue.as_deref()).is_some(),
		"run_id_present": non_empty_string(params.run_id.as_deref()).is_some(),
		"expected_turn_id_present": non_empty_string(params.expected_turn_id.as_deref()).is_some(),
		"message_present": non_empty_string(params.message.as_deref()).is_some(),
		"force_requested": params.force.unwrap_or(false),
		"authority_reason_present": authority
			.and_then(|value| non_empty_string(value.reason.as_deref()))
			.is_some(),
		"authority_source_present": authority
			.and_then(|value| non_empty_string(value.source.as_deref()))
			.is_some(),
		"authority_inspected_run_id_present": authority
			.and_then(|value| non_empty_string(value.inspected_run_id.as_deref()))
			.is_some(),
		"authority_expected_turn_id_present": authority
			.and_then(|value| non_empty_string(value.expected_turn_id.as_deref()))
			.is_some()
	})
}

fn admin_stub_result(arguments: Value, profile: McpCapabilityProfile) -> Value {
	let params = match serde_json::from_value::<AdminToolArgs>(arguments) {
		Ok(params) => params,
		Err(_) =>
			return invalid_tool_arguments(
				TOOL_ADMIN,
				"`action` is required and must be capabilities.",
			),
	};

	if params.action != "capabilities" {
		return invalid_tool_arguments(TOOL_ADMIN, "`action` must be capabilities.");
	}

	tool_refusal_value(serde_json::json!({
		"schema": "decodex.mcp.admin_result/1",
		"status": "refused",
		"reason": "deferred_admin_control",
		"message": "Admin MCP behavior is not implemented in this stdio core-primitives lane.",
		"capability_profile": profile.as_str(),
		"action": params.action.as_str(),
		"supported_admin_actions": []
	}))
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
) -> Result<()>
where
	R: Read,
	W: Write,
{
	let server = McpServer { context, capability_profile };
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

fn write_json_line<W>(writer: &mut W, value: &Value) -> Result<()>
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
) -> Result<Option<PathBuf>> {
	if let Some(path) = explicit_path {
		return Ok(Some(path.to_path_buf()));
	}

	let Some(state_store) = state_store else {
		return Ok(None);
	};

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)
}

fn discover_repo_root_from_current_dir() -> Result<Option<PathBuf>> {
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

fn sanitize_mcp_observability_value(value: &mut Value) {
	match value {
		Value::Object(object) => {
			for key in [
				"worktreePath",
				"worktree_path",
				"channelPath",
				"channel_path",
				"requestPath",
				"request_path",
				"configPath",
				"config_path",
				"repoRoot",
				"repo_root",
				"effectiveCwd",
				"effective_cwd",
				"cwd",
				"privateEvidence",
				"private_evidence",
				"privateEvidenceRef",
				"private_evidence_ref",
				"privateEvidenceRefs",
				"private_evidence_refs",
				"readCommand",
				"read_command",
				"githubCliAuthority",
				"github_cli_authority",
				"githubCommandPath",
				"github_command_path",
				"ghCommandPath",
				"gh_command_path",
				"githubTokenEnvVar",
				"github_token_env_var",
				"path",
			] {
				object.remove(key);
			}
			for child in object.values_mut() {
				sanitize_mcp_observability_value(child);
			}
		},
		Value::Array(items) =>
			for item in items {
				sanitize_mcp_observability_value(item);
			},
		_ => {},
	}
}

fn push_file_resource(
	resources: &mut Vec<McpResource>,
	path: PathBuf,
	uri: &str,
	name: &str,
	description: &str,
) {
	if path.is_file() {
		resources.push(McpResource::markdown(uri, name, description));
	}
}

fn read_sorted_dir(path: &Path) -> Result<Vec<PathBuf>, McpError> {
	let entries = match fs::read_dir(path) {
		Ok(entries) => entries,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
		Err(error) => return Err(McpError::internal(error)),
	};
	let mut paths = entries
		.map(|entry| entry.map(|entry| entry.path()).map_err(McpError::internal))
		.collect::<Result<Vec<_>, _>>()?;

	paths.sort();

	Ok(paths)
}

fn markdown_stem(path: &Path) -> Option<String> {
	if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
		return None;
	}

	path.file_stem().and_then(|stem| stem.to_str()).map(str::to_owned)
}

fn json_stem(path: &Path) -> Option<String> {
	if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
		return None;
	}

	path.file_stem().and_then(|stem| stem.to_str()).map(str::to_owned)
}

fn read_file_resource(
	uri: &str,
	path: PathBuf,
	mime_type: &str,
) -> Result<ResourceContent, McpError> {
	let text = fs::read_to_string(path).map_err(|error| match error.kind() {
		ErrorKind::NotFound => McpError::resource_not_found(),
		_ => McpError::internal(error),
	})?;

	Ok(ResourceContent { uri: uri.to_owned(), mime_type: mime_type.to_owned(), text })
}

fn docs_lane_allowed(lane: &str) -> bool {
	matches!(lane, "spec" | "runbook" | "reference" | "decisions" | "research")
}

fn safe_resource_stem(value: &str) -> bool {
	!value.is_empty()
		&& value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn safe_research_artifact(value: &str) -> bool {
	!value.is_empty()
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
		&& !value.contains("..")
}

fn safe_runtime_identifier(value: &str) -> bool {
	!value.is_empty()
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
		&& !value.contains("..")
}

#[cfg(test)]
mod tests {
	use std::{fs, io::Cursor, path::Path};

	use serde_json::Value;
	use tempfile::TempDir;

	use crate::{
		loop_contract::DecisionContract,
		mcp::{self, McpCapabilityProfile, McpContext, ResourceContent},
		state::StateStore,
	};

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
	fn resources_list_includes_docs_decisions_and_research_json() {
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
		assert!(uri_templates.contains(&"decodex://projects/{project_id}/lane-control/{issue}"));
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
		let plan = tools
			.iter()
			.find(|tool| tool.get("name").and_then(Value::as_str) == Some("decodex_plan"))
			.expect("plan tool should be listed");

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
	fn tools_call_refuses_deferred_lane_control_mutation() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"interrupt","issue":"XY-994","runId":"run-1"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["status"], "refused");
		assert_eq!(result["structuredContent"]["reason"], "deferred_to_XY-998");
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
	fn tools_call_refuses_deferred_admin_operation() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_admin","arguments":{"action":"capabilities"}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.admin_result/1");
		assert_eq!(result["structuredContent"]["status"], "refused");
		assert_eq!(result["structuredContent"]["reason"], "deferred_admin_control");
	}

	#[test]
	fn tools_call_refuses_missing_admin_action() {
		let repo = test_repo();
		let responses = run_stdio(
			repo.path(),
			r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_admin","arguments":{}}}"#,
		);
		let result = &response_at(&responses, 0)["result"];

		assert_eq!(result["isError"], true);
		assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.tool_validation_error/1");
		assert_eq!(result["structuredContent"]["reason"], "invalid_arguments");
		assert_eq!(result["structuredContent"]["tool"], "decodex_admin");
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

	fn test_repo() -> TempDir {
		let repo = TempDir::new().expect("temp repo should exist");

		write_file(repo.path().join("Cargo.toml"), "[workspace]\n");
		write_file(repo.path().join("docs/index.md"), "# Docs\n");
		write_file(repo.path().join("docs/policy.md"), "# Policy\n");
		write_file(repo.path().join("docs/spec/runtime.md"), "# Runtime\n\nSpec body.\n");
		write_file(repo.path().join("docs/decisions/mcp-gateway.md"), "# MCP\n");
		write_file(repo.path().join("docs/research/sample-report.json"), "{}\n");

		repo
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
