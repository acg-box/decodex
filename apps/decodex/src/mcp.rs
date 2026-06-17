use std::{
	collections::BTreeSet,
	env,
	fmt::Display,
	fs,
	io::{self, BufRead as _, BufReader, ErrorKind, Read, Write},
	net::{IpAddr, TcpListener, TcpStream},
	path::{Path, PathBuf},
	str,
	time::Duration,
};

use clap::ValueEnum;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use crate::{config::ServiceConfig, orchestrator, prelude::eyre, runtime, state::StateStore};

/// Safe default listen address for Streamable HTTP MCP.
pub(crate) const DEFAULT_MCP_HTTP_LISTEN_ADDRESS: &str = "127.0.0.1:8193";

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
const MCP_HTTP_ENDPOINT_PATH: &str = "/mcp";
const MCP_HTTP_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MCP_HTTP_MAX_REQUEST_BYTES: usize = 1_024 * 1_024;
const MCP_SESSION_HEADER: &str = "Mcp-Session-Id";
const MCP_CORS_ALLOW_METHODS: &str = "POST, DELETE, OPTIONS";
const MCP_CORS_ALLOW_HEADERS: &str = "Content-Type, Accept, Mcp-Session-Id";

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

	fn list_resources(&self) -> crate::prelude::Result<Value, McpError> {
		let mut resources = self.context.docs_resources()?;

		resources.extend(self.context.decision_contract_resources()?);

		if let Some(project_id) = self.context.project_id() {
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/status"),
				format!("Project {project_id} status"),
				"Read-only local runtime status snapshot.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/status_live"),
				format!("Project {project_id} live status"),
				"Remote-safe status, activity, progress, and lane-control summary.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/activity_tail"),
				format!("Project {project_id} activity tail"),
				"Remote-safe current/recent run activity summary.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/lane-control"),
				format!("Project {project_id} lane-control readback"),
				"Read-only lane-control state for current and recent local lanes.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/pr_review_state"),
				format!("Project {project_id} PR/review state"),
				"Remote-safe PR and review-state readback.",
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
					"uriTemplate": "decodex://projects/{project_id}/status_live",
					"name": "Project live status",
					"description": "Remote-safe current operation, phase, event counts, progress diagnostics, and validation status.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/activity_tail",
					"name": "Project activity tail",
					"description": "Remote-safe activity readback for current and recent runs.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/lane_inspect/{issue}",
					"name": "Lane inspect readback",
					"description": "Read-only lane inspect alias for remote-safe current-lane state.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/lane-control/{issue}",
					"name": "Lane-control readback",
					"description": "Inspect one local lane before requesting guarded lane-control actions.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/runs/{run_id}/events",
					"name": "Run event readback",
					"description": "Remote-safe event counts for a run visible in the current/recent status snapshot.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/runs/{run_id}/protocol_activity",
					"name": "Run protocol activity",
					"description": "Remote-safe protocol activity for a run visible in the current/recent status snapshot, without hidden reasoning or raw payloads.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/runs/{run_id}/child_agent_activity",
					"name": "Run child-agent activity",
					"description": "Remote-safe child-agent activity for a run visible in the current/recent status snapshot.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/runs/{run_id}/progress_diagnostics",
					"name": "Run progress diagnostics",
					"description": "Remote-safe progress and suspected-stall diagnostics for a run visible in the current/recent status snapshot.",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "decodex://projects/{project_id}/pr_review_state",
					"name": "PR/review state",
					"description": "Remote-safe PR and review-state readback.",
					"mimeType": "application/json"
				}
			]
		})
	}

	fn read_resource(&self, params: Option<Value>) -> crate::prelude::Result<Value, McpError> {
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

	fn get_prompt(&self, params: Option<Value>) -> crate::prelude::Result<Value, McpError> {
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

	fn call_tool(&self, params: Option<Value>) -> crate::prelude::Result<Value, McpError> {
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
			.map(mcp_public_lane_inspect_resource)
		} else {
			orchestrator::build_mcp_status_resource(self.context.config_path.as_deref(), limit)
				.map(mcp_status_live_resource)
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

	fn docs_resources(&self) -> crate::prelude::Result<Vec<McpResource>, McpError> {
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

	fn decision_contract_resources(&self) -> crate::prelude::Result<Vec<McpResource>, McpError> {
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

	fn read_resource(&self, uri: &str) -> crate::prelude::Result<ResourceContent, McpError> {
		let resource_uri = ResourceUri::parse(uri)?;

		match resource_uri.host.as_str() {
			DOCS_HOST => self.read_docs_resource(&resource_uri),
			RESEARCH_HOST => self.read_research_resource(&resource_uri),
			DECISION_CONTRACTS_HOST => self.read_decision_contract_resource(&resource_uri),
			PROJECTS_HOST => self.read_project_resource(&resource_uri),
			_ => Err(McpError::resource_not_found()),
		}
	}

	fn read_docs_resource(
		&self,
		uri: &ResourceUri,
	) -> crate::prelude::Result<ResourceContent, McpError> {
		let path = match uri.segments.as_slice() {
			[segment] if segment == "index" => self.repo_root.join("docs/index.md"),
			[segment] if segment == "policy" => self.repo_root.join("docs/policy.md"),
			[lane, topic] if docs_lane_allowed(lane) && safe_resource_stem(topic) =>
				self.repo_root.join("docs").join(lane).join(format!("{topic}.md")),
			_ => return Err(McpError::resource_not_found()),
		};

		read_file_resource(&uri.raw, path, "text/markdown")
	}

	fn read_research_resource(
		&self,
		uri: &ResourceUri,
	) -> crate::prelude::Result<ResourceContent, McpError> {
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
	) -> crate::prelude::Result<ResourceContent, McpError> {
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
		let mut value = serde_json::json!({
			"schema": "decodex.mcp.decision_contract_resource/1",
			"project_id": record.project_id(),
			"source_issue_id": record.source_issue_id(),
			"status": record.status(),
			"created_at": record.created_at(),
			"updated_at": record.updated_at(),
			"decision_contract": record.contract()
		});

		sanitize_mcp_observability_value(&mut value);

		ResourceContent::json(&uri.raw, value)
	}

	fn read_project_resource(
		&self,
		uri: &ResourceUri,
	) -> crate::prelude::Result<ResourceContent, McpError> {
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
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal),
			("status_live", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map(mcp_status_live_resource)
					.map_err(McpError::internal),
			("activity_tail", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map(mcp_activity_tail_resource)
					.map_err(McpError::internal),
			("lane-control", []) => orchestrator::build_mcp_lane_control_resource(
				Some(config_path),
				None,
				None,
				DEFAULT_MCP_STATUS_LIMIT,
			)
			.map(mcp_public_lane_control_readback_resource)
			.map_err(McpError::internal),
			("lane-control", [issue]) if safe_runtime_identifier(issue) =>
				orchestrator::build_mcp_lane_control_resource(
					Some(config_path),
					Some(issue),
					None,
					DEFAULT_MCP_STATUS_LIMIT,
				)
				.map(mcp_public_lane_inspect_resource)
				.map_err(McpError::internal),
			("lane_inspect", [issue]) if safe_runtime_identifier(issue) =>
				orchestrator::build_mcp_lane_control_resource(
					Some(config_path),
					Some(issue),
					None,
					DEFAULT_MCP_STATUS_LIMIT,
				)
				.map(mcp_public_lane_inspect_resource)
				.map_err(McpError::internal),
			("runs", [run_id, resource])
				if resource == "events" && safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| mcp_run_resource(&snapshot, run_id, "events")),
			("runs", [run_id, resource])
				if resource == "protocol_activity" && safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| mcp_run_resource(&snapshot, run_id, "protocol_activity")),
			("runs", [run_id, resource])
				if resource == "child_agent_activity" && safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| {
						mcp_run_resource(&snapshot, run_id, "child_agent_activity")
					}),
			("runs", [run_id, resource])
				if resource == "progress_diagnostics" && safe_runtime_identifier(run_id) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map_err(McpError::internal)
					.and_then(|snapshot| {
						mcp_run_resource(&snapshot, run_id, "progress_diagnostics")
					}),
			("pr_review_state", []) =>
				orchestrator::build_mcp_status_resource(Some(config_path), DEFAULT_MCP_STATUS_LIMIT)
					.map(mcp_pr_review_state_resource)
					.map_err(McpError::internal),
			_ => return Err(McpError::resource_not_found()),
		}?;

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
	fn json(uri: &str, value: Value) -> crate::prelude::Result<Self, McpError> {
		let text = serde_json::to_string_pretty(&value).map_err(McpError::internal)?;

		Ok(Self { uri: uri.to_owned(), mime_type: String::from("application/json"), text })
	}

	fn mcp_observability_json(
		uri: &str,
		mut value: Value,
	) -> crate::prelude::Result<Self, McpError> {
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
	fn parse(uri: &str) -> crate::prelude::Result<Self, McpError> {
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

struct McpHttpHandler {
	server: McpServer,
	sessions: McpHttpSessions,
	allowed_origins: Vec<String>,
	listen_address: Option<String>,
}
impl McpHttpHandler {
	fn handle_request_bytes(&mut self, request: &[u8]) -> crate::prelude::Result<Vec<u8>> {
		let request = match McpHttpRequest::parse(request) {
			Ok(request) => request,
			Err(response) => return response.into_bytes(),
		};
		let response = self.handle_request(request)?;

		response.into_bytes()
	}

	fn handle_request(
		&mut self,
		request: McpHttpRequest,
	) -> crate::prelude::Result<McpHttpResponse> {
		let cors_origin = match self.allowed_cors_origin(&request) {
			Ok(origin) => origin,
			Err(()) =>
				return Ok(McpHttpResponse::json_error(
					"403 Forbidden",
					json_rpc_error(Value::Null, -32_000, "Forbidden origin"),
				)),
		};
		let mut response = if request.path != MCP_HTTP_ENDPOINT_PATH {
			McpHttpResponse::empty("404 Not Found")
		} else {
			match request.method.as_str() {
				"OPTIONS" => self.handle_options(&request),
				"POST" => self.handle_post(request)?,
				"DELETE" => self.handle_delete(&request),
				_ => McpHttpResponse::empty("405 Method Not Allowed"),
			}
		};

		response.add_cors_headers(cors_origin);

		Ok(response)
	}

	fn handle_options(&self, request: &McpHttpRequest) -> McpHttpResponse {
		let Some(method) = request.header("Access-Control-Request-Method") else {
			return McpHttpResponse::empty("204 No Content");
		};

		if matches!(method.to_ascii_uppercase().as_str(), "POST" | "DELETE") {
			McpHttpResponse::empty("204 No Content")
		} else {
			McpHttpResponse::empty("405 Method Not Allowed")
		}
	}

	fn handle_post(&mut self, request: McpHttpRequest) -> crate::prelude::Result<McpHttpResponse> {
		if !request.content_type_is_json() {
			return Ok(McpHttpResponse::json_error(
				"415 Unsupported Media Type",
				json_rpc_error(Value::Null, -32_600, "Invalid Request"),
			));
		}

		let body = match str::from_utf8(&request.body) {
			Ok(body) => body,
			Err(_) =>
				return Ok(McpHttpResponse::json_error(
					"400 Bad Request",
					json_rpc_error(Value::Null, -32_700, "Parse error"),
				)),
		};
		let method = json_rpc_method_name(body);
		let is_initialize = method.as_deref() == Some("initialize");
		let session_id = request.header(MCP_SESSION_HEADER).map(str::to_owned);

		if method.is_none() && serde_json::from_str::<Value>(body).is_err() {
			return McpHttpResponse::json(
				self.server
					.handle_line(body, false)
					.into_iter()
					.next()
					.unwrap_or_else(|| json_rpc_error(Value::Null, -32_700, "Parse error")),
				None,
			);
		}

		let wants_sse = request.accepts_sse();

		if is_initialize {
			let responses = self.server.handle_line(body, wants_sse);
			let response_session_id =
				initialize_response_succeeded(&responses).then(|| self.sessions.create());

			return mcp_http_response_for_server_responses(
				responses,
				wants_sse,
				response_session_id,
			);
		}

		let Some(session_id) = session_id.as_deref() else {
			return Ok(McpHttpResponse::json_error(
				"428 Precondition Required",
				json_rpc_error(Value::Null, -32_000, "Missing MCP session"),
			));
		};

		if !self.sessions.contains(session_id) {
			return Ok(McpHttpResponse::json_error(
				"404 Not Found",
				json_rpc_error(Value::Null, -32_001, "Unknown MCP session"),
			));
		}

		let responses = self.server.handle_line(body, wants_sse);

		mcp_http_response_for_server_responses(responses, wants_sse, Some(session_id.to_owned()))
	}

	fn handle_delete(&mut self, request: &McpHttpRequest) -> McpHttpResponse {
		let Some(session_id) = request.header(MCP_SESSION_HEADER) else {
			return McpHttpResponse::json_error(
				"428 Precondition Required",
				json_rpc_error(Value::Null, -32_000, "Missing MCP session"),
			);
		};

		if !self.sessions.remove(session_id) {
			return McpHttpResponse::json_error(
				"404 Not Found",
				json_rpc_error(Value::Null, -32_001, "Unknown MCP session"),
			);
		}

		McpHttpResponse::empty("202 Accepted")
	}

	fn allowed_cors_origin(
		&self,
		request: &McpHttpRequest,
	) -> std::result::Result<Option<String>, ()> {
		let Some(origin) = request.header("Origin") else {
			return Ok(None);
		};

		if mcp_http_origin_is_allowed(
			origin,
			self.listen_address.as_deref(),
			self.allowed_origins.as_slice(),
		) {
			Ok(Some(origin.to_owned()))
		} else {
			Err(())
		}
	}
}

#[derive(Default)]
struct McpHttpSessions {
	active: BTreeSet<String>,
	next_id: u64,
}
impl McpHttpSessions {
	fn create(&mut self) -> String {
		self.next_id = self.next_id.saturating_add(1);

		let session_id = format!("decodex-mcp-session-{:016x}", self.next_id);

		self.active.insert(session_id.clone());

		session_id
	}

	fn contains(&self, session_id: &str) -> bool {
		self.active.contains(session_id)
	}

	fn remove(&mut self, session_id: &str) -> bool {
		self.active.remove(session_id)
	}
}

struct McpHttpRequest {
	method: String,
	path: String,
	headers: Vec<(String, String)>,
	body: Vec<u8>,
}
impl McpHttpRequest {
	fn parse(request: &[u8]) -> std::result::Result<Self, McpHttpResponse> {
		let Some(header_end) = http_header_end(request) else {
			return Err(McpHttpResponse::empty("400 Bad Request"));
		};
		let header_text = str::from_utf8(&request[..header_end])
			.map_err(|_| McpHttpResponse::empty("400 Bad Request"))?;
		let mut lines = header_text.split("\r\n");
		let request_line = lines.next().ok_or_else(|| McpHttpResponse::empty("400 Bad Request"))?;
		let mut request_parts = request_line.split_whitespace();
		let method = request_parts
			.next()
			.ok_or_else(|| McpHttpResponse::empty("400 Bad Request"))?
			.to_owned();
		let path = request_parts
			.next()
			.ok_or_else(|| McpHttpResponse::empty("400 Bad Request"))?
			.to_owned();
		let version =
			request_parts.next().ok_or_else(|| McpHttpResponse::empty("400 Bad Request"))?;

		if !version.starts_with("HTTP/1.") {
			return Err(McpHttpResponse::empty("505 HTTP Version Not Supported"));
		}

		let mut headers = Vec::new();

		for line in lines {
			if line.is_empty() {
				continue;
			}

			let Some((name, value)) = line.split_once(':') else {
				return Err(McpHttpResponse::empty("400 Bad Request"));
			};

			headers.push((name.trim().to_owned(), value.trim().to_owned()));
		}

		let content_length = http_content_length(&request[..header_end])
			.map_err(|_| McpHttpResponse::empty("400 Bad Request"))?;
		let body_start = header_end + 4;
		let body_end = body_start.saturating_add(content_length);

		if request.len() < body_end {
			return Err(McpHttpResponse::empty("400 Bad Request"));
		}

		Ok(Self { method, path, headers, body: request[body_start..body_end].to_vec() })
	}

	fn header(&self, name: &str) -> Option<&str> {
		self.headers
			.iter()
			.find(|(header, _)| header.eq_ignore_ascii_case(name))
			.map(|(_, value)| value.as_str())
	}

	fn accepts_sse(&self) -> bool {
		header_contains(self.header("Accept"), "text/event-stream")
	}

	fn content_type_is_json(&self) -> bool {
		header_contains(self.header("Content-Type"), "application/json")
	}
}

struct McpHttpResponse {
	status: &'static str,
	content_type: Option<&'static str>,
	headers: Vec<(&'static str, String)>,
	body: Vec<u8>,
}
impl McpHttpResponse {
	fn empty(status: &'static str) -> Self {
		Self { status, content_type: None, headers: Vec::new(), body: Vec::new() }
	}

	fn empty_with_session(status: &'static str, session_id: Option<String>) -> Self {
		let mut response = Self::empty(status);

		response.add_session_header(session_id);

		response
	}

	fn json(value: Value, session_id: Option<String>) -> crate::prelude::Result<Self> {
		let body = serde_json::to_vec(&value)?;
		let mut response = Self {
			status: "200 OK",
			content_type: Some("application/json"),
			headers: vec![("Cache-Control", String::from("no-store"))],
			body,
		};

		response.add_session_header(session_id);

		Ok(response)
	}

	fn json_error(status: &'static str, value: Value) -> Self {
		let body =
			serde_json::to_vec(&value)
				.unwrap_or_else(|_| {
					br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error"}}"#.to_vec()
				});

		Self {
			status,
			content_type: Some("application/json"),
			headers: vec![("Cache-Control", String::from("no-store"))],
			body,
		}
	}

	fn sse(responses: Vec<Value>, session_id: Option<String>) -> crate::prelude::Result<Self> {
		let mut body = Vec::new();

		for response in responses {
			let line = serde_json::to_string(&response)?;

			body.extend_from_slice(b"event: message\n");
			body.extend_from_slice(b"data: ");
			body.extend_from_slice(line.as_bytes());
			body.extend_from_slice(b"\n\n");
		}

		let mut response = Self {
			status: "200 OK",
			content_type: Some("text/event-stream"),
			headers: vec![
				("Cache-Control", String::from("no-store")),
				("X-Accel-Buffering", String::from("no")),
			],
			body,
		};

		response.add_session_header(session_id);

		Ok(response)
	}

	fn add_session_header(&mut self, session_id: Option<String>) {
		if let Some(session_id) = session_id {
			self.headers.push((MCP_SESSION_HEADER, session_id));
		}
	}

	fn add_cors_headers(&mut self, origin: Option<String>) {
		let Some(origin) = origin else {
			return;
		};

		self.headers.push(("Access-Control-Allow-Origin", origin));
		self.headers.push(("Vary", String::from("Origin")));
		self.headers.push(("Access-Control-Allow-Methods", String::from(MCP_CORS_ALLOW_METHODS)));
		self.headers.push(("Access-Control-Allow-Headers", String::from(MCP_CORS_ALLOW_HEADERS)));
		self.headers.push(("Access-Control-Expose-Headers", String::from(MCP_SESSION_HEADER)));
	}

	fn into_bytes(self) -> crate::prelude::Result<Vec<u8>> {
		let mut response = Vec::new();

		response.extend_from_slice(format!("HTTP/1.1 {}\r\n", self.status).as_bytes());
		response.extend_from_slice(b"Connection: close\r\n");
		response.extend_from_slice(format!("Content-Length: {}\r\n", self.body.len()).as_bytes());

		if let Some(content_type) = self.content_type {
			response.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
		}

		for (name, value) in self.headers {
			response.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
		}

		response.extend_from_slice(b"\r\n");
		response.extend_from_slice(&self.body);

		Ok(response)
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
			validate_mcp_http_listen_address(request.listen_address, request.allowed_origins)?;

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
			)
		},
	}
}

fn mcp_http_response_for_server_responses(
	responses: Vec<Value>,
	wants_sse: bool,
	session_id: Option<String>,
) -> crate::prelude::Result<McpHttpResponse> {
	if responses.is_empty() {
		return Ok(McpHttpResponse::empty_with_session("202 Accepted", session_id));
	}
	if wants_sse {
		return McpHttpResponse::sse(responses, session_id);
	}

	McpHttpResponse::json(
		responses.into_iter().next().unwrap_or_else(|| serde_json::json!({})),
		session_id,
	)
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

fn mcp_status_live_resource(snapshot: Value) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.status_live/1",
		"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
		"status_source": snapshot.get("status_source").cloned().unwrap_or(Value::Null),
		"run_limit": snapshot.get("run_limit").cloned().unwrap_or(Value::Null),
		"current_lanes": mcp_run_activity_summaries(snapshot.get("current_lanes")),
		"recent_runs": mcp_run_activity_summaries(snapshot.get("recent_runs")),
		"post_review_lanes": mcp_public_post_review_lanes(snapshot.get("post_review_lanes"))
	})
}

fn mcp_activity_tail_resource(snapshot: Value) -> Value {
	let mut activity = Vec::new();

	for run in mcp_all_runs(&snapshot) {
		activity.push(mcp_run_activity_summary(run));
	}

	serde_json::json!({
		"schema": "decodex.mcp.activity_tail/1",
		"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
		"activity": activity
	})
}

fn mcp_public_lane_control_readback_resource(readback: Value) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.lane_control_readback/1",
		"project_id": readback.get("project_id").cloned().unwrap_or(Value::Null),
		"read_only": readback.get("read_only").cloned().unwrap_or(Value::Null),
		"mutating_tools": readback.get("mutating_tools").cloned().unwrap_or_else(|| serde_json::json!([])),
		"current_lanes": mcp_run_activity_summaries(readback.get("current_lanes")),
		"recent_runs": mcp_run_activity_summaries(readback.get("recent_runs")),
		"post_review_lanes": mcp_public_post_review_lanes(readback.get("post_review_lanes"))
	})
}

fn mcp_public_lane_inspect_resource(report: Value) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.lane_inspect/1",
		"projectId": report.get("projectId").cloned().unwrap_or(Value::Null),
		"issue": report.get("issue").cloned().unwrap_or(Value::Null),
		"runId": report.get("runId").cloned().unwrap_or(Value::Null),
		"matchedRunCount": report.get("matchedRunCount").cloned().unwrap_or(Value::Null),
		"runs": mcp_public_lane_inspect_runs(report.get("runs"))
	})
}

fn mcp_public_lane_inspect_runs(runs: Option<&Value>) -> Vec<Value> {
	runs.and_then(Value::as_array).into_iter().flatten().map(mcp_public_lane_inspect_run).collect()
}

fn mcp_public_lane_inspect_run(run: &Value) -> Value {
	serde_json::json!({
		"projectId": run.get("projectId").cloned().unwrap_or(Value::Null),
		"issueId": run.get("issueId").cloned().unwrap_or(Value::Null),
		"issueIdentifier": run.get("issueIdentifier").cloned().unwrap_or(Value::Null),
		"runId": run.get("runId").cloned().unwrap_or(Value::Null),
		"attemptNumber": run.get("attemptNumber").cloned().unwrap_or(Value::Null),
		"status": run.get("status").cloned().unwrap_or(Value::Null),
		"attemptStatus": run.get("attemptStatus").cloned().unwrap_or(Value::Null),
		"phase": run.get("phase").cloned().unwrap_or(Value::Null),
		"waitReason": run.get("waitReason").cloned().unwrap_or(Value::Null),
		"currentOperation": run.get("currentOperation").cloned().unwrap_or(Value::Null),
		"laneControlNextAction": run
			.get("laneControlNextAction")
			.cloned()
			.unwrap_or(Value::Null),
		"laneControlConditions": run
			.get("laneControlConditions")
			.cloned()
			.unwrap_or_else(|| serde_json::json!([])),
		"lastEventType": run.get("lastEventType").cloned().unwrap_or(Value::Null),
		"lastEventAt": run.get("lastEventAt").cloned().unwrap_or(Value::Null),
		"eventCount": run.get("eventCount").cloned().unwrap_or(Value::Null)
	})
}

fn mcp_run_resource(snapshot: &Value, run_id: &str, kind: &str) -> Result<Value, McpError> {
	let Some(run) = mcp_find_run(snapshot, run_id) else {
		return Err(McpError::resource_not_found());
	};
	let value = match kind {
		"events" => serde_json::json!({
			"schema": "decodex.mcp.run_events/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"issue_id": run.get("issue_id").cloned().unwrap_or(Value::Null),
			"issue_identifier": run.get("issue_identifier").cloned().unwrap_or(Value::Null),
			"event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
			"last_event_type": run.get("last_event_type").cloned().unwrap_or(Value::Null),
			"last_event_at": run.get("last_event_at").cloned().unwrap_or(Value::Null),
			"last_protocol_activity_at": run
				.get("last_protocol_activity_at")
				.cloned()
				.unwrap_or(Value::Null)
		}),
		"protocol_activity" => serde_json::json!({
			"schema": "decodex.mcp.protocol_activity/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"protocol_activity": mcp_public_protocol_activity(run),
			"event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
			"last_event_type": run.get("last_event_type").cloned().unwrap_or(Value::Null)
		}),
		"child_agent_activity" => serde_json::json!({
			"schema": "decodex.mcp.child_agent_activity/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"child_agent_activity": run.get("child_agent_activity").cloned().unwrap_or(Value::Null)
		}),
		"progress_diagnostics" => serde_json::json!({
			"schema": "decodex.mcp.progress_diagnostics/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"phase": run.get("phase").cloned().unwrap_or(Value::Null),
			"run_phase": run.get("run_phase").cloned().unwrap_or(Value::Null),
			"current_operation": run.get("current_operation").cloned().unwrap_or(Value::Null),
			"last_progress_at": run.get("last_progress_at").cloned().unwrap_or(Value::Null),
			"progress_diagnostic": run.get("progress_diagnostic").cloned().unwrap_or(Value::Null),
			"suspected_stall": run.get("suspected_stall").cloned().unwrap_or(Value::Null)
		}),
		_ => unreachable!("MCP run resource kind is selected by static match arms"),
	};

	Ok(value)
}

fn mcp_pr_review_state_resource(snapshot: Value) -> Value {
	let review_lanes = mcp_public_post_review_lanes(snapshot.get("post_review_lanes"));
	let current_lane_reviews = mcp_current_lane_runs(&snapshot)
		.into_iter()
		.filter_map(|run| {
			let review = run.get("loop_status")?.get("review")?;

			Some(serde_json::json!({
				"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
				"issue_id": run.get("issue_id").cloned().unwrap_or(Value::Null),
				"issue_identifier": run.get("issue_identifier").cloned().unwrap_or(Value::Null),
				"review": mcp_public_review_status(review)
			}))
		})
		.collect::<Vec<_>>();

	serde_json::json!({
		"schema": "decodex.mcp.pr_review_state/1",
		"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
		"post_review_lanes": review_lanes,
		"current_lane_reviews": current_lane_reviews
	})
}

fn mcp_run_activity_summaries(runs: Option<&Value>) -> Vec<Value> {
	runs.and_then(Value::as_array).into_iter().flatten().map(mcp_run_activity_summary).collect()
}

fn mcp_public_post_review_lanes(lanes: Option<&Value>) -> Vec<Value> {
	lanes.and_then(Value::as_array).into_iter().flatten().map(mcp_public_post_review_lane).collect()
}

fn mcp_public_post_review_lane(lane: &Value) -> Value {
	serde_json::json!({
		"project_id": lane.get("project_id").cloned().unwrap_or(Value::Null),
		"issue_id": lane.get("issue_id").cloned().unwrap_or(Value::Null),
		"issue_identifier": lane.get("issue_identifier").cloned().unwrap_or(Value::Null),
		"issue_state": lane.get("issue_state").cloned().unwrap_or(Value::Null),
		"classification": lane.get("classification").cloned().unwrap_or(Value::Null),
		"reason": lane.get("reason").cloned().unwrap_or(Value::Null),
		"pr_url": lane.get("pr_url").cloned().unwrap_or(Value::Null),
		"pr_state": lane.get("pr_state").cloned().unwrap_or(Value::Null),
		"review_decision": lane.get("review_decision").cloned().unwrap_or(Value::Null),
		"mergeable": lane.get("mergeable").cloned().unwrap_or(Value::Null),
		"check_state": lane.get("check_state").cloned().unwrap_or(Value::Null),
		"unresolved_review_threads": lane
			.get("unresolved_review_threads")
			.cloned()
			.unwrap_or(Value::Null),
		"shadowed_by_current_lane": lane
			.get("shadowed_by_current_lane")
			.cloned()
			.unwrap_or(Value::Null),
		"readback_warning": lane.get("readback_warning").cloned().unwrap_or(Value::Null),
		"readback_root_cause": lane.get("readback_root_cause").cloned().unwrap_or(Value::Null),
		"loop_review": lane
			.get("loop_status")
			.and_then(|status| status.get("review"))
			.map(mcp_public_review_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_all_runs(snapshot: &Value) -> Vec<&Value> {
	let mut runs = Vec::new();
	let mut seen_run_ids = BTreeSet::new();

	for key in ["current_lanes", "recent_runs"] {
		if let Some(items) = snapshot.get(key).and_then(Value::as_array) {
			for (index, run) in items.iter().enumerate() {
				let run_key = run
					.get("run_id")
					.and_then(Value::as_str)
					.map(str::to_owned)
					.unwrap_or_else(|| format!("{key}:{index}"));

				if seen_run_ids.insert(run_key) {
					runs.push(run);
				}
			}
		}
	}

	runs
}

fn mcp_current_lane_runs(snapshot: &Value) -> Vec<&Value> {
	snapshot.get("current_lanes").and_then(Value::as_array).into_iter().flatten().collect()
}

fn mcp_find_run<'a>(snapshot: &'a Value, run_id: &str) -> Option<&'a Value> {
	mcp_all_runs(snapshot)
		.into_iter()
		.find(|run| run.get("run_id").and_then(Value::as_str) == Some(run_id))
}

fn mcp_run_activity_summary(run: &Value) -> Value {
	serde_json::json!({
		"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
		"issue_id": run.get("issue_id").cloned().unwrap_or(Value::Null),
		"issue_identifier": run.get("issue_identifier").cloned().unwrap_or(Value::Null),
		"attempt_number": run.get("attempt_number").cloned().unwrap_or(Value::Null),
		"status": run.get("status").cloned().unwrap_or(Value::Null),
		"attempt_status": run.get("attempt_status").cloned().unwrap_or(Value::Null),
		"phase": run.get("phase").cloned().unwrap_or(Value::Null),
		"run_phase": run.get("run_phase").cloned().unwrap_or(Value::Null),
		"wait_reason": run.get("wait_reason").cloned().unwrap_or(Value::Null),
		"current_operation": run.get("current_operation").cloned().unwrap_or(Value::Null),
		"lane_control_next_action": run
			.get("lane_control_next_action")
			.cloned()
			.unwrap_or(Value::Null),
		"event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
		"last_event_type": run.get("last_event_type").cloned().unwrap_or(Value::Null),
		"last_event_at": run.get("last_event_at").cloned().unwrap_or(Value::Null),
		"last_protocol_activity_at": run
			.get("last_protocol_activity_at")
			.cloned()
			.unwrap_or(Value::Null),
		"last_progress_at": run.get("last_progress_at").cloned().unwrap_or(Value::Null),
		"protocol_activity": mcp_public_protocol_activity(run),
		"child_agent_activity": run.get("child_agent_activity").cloned().unwrap_or(Value::Null),
		"progress_diagnostic": run.get("progress_diagnostic").cloned().unwrap_or(Value::Null),
		"phase_acceptance": run
			.get("phase_acceptance")
			.map(mcp_public_phase_acceptance_status)
			.unwrap_or(Value::Null),
		"loop_review": run
			.get("loop_status")
			.and_then(|status| status.get("review"))
			.map(mcp_public_review_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_public_phase_acceptance_status(acceptance: &Value) -> Value {
	serde_json::json!({
		"phase": acceptance.get("phase").cloned().unwrap_or(Value::Null),
		"decision": acceptance.get("decision").cloned().unwrap_or(Value::Null),
		"reason_code": acceptance.get("reason_code").cloned().unwrap_or(Value::Null),
		"objective_covered": acceptance.get("objective_covered").cloned().unwrap_or(Value::Null),
		"effective_delta_present": acceptance
			.get("effective_delta_present")
			.cloned()
			.unwrap_or(Value::Null),
		"non_goal_passed": acceptance.get("non_goal_passed").cloned().unwrap_or(Value::Null),
		"validation_passed": acceptance.get("validation_passed").cloned().unwrap_or(Value::Null),
		"next_action": acceptance.get("next_action").cloned().unwrap_or(Value::Null)
	})
}

fn mcp_public_review_status(review: &Value) -> Value {
	serde_json::json!({
		"phase": review.get("phase").cloned().unwrap_or(Value::Null),
		"status": review.get("status").cloned().unwrap_or(Value::Null),
		"checkpoint": review
			.get("checkpoint")
			.map(mcp_public_review_checkpoint_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_public_review_checkpoint_status(checkpoint: &Value) -> Value {
	serde_json::json!({
		"round": checkpoint.get("round").cloned().unwrap_or(Value::Null),
		"nonclean_rounds": checkpoint.get("nonclean_rounds").cloned().unwrap_or(Value::Null),
		"updated_at": checkpoint.get("updated_at").cloned().unwrap_or(Value::Null)
	})
}

fn mcp_public_protocol_activity(run: &Value) -> Value {
	let mut activity = run.get("protocol_activity").cloned().unwrap_or(Value::Null);

	redact_reasoning_protocol_activity(&mut activity);

	activity
}

fn redact_reasoning_protocol_activity(value: &mut Value) {
	match value {
		Value::Object(object) => {
			let is_reasoning_event = object
				.get("category")
				.and_then(Value::as_str)
				.is_some_and(|category| category.eq_ignore_ascii_case("reasoning"))
				|| object.get("event_type").and_then(Value::as_str).is_some_and(|event_type| {
					event_type.to_ascii_lowercase().contains("reasoning")
				});

			if is_reasoning_event {
				object.insert(
					String::from("detail"),
					Value::String(String::from("redacted_reasoning")),
				);
				object.remove("text");
				object.remove("summary");
				object.remove("content");
				object.remove("body");
			}

			for child in object.values_mut() {
				redact_reasoning_protocol_activity(child);
			}
		},
		Value::Array(items) =>
			for item in items {
				redact_reasoning_protocol_activity(item);
			},
		_ => {},
	}
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
		"message": "MCP lane-control mutation is intentionally deferred; this gateway currently exposes discovery, observability, and structured refusal surfaces.",
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
		"message": "Admin MCP behavior is not implemented in this gateway lane.",
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

fn serve_streamable_http_with_profile(
	listener: TcpListener,
	context: McpContext,
	capability_profile: McpCapabilityProfile,
	allowed_origins: Vec<String>,
) -> crate::prelude::Result<()> {
	let mut handler = McpHttpHandler {
		server: McpServer { context, capability_profile, transport: McpTransport::StreamableHttp },
		sessions: McpHttpSessions::default(),
		allowed_origins,
		listen_address: listener.local_addr().map(|address| address.to_string()).ok(),
	};

	for stream in listener.incoming() {
		match stream {
			Ok(mut stream) =>
				if let Err(error) = handle_mcp_http_stream(&mut stream, &mut handler) {
					tracing::warn!(?error, "Decodex MCP Streamable HTTP request failed.");
				},
			Err(error) if error.kind() == ErrorKind::Interrupted => continue,
			Err(error) => return Err(error.into()),
		}
	}

	Ok(())
}

fn handle_mcp_http_stream(
	stream: &mut TcpStream,
	handler: &mut McpHttpHandler,
) -> crate::prelude::Result<()> {
	stream.set_read_timeout(Some(MCP_HTTP_READ_TIMEOUT))?;

	let request = read_mcp_http_request(stream)?;
	let response = handler.handle_request_bytes(&request)?;

	stream.write_all(&response)?;
	stream.flush()?;

	Ok(())
}

fn read_mcp_http_request(stream: &mut TcpStream) -> crate::prelude::Result<Vec<u8>> {
	let mut buffer = Vec::new();
	let mut scratch = [0_u8; 1_024];
	let mut expected_len = None;

	loop {
		let read = stream.read(&mut scratch)?;

		if read == 0 {
			break;
		}

		buffer.extend_from_slice(&scratch[..read]);

		if buffer.len() > MCP_HTTP_MAX_REQUEST_BYTES {
			eyre::bail!("MCP HTTP request exceeded {MCP_HTTP_MAX_REQUEST_BYTES} bytes.");
		}
		if expected_len.is_none()
			&& let Some(header_end) = http_header_end(&buffer)
		{
			let content_length = http_content_length(&buffer[..header_end])?;

			expected_len = Some(header_end + 4 + content_length);
		}
		if expected_len.is_some_and(|length| buffer.len() >= length) {
			break;
		}
	}

	Ok(buffer)
}

fn validate_mcp_http_listen_address(
	address: &str,
	allowed_origins: &[String],
) -> crate::prelude::Result<()> {
	if listen_address_host_is_loopback(address) || !allowed_origins.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"Refusing to bind Decodex MCP Streamable HTTP to `{address}` without --allow-origin; use the loopback default or set explicit trusted origins."
	)
}

fn listen_address_host_is_loopback(address: &str) -> bool {
	let host = listen_address_host(address);

	host.as_deref().is_some_and(host_is_loopback)
}

fn mcp_http_origin_is_allowed(
	origin: &str,
	listen_address: Option<&str>,
	allowed_origins: &[String],
) -> bool {
	if allowed_origins.iter().any(|allowed| allowed == origin) {
		return true;
	}

	let Ok(parsed) = Url::parse(origin) else {
		return false;
	};
	let Some(host) = parsed.host_str() else {
		return false;
	};

	if !matches!(parsed.scheme(), "http" | "https") || !host_is_loopback(host) {
		return false;
	}

	let Some(listen_port) = listen_address.and_then(listen_address_port) else {
		return true;
	};

	parsed.port_or_known_default() == Some(listen_port)
}

fn host_is_loopback(host: &str) -> bool {
	host.eq_ignore_ascii_case("localhost")
		|| host
			.trim_matches(['[', ']'])
			.parse::<IpAddr>()
			.is_ok_and(|address| address.is_loopback())
}

fn listen_address_host(address: &str) -> Option<String> {
	let (host, _) = address.rsplit_once(':')?;

	Some(host.trim_matches(['[', ']']).to_owned())
}

fn listen_address_port(address: &str) -> Option<u16> {
	let (_, port) = address.rsplit_once(':')?;

	port.parse().ok()
}

fn http_header_end(bytes: &[u8]) -> Option<usize> {
	bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn http_content_length(header_bytes: &[u8]) -> crate::prelude::Result<usize> {
	let header_text = str::from_utf8(header_bytes)?;

	for line in header_text.split("\r\n").skip(1) {
		let Some((name, value)) = line.split_once(':') else {
			continue;
		};

		if name.trim().eq_ignore_ascii_case("Content-Length") {
			return Ok(value.trim().parse()?);
		}
	}

	Ok(0)
}

fn header_contains(header: Option<&str>, value: &str) -> bool {
	header
		.map(|header| {
			header.split(',').any(|item| {
				item.trim().split(';').next().is_some_and(|item| item.eq_ignore_ascii_case(value))
			})
		})
		.unwrap_or(false)
}

fn json_rpc_method_name(body: &str) -> Option<String> {
	serde_json::from_str::<Value>(body)
		.ok()
		.and_then(|value| value.get("method").and_then(Value::as_str).map(str::to_owned))
}

fn initialize_response_succeeded(responses: &[Value]) -> bool {
	responses.iter().any(|response| {
		response.get("error").is_none()
			&& response
				.get("result")
				.and_then(|result| result.get("protocolVersion"))
				.and_then(Value::as_str)
				== Some(MCP_PROTOCOL_VERSION)
	})
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
				"executionProgramId",
				"execution_program_id",
				"executionProgramNodeIds",
				"execution_program_node_ids",
				"graphId",
				"graph_id",
				"nodeId",
				"node_id",
				"programId",
				"program_id",
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
		Value::String(text) =>
			if observability_string_contains_sensitive_text(text) {
				*text = String::from("redacted_sensitive_detail");
			},
		Value::Array(items) =>
			for item in items {
				sanitize_mcp_observability_value(item);
			},
		_ => {},
	}
}

fn observability_string_contains_sensitive_text(value: &str) -> bool {
	let lower = value.to_ascii_lowercase();
	let upper = value.to_ascii_uppercase();

	lower.contains("/private")
		|| lower.contains("/users/")
		|| lower.contains("/var/folders/")
		|| lower.contains("/tmp/")
		|| lower.contains("file://")
		|| observability_string_contains_absolute_path(value)
		|| observability_string_contains_windows_path(value)
		|| observability_string_contains_secret_like_token(value)
		|| upper.contains("GITHUB_PAT_")
		|| upper.contains("LINEAR_API_KEY")
		|| upper.contains("OPENAI_API_KEY")
		|| lower.contains("authorization:")
		|| lower.contains("bearer ")
		|| lower.contains("token=")
		|| lower.contains("api_key")
}

fn observability_string_contains_absolute_path(value: &str) -> bool {
	let mut previous = None;
	let mut chars = value.char_indices().peekable();

	while let Some((index, character)) = chars.next() {
		if character != '/' {
			previous = Some(character);

			continue;
		}
		if previous == Some(':') || previous == Some('/') {
			previous = Some(character);

			continue;
		}

		let path_boundary = index == 0
			|| previous.is_some_and(|previous| {
				previous.is_whitespace()
					|| matches!(previous, '"' | '\'' | '`' | '(' | '[' | '{' | '=')
			});
		let path_component = chars
			.peek()
			.map(|(_, next)| next.is_ascii_alphanumeric() || matches!(next, '.' | '_' | '-'))
			.unwrap_or(false);

		if path_boundary && path_component {
			return true;
		}

		previous = Some(character);
	}

	false
}

fn observability_string_contains_windows_path(value: &str) -> bool {
	let bytes = value.as_bytes();

	bytes.windows(3).enumerate().any(|(index, window)| {
		let boundary = index == 0 || {
			let previous = bytes[index - 1];

			previous.is_ascii_whitespace()
				|| matches!(previous, b'"' | b'\'' | b'`' | b'(' | b'[' | b'{' | b'=')
		};

		boundary
			&& window[0].is_ascii_alphabetic()
			&& window[1] == b':'
			&& matches!(window[2], b'\\' | b'/')
	})
}

fn observability_string_contains_secret_like_token(value: &str) -> bool {
	value
		.split(|character: char| {
			!(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':' | '/'))
		})
		.any(|token| {
			let lower = token.to_ascii_lowercase();

			(lower.starts_with("ghp_") && token.len() >= 20)
				|| (lower.starts_with("github_pat_") && token.len() >= 20)
				|| (lower.starts_with("sk-") && token.len() >= 20)
				|| (lower.starts_with("sk-proj-") && token.len() >= 20)
				|| (lower.starts_with("xoxb-") && token.len() >= 20)
				|| (lower.starts_with("xoxp-") && token.len() >= 20)
				|| observability_token_looks_high_entropy_secret(token)
				|| observability_token_looks_like_jwt(token)
		})
}

fn observability_token_looks_high_entropy_secret(token: &str) -> bool {
	if token.len() < 32 || token.len() > 256 {
		return false;
	}
	if !token.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) {
		return false;
	}

	let mut has_lower = false;
	let mut has_upper = false;
	let mut digit_count = 0_usize;
	let mut seen = [false; 128];
	let mut unique_count = 0_usize;

	for byte in token.bytes() {
		has_lower |= byte.is_ascii_lowercase();
		has_upper |= byte.is_ascii_uppercase();

		if byte.is_ascii_digit() {
			digit_count += 1;
		}
		if byte.is_ascii() && !seen[byte as usize] {
			seen[byte as usize] = true;
			unique_count += 1;
		}
	}

	has_lower && has_upper && digit_count >= 4 && unique_count >= 16
}

fn observability_token_looks_like_jwt(token: &str) -> bool {
	let mut segments = token.split('.');
	let Some(header) = segments.next() else {
		return false;
	};
	let Some(payload) = segments.next() else {
		return false;
	};
	let Some(signature) = segments.next() else {
		return false;
	};

	segments.next().is_none()
		&& header.starts_with("eyJ")
		&& payload.len() >= 16
		&& signature.len() >= 16
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

fn read_sorted_dir(path: &Path) -> crate::prelude::Result<Vec<PathBuf>, McpError> {
	let entries = match fs::read_dir(path) {
		Ok(entries) => entries,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
		Err(error) => return Err(McpError::internal(error)),
	};
	let mut paths = entries
		.map(|entry| entry.map(|entry| entry.path()).map_err(McpError::internal))
		.collect::<crate::prelude::Result<Vec<_>, _>>()?;

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
) -> crate::prelude::Result<ResourceContent, McpError> {
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
	use std::{fs, io::Cursor, path::Path, process, str};

	use serde_json::Value;
	use tempfile::TempDir;

	use crate::{
		loop_contract::DecisionContract,
		mcp::{
			self, DEFAULT_MCP_HTTP_LISTEN_ADDRESS, McpCapabilityProfile, McpContext,
			McpHttpHandler, McpHttpSessions, McpServer, McpTransport, ResourceContent,
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
		assert_eq!(combined["live"]["current_lanes"][0]["loop_review"]["status"], "pending");
		assert_eq!(combined["live"]["current_lanes"][0]["loop_review"]["checkpoint"]["round"], 3);
		assert!(
			combined["live"]["current_lanes"][0]["loop_review"]["checkpoint"]["head_sha"].is_null()
		);
		assert_eq!(combined["review"]["post_review_lanes"][0]["pr_url"], "https://example/pr/1");
		assert!(combined["review"]["post_review_lanes"][0]["branch_name"].is_null());
		assert!(combined["review"]["post_review_lanes"][0]["loop_status"].is_null());
		assert_eq!(
			combined["review"]["current_lane_reviews"].as_array().expect("review array").len(),
			1
		);
		assert_eq!(combined["review"]["current_lane_reviews"][0]["review"]["status"], "pending");
		assert_eq!(
			combined["review"]["current_lane_reviews"][0]["review"]["checkpoint"]["round"],
			3
		);
		assert!(
			combined["review"]["current_lane_reviews"][0]["review"]["checkpoint"]["active_fingerprints"]
				.is_null()
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
	fn resources_read_exposes_bounded_live_activity_and_recent_run_readback() {
		let repo = test_repo();
		let codex_home = repo.path().join("codex-home");
		let codex_home = codex_home.to_string_lossy().into_owned();
		let _codex_home_guard = TestEnvVarGuard::set("CODEX_HOME", &codex_home);
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
		assert_eq!(
			pr_review_state["current_lane_reviews"].as_array().expect("review array").len(),
			0
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
		let codex_home = repo.path().join("codex-home");
		let codex_home = codex_home.to_string_lossy().into_owned();
		let _codex_home_guard = TestEnvVarGuard::set("CODEX_HOME", &codex_home);
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
		assert!(
			!result["structuredContent"]["message"]
				.as_str()
				.expect("message should be text")
				.contains("stdio")
		);
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
		assert!(
			!result["structuredContent"]["message"]
				.as_str()
				.expect("message should be text")
				.contains("stdio")
		);
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
			Some("Content-Type, Accept, Mcp-Session-Id")
		);
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
			mcp::validate_mcp_http_listen_address(DEFAULT_MCP_HTTP_LISTEN_ADDRESS, &[]).is_ok()
		);
		assert!(mcp::validate_mcp_http_listen_address("0.0.0.0:8193", &[]).is_err());
		assert!(
			mcp::validate_mcp_http_listen_address(
				"0.0.0.0:8193",
				&[String::from("https://relay.example")]
			)
			.is_ok()
		);
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
			("decodex_admin", "admin", r#"{"action":"capabilities"}"#),
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
			"loop_status": {
				"review": observability_review_status_fixture(
					"private-head-sha",
					"fingerprint-private",
					"stop-fingerprint-private",
					3
				)
			},
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

	fn http_handler(repo_root: &Path, capability_profile: McpCapabilityProfile) -> McpHttpHandler {
		http_handler_with_allowed_origins(repo_root, capability_profile, Vec::new())
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
		McpHttpHandler {
			server: McpServer {
				context,
				capability_profile,
				transport: McpTransport::StreamableHttp,
			},
			sessions: McpHttpSessions::default(),
			allowed_origins,
			listen_address: Some(String::from(DEFAULT_MCP_HTTP_LISTEN_ADDRESS)),
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
		write_file(repo.path().join("docs/research/sample-report.json"), "{}\n");

		repo
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

			state_store
				.upsert_worktree(
					"pubfi",
					&issue_id,
					&format!("x/pubfi-{index:03}"),
					&worktree_path.display().to_string(),
				)
				.expect("worktree should record");
			state_store
				.record_run_attempt(&run_id, &issue_id, 1, "succeeded")
				.expect("run attempt should record");
			state_store
				.append_event(&run_id, 1, "turn/completed", r#"{"status":"completed"}"#)
				.expect("event should record");

			if index == 12 {
				seed_mcp_lane_runtime_markers(&state_store, &worktree_path, &run_id);

				state_store
					.append_event(
						&run_id,
						2,
						"configWarning",
						r#"{"summary":"config at /private/worktree using GITHUB_PAT_Y"}"#,
					)
					.expect("warning event should record");
				state_store
					.append_event(
						&run_id,
						3,
						"error",
						r#"{"error":{"codexErrorInfo":"failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK"}}"#,
					)
					.expect("error event should record");
				state_store
					.append_event(
						&run_id,
						4,
						"configWarning",
						r#"{"summary":"state marker under /srv/decodex/runtime"}"#,
					)
					.expect("generic path warning event should record");
				state_store
					.append_event(
						&run_id,
						5,
						"error",
						r#"{"error":{"codexErrorInfo":"upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456"}}"#,
					)
					.expect("token-shaped error event should record");
				state_store
					.append_event(
						&run_id,
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
							detail: Some(String::from(
								"config at /private/worktree using GITHUB_PAT_Y",
							)),
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
					.record_run_activity_summary(&run_id, 1, None, Some(&protocol_activity))
					.expect("activity summary should record");
			}
		}
	}

	fn seed_mcp_lane_runtime_markers(state_store: &StateStore, worktree_path: &Path, run_id: &str) {
		fs::create_dir_all(worktree_path).expect("worktree path should exist");

		state_store.update_run_thread(run_id, "thread-12").expect("thread should record");
		state_store.update_run_turn(run_id, "turn-12").expect("turn should record");

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
