use std::{
	collections::{BTreeMap, BTreeSet},
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
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
		AutonomyObjectiveState,
	},
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalAuthorityActorKind, AutonomyProposalChallengeInput,
		AutonomyProposalChallengeSource, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority,
	},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalKind, AutonomySignalPrivacy,
		AutonomySignalReviewEvidence, AutonomySignalSourceType,
	},
	config::ServiceConfig,
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	orchestrator::{self, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, McpLaneSteerRequest},
	prelude::eyre,
	program_intake::{
		self, GoalIntakeCommandRequest, GoalIntakeIssueReport, GoalIntakeReport,
		GoalIntakeRunRequest,
	},
	research_design::{
		self, ResearchDesignOutcome, ResearchDesignRunInput, ResearchDesignRunReport,
	},
	runtime,
	state::{AutonomyProposalRecord, AutonomySignalRecord, StateStore},
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate,
	},
	workflow::WorkflowDocument,
};

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
const MCP_HTTP_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MCP_HTTP_MAX_REQUEST_BYTES: usize = 1_024 * 1_024;
const MCP_SESSION_HEADER: &str = "Mcp-Session-Id";
const MCP_CORS_ALLOW_METHODS: &str = "POST, DELETE, OPTIONS";
const MCP_CORS_ALLOW_HEADERS: &str = "Content-Type, Accept, Mcp-Session-Id, Authorization";
const MCP_AUTHORIZATION_HEADER: &str = "Authorization";
const MCP_WWW_AUTHENTICATE_HEADER: &str = "Bearer realm=\"decodex-mcp\"";

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
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/autonomy"),
				format!("Project {project_id} autonomy summaries"),
				"Read-only autonomy objective, signal, proposal, and evidence summaries.",
			));
		}

		Ok(serde_json::json!({ "resources": resources }))
	}

	fn list_resource_templates(&self) -> Value {
		let mut resource_templates = docs_resource_templates();

		resource_templates.extend(runtime_resource_templates());

		serde_json::json!({
			"resourceTemplates": resource_templates
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

	fn call_research_compile_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<ResearchCompileToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_RESEARCH_COMPILE,
					"`mode` must be dry_run or apply, with either `input` or `intent`.",
				);
			},
		};
		let mode = match planning_mode(params.mode.as_deref(), "dry_run", TOOL_RESEARCH_COMPILE) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_RESEARCH_COMPILE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_RESEARCH_COMPILE,
				"research_compile apply requires authority.source and authority.reason.",
			);
		}

		let input = match research_compile_input(params) {
			Ok(input) => input,
			Err(result) => return result,
		};
		let report = if mode == "apply" {
			let store = match planning_state_store(&self.context, TOOL_RESEARCH_COMPILE) {
				Ok(store) => store,
				Err(result) => return result,
			};

			research_design::persist_research_design_run(store, &project_id, input)
		} else {
			research_design::dry_run_research_design_compile(input, &project_id)
		};

		match report {
			Ok(report) => tool_success(research_compile_result(&report, mode == "apply", mode)),
			Err(_) => tool_refusal(
				"research_compile_refused",
				"Research compile input did not satisfy Decodex Decision Contract requirements.",
			),
		}
	}

	fn call_research_promote_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<ResearchPromoteToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_RESEARCH_PROMOTE,
					"`contractId` is required and `mode` must be dry_run or apply.",
				);
			},
		};
		let Some(contract_id) = non_empty_string(Some(params.contract_id.as_str())) else {
			return invalid_tool_arguments(TOOL_RESEARCH_PROMOTE, "`contractId` is required.");
		};

		if !safe_runtime_identifier(contract_id) {
			return invalid_tool_arguments(
				TOOL_RESEARCH_PROMOTE,
				"`contractId` must be a safe Decodex runtime identifier.",
			);
		}

		let mode = match planning_mode(params.mode.as_deref(), "dry_run", TOOL_RESEARCH_PROMOTE) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_RESEARCH_PROMOTE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let store = match planning_state_store(&self.context, TOOL_RESEARCH_PROMOTE) {
			Ok(store) => store,
			Err(result) => return result,
		};

		if mode == "dry_run" {
			return match store.decision_contract(&project_id, contract_id) {
				Ok(Some(record)) => tool_success(research_promote_readiness_result(
					record.contract_id(),
					record.status().as_str(),
					record.contract().execution_readiness().ready_for_issue_shaping(),
					false,
					mode,
				)),
				Ok(None) => tool_refusal(
					"contract_not_found",
					"Decision Contract was not found in the current Decodex project.",
				),
				Err(_) => tool_refusal(
					"research_promote_refused",
					"Decision Contract readback failed before promotion.",
				),
			};
		}

		let authority = match promotion_authority(params.authority.as_ref()) {
			Ok(authority) => authority,
			Err(result) => return result,
		};
		let accepted_at = match authority.accepted_at {
			Some(accepted_at) => accepted_at.to_owned(),
			None => match OffsetDateTime::now_utc().format(&Rfc3339) {
				Ok(value) => value,
				Err(_) => {
					return tool_refusal(
						"research_promote_refused",
						"Promotion timestamp could not be prepared.",
					);
				},
			},
		};
		let promotion = match DecisionPromotion::new(
			authority.accepted_by,
			DecisionPromotionActorKind::User,
			accepted_at,
			authority.acceptance_source,
			authority.reason.cloned(),
		) {
			Ok(promotion) => promotion,
			Err(_) => {
				return tool_refusal(
					"research_promote_refused",
					"Promotion authority did not satisfy Decodex Decision Contract requirements.",
				);
			},
		};

		match research_design::promote_research_design_contract(
			store,
			&project_id,
			contract_id,
			promotion,
		) {
			Ok(record) => tool_success(research_promote_readiness_result(
				record.contract_id(),
				record.status().as_str(),
				record.contract().execution_readiness().ready_for_issue_shaping(),
				true,
				mode,
			)),
			Err(_) => tool_refusal(
				"research_promote_refused",
				"Decision Contract promotion was refused by Decodex authority checks.",
			),
		}
	}

	fn call_intake_goal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<IntakeGoalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_INTAKE_GOAL,
					"`contractId` is required and `mode` must be dry_run or apply.",
				);
			},
		};
		let Some(contract_id) = non_empty_string(Some(params.contract_id.as_str())) else {
			return invalid_tool_arguments(TOOL_INTAKE_GOAL, "`contractId` is required.");
		};

		if !safe_runtime_identifier(contract_id) {
			return invalid_tool_arguments(
				TOOL_INTAKE_GOAL,
				"`contractId` must be a safe Decodex runtime identifier.",
			);
		}

		let mode = match planning_mode(params.mode.as_deref(), "dry_run", TOOL_INTAKE_GOAL) {
			Ok(mode) => mode,
			Err(result) => return result,
		};

		if mode == "apply" {
			if !planning_authority_present(params.authority.as_ref()) {
				return missing_authority_refusal(
					TOOL_INTAKE_GOAL,
					"intake_goal apply requires authority.source and authority.reason.",
				);
			}

			return self
				.apply_intake_goal_tool(contract_id, params.team_issue_identifier.as_deref());
		}

		let store = match planning_state_store(&self.context, TOOL_INTAKE_GOAL) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let config_path = match self.context.config_path.as_deref() {
			Some(path) => path,
			None => {
				return tool_refusal(
					"missing_project_context",
					"intake_goal dry-run requires a registered Decodex project config or --config.",
				);
			},
		};
		let config = match ServiceConfig::from_path(config_path) {
			Ok(config) => config,
			Err(_) => {
				return tool_refusal(
					"missing_project_context",
					"intake_goal dry-run could not load the Decodex project config.",
				);
			},
		};
		let workflow = match WorkflowDocument::from_path(config.workflow_path()) {
			Ok(workflow) => workflow,
			Err(_) => {
				return tool_refusal(
					"missing_project_context",
					"intake_goal dry-run could not load the Decodex workflow contract.",
				);
			},
		};
		let tracker = McpDryRunTracker;

		match program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id,
			team_issue_identifier: params.team_issue_identifier,
			dry_run: true,
			apply: false,
		}) {
			Ok(report) => tool_success(intake_goal_result(&report, mode)),
			Err(_) => tool_refusal(
				"intake_goal_refused",
				"Goal intake dry-run was refused by Decodex authority checks.",
			),
		}
	}

	fn apply_intake_goal_tool(
		&self,
		contract_id: &str,
		team_issue_identifier: Option<&str>,
	) -> Value {
		let Some(config_path) = self.context.config_path.as_deref() else {
			return tool_refusal(
				"missing_project_context",
				"intake_goal apply requires a registered Decodex project config or --config.",
			);
		};

		match program_intake::run_goal_intake_command(GoalIntakeCommandRequest {
			config_path: Some(config_path),
			project_id: self.context.project_id.as_deref(),
			contract_id,
			team_issue_identifier,
			dry_run: false,
			apply: true,
		}) {
			Ok(report) => tool_success(intake_goal_result(&report, "apply")),
			Err(_) => tool_refusal(
				"intake_goal_refused",
				"Goal intake apply was refused by Decodex authority or tracker checks.",
			),
		}
	}

	fn call_autonomy_draft_objective_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyDraftObjectiveToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_DRAFT_OBJECTIVE,
					"`objective` is required and `mode` must be dry_run or apply.",
				),
		};
		let mode =
			match planning_mode(params.mode.as_deref(), "dry_run", TOOL_AUTONOMY_DRAFT_OBJECTIVE) {
				Ok(mode) => mode,
				Err(result) => return result,
			};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_DRAFT_OBJECTIVE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};

		if params.objective.project_id() != project_id {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_DRAFT_OBJECTIVE,
				"`objective.project_id` must match the MCP project context.",
			);
		}
		if params.objective.state() != AutonomyObjectiveState::Draft {
			return tool_refusal(
				"objective_draft_refused",
				"autonomy_draft_objective only stores draft Objective Contracts; acceptance uses a separate explicit authority surface.",
			);
		}

		if let Err(error) = params.objective.validate() {
			return tool_refusal(
				"objective_draft_refused",
				format!("Objective Contract draft did not validate: {error}"),
			);
		}

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_AUTONOMY_DRAFT_OBJECTIVE,
				"autonomy_draft_objective apply requires authority.source and authority.reason.",
			);
		}
		if mode == "dry_run" {
			return tool_success(autonomy_objective_tool_result(
				&project_id,
				&params.objective,
				mode,
				false,
				None,
			));
		}

		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_DRAFT_OBJECTIVE) {
			Ok(store) => store,
			Err(result) => return result,
		};

		match store.upsert_autonomy_objective_draft(&project_id, params.objective) {
			Ok(record) => tool_success(autonomy_objective_tool_result(
				&project_id,
				record.objective(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"objective_draft_refused",
				format!(
					"Objective Contract draft was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	fn call_autonomy_accept_objective_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyAcceptObjectiveToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
					"`objectiveId`, `objectiveVersion`, and optional `mode` are required.",
				),
		};
		let Some(objective_id) = non_empty_string(Some(params.objective_id.as_str())) else {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"`objectiveId` is required.",
			);
		};

		if !safe_autonomy_record_identifier(objective_id) {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"`objectiveId` must be a safe Decodex autonomy identifier.",
			);
		}
		if params.objective_version == 0 {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"`objectiveVersion` must be greater than zero.",
			);
		}

		let mode = match planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_ACCEPT_OBJECTIVE) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let record =
			match store.autonomy_objective(&project_id, objective_id, params.objective_version) {
				Ok(Some(record)) => record,
				Ok(None) =>
					return tool_refusal(
						"objective_not_found",
						"Autonomy Objective Contract draft was not found in the current Decodex project.",
					),
				Err(error) =>
					return tool_refusal(
						"objective_acceptance_refused",
						format!("Objective Contract readback failed closed: {error}"),
					),
			};

		if record.state() != AutonomyObjectiveState::Draft {
			return tool_refusal(
				"objective_acceptance_refused",
				"Only draft Objective Contract versions can be accepted through autonomy_accept_objective.",
			);
		}
		if mode == "dry_run" {
			return tool_success(autonomy_objective_accept_tool_result(
				&project_id,
				record.objective(),
				mode,
				false,
				Some(record.updated_at()),
			));
		}

		let Some(authority) = params.authority else {
			return missing_authority_refusal(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"autonomy_accept_objective apply requires explicit objective acceptance authority.",
			);
		};
		let acceptance = match authority.into_acceptance() {
			Ok(acceptance) => acceptance,
			Err(result) => return result,
		};

		match store.accept_autonomy_objective_version(
			&project_id,
			objective_id,
			params.objective_version,
			acceptance,
		) {
			Ok(record) => tool_success(autonomy_objective_accept_tool_result(
				&project_id,
				record.objective(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"objective_acceptance_refused",
				format!(
					"Objective Contract acceptance was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	fn call_autonomy_submit_signal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomySubmitSignalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_SUBMIT_SIGNAL,
					"`kind`, `signal`, and optional `mode` are required.",
				),
		};
		let mode =
			match planning_mode(params.mode.as_deref(), "dry_run", TOOL_AUTONOMY_SUBMIT_SIGNAL) {
				Ok(mode) => mode,
				Err(result) => return result,
			};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_SUBMIT_SIGNAL,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let signal = match autonomy_signal_from_tool_args(params.kind, params.signal, &project_id) {
			Ok(signal) => signal,
			Err(result) => return result,
		};

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_AUTONOMY_SUBMIT_SIGNAL,
				"autonomy_submit_signal apply requires authority.source and authority.reason.",
			);
		}
		if mode == "dry_run" {
			return tool_success(autonomy_signal_tool_result(
				&project_id,
				&signal,
				mode,
				false,
				None,
			));
		}

		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_SUBMIT_SIGNAL) {
			Ok(store) => store,
			Err(result) => return result,
		};

		match store.record_autonomy_signal(&project_id, signal) {
			Ok(record) => tool_success(autonomy_signal_tool_result(
				&project_id,
				record.signal(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"autonomy_signal_refused",
				format!("Autonomy signal was refused by Decodex authority checks: {error}"),
			),
		}
	}

	fn call_autonomy_compile_proposal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyCompileProposalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_COMPILE_PROPOSAL,
					"`proposal`, `signalIds`, and optional `mode` are required.",
				),
		};
		let mode = match planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_COMPILE_PROPOSAL,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_COMPILE_PROPOSAL,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let input = match params.proposal.into_compile_input(&project_id) {
			Ok(input) => input,
			Err(result) => return result,
		};

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_AUTONOMY_COMPILE_PROPOSAL,
				"autonomy_compile_proposal apply requires authority.source and authority.reason.",
			);
		}

		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_COMPILE_PROPOSAL) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let proposal = match store.compile_autonomy_proposal_dry_run(input, &params.signal_ids) {
			Ok(proposal) => proposal,
			Err(error) =>
				return tool_refusal(
					"autonomy_proposal_refused",
					format!("Autonomy proposal compile was refused: {error}"),
				),
		};

		if mode == "dry_run" {
			return tool_success(autonomy_proposal_tool_result(
				&project_id,
				&proposal,
				mode,
				false,
				None,
			));
		}

		match store.record_autonomy_proposal(&project_id, proposal) {
			Ok(record) => tool_success(autonomy_proposal_tool_result(
				&project_id,
				record.proposal(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"autonomy_proposal_refused",
				format!(
					"Autonomy proposal persistence was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	fn call_autonomy_challenge_proposal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyChallengeProposalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
					"`proposalId`, `challenge`, and optional `mode` are required.",
				),
		};
		let Some(proposal_id) = non_empty_string(Some(params.proposal_id.as_str())) else {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"`proposalId` is required.",
			);
		};

		if !safe_autonomy_record_identifier(proposal_id) {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"`proposalId` must be a safe Decodex autonomy identifier.",
			);
		}

		let mode = match planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let challenge = match params.challenge.into_challenge_input() {
			Ok(challenge) => challenge,
			Err(result) => return result,
		};
		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_CHALLENGE_PROPOSAL) {
			Ok(store) => store,
			Err(result) => return result,
		};

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"autonomy_challenge_proposal apply requires authority.source and authority.reason.",
			);
		}
		if mode == "dry_run" {
			let record = match store.autonomy_proposal(&project_id, proposal_id) {
				Ok(Some(record)) => record,
				Ok(None) =>
					return tool_refusal(
						"proposal_not_found",
						"Autonomy proposal was not found in the current Decodex project.",
					),
				Err(error) =>
					return tool_refusal(
						"autonomy_challenge_refused",
						format!("Autonomy proposal readback failed closed: {error}"),
					),
			};
			let mut proposal = record.proposal().clone();

			return match proposal.record_challenge(challenge) {
				Ok(()) => tool_success(autonomy_challenge_tool_result(
					&project_id,
					&proposal,
					mode,
					false,
					Some(record.updated_at()),
				)),
				Err(error) => tool_refusal(
					"autonomy_challenge_refused",
					format!("Autonomy proposal challenge was refused: {error}"),
				),
			};
		}

		match store.record_autonomy_proposal_challenge(&project_id, proposal_id, challenge) {
			Ok(record) => tool_success(autonomy_challenge_tool_result(
				&project_id,
				record.proposal(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"autonomy_challenge_refused",
				format!(
					"Autonomy proposal challenge was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	fn call_autonomy_request_promotion_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyRequestPromotionToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_REQUEST_PROMOTION,
					"`proposalId` and optional `mode` are required.",
				),
		};
		let Some(proposal_id) = non_empty_string(Some(params.proposal_id.as_str())) else {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_REQUEST_PROMOTION,
				"`proposalId` is required.",
			);
		};

		if !safe_autonomy_record_identifier(proposal_id) {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_REQUEST_PROMOTION,
				"`proposalId` must be a safe Decodex autonomy identifier.",
			);
		}

		let mode =
			match planning_mode(params.mode.as_deref(), "dry_run", TOOL_AUTONOMY_REQUEST_PROMOTION)
			{
				Ok(mode) => mode,
				Err(result) => return result,
			};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_REQUEST_PROMOTION,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_REQUEST_PROMOTION) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let record = match store.autonomy_proposal(&project_id, proposal_id) {
			Ok(Some(record)) => record,
			Ok(None) =>
				return tool_refusal(
					"proposal_not_found",
					"Autonomy proposal was not found in the current Decodex project.",
				),
			Err(error) =>
				return tool_refusal(
					"autonomy_promotion_refused",
					format!("Autonomy proposal readback failed closed: {error}"),
				),
		};

		if mode == "dry_run" {
			return tool_success(autonomy_promotion_request_result(
				&project_id,
				record.proposal(),
				mode,
				false,
				None,
			));
		}

		let Some(authority) = params.authority else {
			return missing_authority_refusal(
				TOOL_AUTONOMY_REQUEST_PROMOTION,
				"autonomy_request_promotion apply requires explicit proposal acceptance authority.",
			);
		};
		let authority = match authority.into_decision_bridge_authority() {
			Ok(authority) => authority,
			Err(result) => return result,
		};

		match store.accept_autonomy_proposal_as_decision_contract_candidate(
			&project_id,
			proposal_id,
			authority,
		) {
			Ok(contract) => tool_success(autonomy_promotion_request_result(
				&project_id,
				record.proposal(),
				mode,
				true,
				Some(contract.contract_id()),
			)),
			Err(error) => tool_refusal(
				"autonomy_promotion_refused",
				format!(
					"Autonomy proposal promotion request was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	fn call_lane_control_tool(&self, arguments: Value, profile: McpCapabilityProfile) -> Value {
		let params = match serde_json::from_value::<LaneControlToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_LANE_CONTROL,
					"`action` is required and must be one of inspect, interrupt, steer, manual_attention, or retained_resume.",
				);
			},
		};

		if !matches!(
			params.action.as_str(),
			"inspect" | "interrupt" | "steer" | "manual_attention" | "retained_resume"
		) {
			return invalid_tool_arguments(
				TOOL_LANE_CONTROL,
				"`action` must be one of inspect, interrupt, steer, manual_attention, or retained_resume.",
			);
		}

		match params.action.as_str() {
			"inspect" => self.call_lane_control_inspect_tool(&params, profile),
			"interrupt" => self.call_lane_control_interrupt_tool(&params, profile),
			"steer" => self.call_lane_control_steer_tool(&params, profile),
			"manual_attention" => lane_control_refusal_result(
				&params,
				profile,
				"tracker_terminal_path_required",
				"MCP does not synthesize manual attention. Use the issue-scoped tracker terminal path so Decodex can validate the public blocker and terminal finalize state.",
			),
			"retained_resume" => lane_control_refusal_result(
				&params,
				profile,
				"runtime_lifecycle_required",
				"Retained resume is owned by the Decodex runtime lifecycle. Use the normal retained-lane dispatch path instead of an MCP shortcut.",
			),
			_ => unreachable!("lane-control action was validated above"),
		}
	}

	fn call_lane_control_inspect_tool(
		&self,
		params: &LaneControlToolArgs,
		profile: McpCapabilityProfile,
	) -> Value {
		let Some(issue) = non_empty_string(params.issue.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"issue_required",
				"`issue` is required for lane-control inspect.",
			);
		};

		if let Some(project_id) = non_empty_string(params.project_id.as_deref())
			&& Some(project_id) != self.context.project_id.as_deref()
		{
			return lane_control_refusal_result(
				params,
				profile,
				"project_mismatch",
				"The requested projectId does not match this MCP gateway context.",
			);
		}

		let report = match orchestrator::build_mcp_lane_control_resource(
			self.context.config_path.as_deref(),
			Some(issue),
			params.run_id.as_deref().and_then(|run_id| non_empty_string(Some(run_id))),
			DEFAULT_MCP_STATUS_LIMIT,
		) {
			Ok(report) => report,
			Err(error) => {
				return lane_control_refusal_result(
					params,
					profile,
					"lane_inspect_unavailable",
					format!("Lane inspect failed closed: {error}"),
				);
			},
		};
		let mut result = serde_json::json!({
			"schema": "decodex.mcp.lane_control_result/1",
			"status": "ok",
			"reason": "inspect_complete",
			"message": "Inspect returned current lane-control preconditions for any later mutating request.",
			"capability_profile": profile.as_str(),
			"action": "inspect",
			"project_id": self.context.project_id.as_deref(),
			"issue": issue,
			"run_id": params.run_id.as_deref(),
			"preconditions": lane_control_preconditions(params),
			"result": {
				"inspect": mcp_public_lane_inspect_resource(report.clone()),
				"mutating_preconditions": lane_control_mutating_preconditions(&report)
			}
		});

		sanitize_mcp_observability_value(&mut result);

		tool_success(result)
	}

	fn call_lane_control_interrupt_tool(
		&self,
		params: &LaneControlToolArgs,
		profile: McpCapabilityProfile,
	) -> Value {
		let Some(issue) = non_empty_string(params.issue.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"issue_required",
				"`issue` is required for lane-control interrupt.",
			);
		};
		let Some(run_id) = non_empty_string(params.run_id.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_missing",
				"`runId` from lane-control inspect is required for interrupt.",
			);
		};
		let Some(authority) = lane_control_authority(params) else {
			return lane_control_refusal_result(
				params,
				profile,
				"authority_required",
				"Mutating lane-control calls require authority.reason, authority.source, and authority.inspectedRunId.",
			);
		};

		if authority.inspected_run_id != run_id {
			return lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_mismatch",
				"authority.inspectedRunId must match the requested runId.",
			);
		}
		if params.force.unwrap_or(false) && !authority.allow_hard_fallback {
			return lane_control_refusal_result(
				params,
				profile,
				"hard_fallback_authority_missing",
				"Hard interrupt fallback requires force=true and authority.allowHardFallback=true.",
			);
		}

		let report = match orchestrator::run_mcp_lane_interrupt(
			self.context.config_path.as_deref(),
			issue,
			run_id,
			params.force.unwrap_or(false),
			Some(authority.reason),
			authority.source,
		) {
			Ok(report) => report,
			Err(error) => {
				return lane_control_refusal_result(
					params,
					profile,
					"lane_interrupt_unavailable",
					format!("Lane interrupt failed closed: {error}"),
				);
			},
		};

		lane_control_interrupt_result(params, profile, report)
	}

	fn call_lane_control_steer_tool(
		&self,
		params: &LaneControlToolArgs,
		profile: McpCapabilityProfile,
	) -> Value {
		let Some(issue) = non_empty_string(params.issue.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"issue_required",
				"`issue` is required for lane-control steer.",
			);
		};
		let Some(run_id) = non_empty_string(params.run_id.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_missing",
				"`runId` from lane-control inspect is required for steer.",
			);
		};
		let Some(expected_turn_id) = non_empty_string(params.expected_turn_id.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"expected_turn_id_required",
				"`expectedTurnId` from lane-control inspect is required for steer.",
			);
		};
		let Some(message) = non_empty_string(params.message.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"message_required",
				"`message` is required for steer and is never echoed in MCP results.",
			);
		};
		let Some(authority) = lane_control_authority(params) else {
			return lane_control_refusal_result(
				params,
				profile,
				"authority_required",
				"Mutating lane-control calls require authority.reason, authority.source, and authority.inspectedRunId.",
			);
		};

		if authority.inspected_run_id != run_id {
			return lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_mismatch",
				"authority.inspectedRunId must match the requested runId.",
			);
		}
		if authority.expected_turn_id != Some(expected_turn_id) {
			return lane_control_refusal_result(
				params,
				profile,
				"expected_turn_authority_mismatch",
				"authority.expectedTurnId must match the requested expectedTurnId.",
			);
		}

		let report = match orchestrator::run_mcp_lane_steer(McpLaneSteerRequest {
			config_path: self.context.config_path.as_deref(),
			project_id: params
				.project_id
				.as_deref()
				.and_then(|project_id| non_empty_string(Some(project_id))),
			issue,
			run_id,
			expected_turn_id,
			message,
			source: authority.source,
			wait_timeout: DEFAULT_STEER_RESULT_WAIT_TIMEOUT,
		}) {
			Ok(report) => report,
			Err(error) => {
				return lane_control_refusal_result(
					params,
					profile,
					"lane_steer_unavailable",
					format!("Lane steer failed closed: {error}"),
				);
			},
		};

		lane_control_steer_result(params, profile, report)
	}

	fn call_project_control_tool(&self, arguments: Value, profile: McpCapabilityProfile) -> Value {
		let params = match serde_json::from_value::<ProjectControlToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_PROJECT_CONTROL,
					"`action` is required and must be one of status, pause, resume, or scan.",
				);
			},
		};

		if !matches!(params.action.as_str(), "status" | "pause" | "resume" | "scan") {
			return invalid_tool_arguments(
				TOOL_PROJECT_CONTROL,
				"`action` must be one of status, pause, resume, or scan.",
			);
		}

		let Some(project_id) =
			non_empty_string(params.project_id.as_deref()).or(self.context.project_id.as_deref())
		else {
			return project_control_refusal_result(
				&params,
				profile,
				"project_id_required",
				"`projectId` is required when the MCP gateway is not bound to one project config.",
			);
		};

		if let Some(context_project_id) = self.context.project_id.as_deref()
			&& context_project_id != project_id
		{
			return project_control_refusal_result(
				&params,
				profile,
				"project_mismatch",
				"The requested projectId does not match this MCP gateway context.",
			);
		}

		match params.action.as_str() {
			"status" => project_control_status_result(&params, profile, project_id),
			"scan" => project_control_refusal_result(
				&params,
				profile,
				"operator_control_loop_required",
				"Linear scan requests are queued by the Decodex operator control-plane loop; standalone MCP serve cannot enqueue that in-memory request.",
			),
			"pause" | "resume" => self.call_project_enablement_tool(&params, profile, project_id),
			_ => unreachable!("project-control action was validated above"),
		}
	}

	fn call_project_enablement_tool(
		&self,
		params: &ProjectControlToolArgs,
		profile: McpCapabilityProfile,
		project_id: &str,
	) -> Value {
		let Some(authority) = project_control_authority(params) else {
			return project_control_refusal_result(
				params,
				profile,
				"authority_required",
				"Project pause/resume requires authority.reason, authority.source, and authority.acknowledgeFutureDispatchOnly=true.",
			);
		};

		if !authority.acknowledge_future_dispatch_only {
			return project_control_refusal_result(
				params,
				profile,
				"future_dispatch_ack_required",
				"Project control affects future dispatch only and does not kill active lanes.",
			);
		}

		let state_store = match runtime::open_runtime_store_lazy() {
			Ok(state_store) => state_store,
			Err(error) => {
				return project_control_refusal_result(
					params,
					profile,
					"project_control_unavailable",
					format!("Project control failed closed: {error}"),
				);
			},
		};

		if let Some(config_path) = self.context.config_path.as_deref()
			&& let Err(error) = runtime::register_project_config(&state_store, config_path, true)
		{
			return project_control_refusal_result(
				params,
				profile,
				"project_registration_unavailable",
				format!("Project registration refresh failed closed: {error}"),
			);
		}

		let enabled = params.action == "resume";

		if let Err(error) = state_store.set_project_enabled(project_id, enabled) {
			return project_control_refusal_result(
				params,
				profile,
				"project_enablement_unavailable",
				format!("Project {action} failed closed: {error}", action = params.action),
			);
		}

		project_control_success_result(
			params,
			profile,
			project_id,
			serde_json::json!({
				"enabled": enabled,
				"authority_source": authority.source,
				"authority_reason_present": !authority.reason.is_empty(),
				"future_dispatch_only": true,
				"active_lanes_killed": false,
				"next_action": if enabled {
					"Future dispatch is enabled. Active lanes were not modified."
				} else {
					"Future dispatch is paused. Inspect active lanes separately before taking lane-control action."
				}
			}),
		)
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
			let Some(stem) = markdown_stem(&entry) else {
				continue;
			};

			resources.push(McpResource::markdown(
				format!("decodex://research/{stem}"),
				format!("docs/research/{stem}.md"),
				"Checked-in Markdown Research Contract concept.",
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
		let [concept] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if !safe_resource_stem(concept) {
			return Err(McpError::resource_not_found());
		}

		read_file_resource(
			&uri.raw,
			self.repo_root.join("docs/research").join(format!("{concept}.md")),
			"text/markdown",
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
		if resource_kind == "autonomy" {
			let value = self.read_autonomy_project_resource(project_id, rest)?;

			return ResourceContent::mcp_observability_json(&uri.raw, value);
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

	fn read_autonomy_project_resource(
		&self,
		project_id: &str,
		rest: &[String],
	) -> crate::prelude::Result<Value, McpError> {
		let Some(state_store) = self.state_store.as_ref() else {
			return Err(McpError::resource_not_found());
		};

		match rest {
			[] => mcp_autonomy_project_resource(state_store, project_id),
			[resource] if resource == "signals" =>
				mcp_autonomy_signals_resource(state_store, project_id),
			[resource, signal_id]
				if resource == "signals" && safe_autonomy_record_identifier(signal_id) =>
				mcp_autonomy_signal_resource(state_store, project_id, signal_id),
			[resource] if resource == "proposals" =>
				mcp_autonomy_proposals_resource(state_store, project_id),
			[resource, proposal_id]
				if resource == "proposals" && safe_autonomy_record_identifier(proposal_id) =>
				mcp_autonomy_proposal_resource(state_store, project_id, proposal_id),
			[resource] if resource == "evidence" =>
				mcp_autonomy_evidence_resource(state_store, project_id),
			[resource, objective_id, selector]
				if resource == "objectives"
					&& safe_runtime_identifier(objective_id)
					&& selector == "current" =>
				mcp_autonomy_current_objective_resource(state_store, project_id, objective_id),
			[resource, objective_id, version]
				if resource == "objectives" && safe_runtime_identifier(objective_id) =>
				mcp_autonomy_objective_version_resource(
					state_store,
					project_id,
					objective_id,
					version,
				),
			_ => Err(McpError::resource_not_found()),
		}
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
struct ResearchCompileToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	input: Option<ResearchDesignRunInput>,
	intent: Option<String>,
	source_issue: Option<String>,
	outcome: Option<ResearchDesignOutcome>,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResearchPromoteToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	contract_id: String,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IntakeGoalToolArgs {
	mode: Option<String>,
	contract_id: String,
	team_issue_identifier: Option<String>,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyDraftObjectiveToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	objective: AutonomyObjectiveContract,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyAcceptObjectiveToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	objective_id: String,
	objective_version: u64,
	authority: Option<AutonomyObjectiveAcceptanceArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyObjectiveAcceptanceArgs {
	accepted_by: String,
	accepted_by_kind: AutonomyObjectiveActorKind,
	accepted_at: Option<String>,
	acceptance_source: String,
}
impl AutonomyObjectiveAcceptanceArgs {
	fn into_acceptance(self) -> Result<AutonomyObjectiveAcceptance, Value> {
		if self.accepted_by_kind == AutonomyObjectiveActorKind::RuntimePolicy {
			return Err(tool_refusal(
				"objective_acceptance_refused",
				"Runtime-policy Objective Contract acceptance must be resolved from trusted Decodex authority state; caller-supplied runtime_policy acceptance fails closed.",
			));
		}

		AutonomyObjectiveAcceptance::new(
			self.accepted_by,
			self.accepted_by_kind,
			self.accepted_at.unwrap_or_else(mcp_now_rfc3339),
			self.acceptance_source,
		)
		.map_err(|error| {
			tool_refusal(
				"objective_acceptance_refused",
				format!("Objective Contract acceptance authority was refused: {error}"),
			)
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomySubmitSignalToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	kind: AutonomySignalKind,
	signal: AutonomySignalInputArgs,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomySignalInputArgs {
	objective_id: String,
	objective_version: u64,
	source_type: AutonomySignalSourceType,
	source_refs: Vec<String>,
	#[serde(default)]
	primary_source_refs: Vec<String>,
	issue_id: Option<String>,
	run_id: Option<String>,
	attempt_id: Option<String>,
	head_sha: Option<String>,
	captured_at: Option<String>,
	freshness: AutonomySignalFreshness,
	summary: String,
	evidence: Vec<String>,
	evidence_class: AutonomySignalEvidenceClass,
	#[serde(default)]
	contradictions: Vec<String>,
	#[serde(default)]
	gaps: Vec<String>,
	confidence: AutonomySignalConfidence,
	privacy: AutonomySignalPrivacy,
	#[serde(default)]
	observed_counts: BTreeMap<String, u64>,
	review_evidence: Option<AutonomySignalReviewEvidence>,
	proposal_only: Option<bool>,
	created_at: Option<String>,
}
impl AutonomySignalInputArgs {
	fn into_signal_input(self, project_id: &str) -> AutonomySignalInput {
		let now = mcp_now_rfc3339();

		AutonomySignalInput {
			project_id: project_id.to_owned(),
			objective_id: self.objective_id,
			objective_version: self.objective_version,
			source_type: self.source_type,
			source_refs: self.source_refs,
			primary_source_refs: self.primary_source_refs,
			issue_id: self.issue_id,
			run_id: self.run_id,
			attempt_id: self.attempt_id,
			head_sha: self.head_sha,
			captured_at: self.captured_at.unwrap_or_else(|| now.clone()),
			freshness: self.freshness,
			summary: self.summary,
			evidence: self.evidence,
			evidence_class: self.evidence_class,
			contradictions: self.contradictions,
			gaps: self.gaps,
			confidence: self.confidence,
			privacy: self.privacy,
			observed_counts: self.observed_counts,
			review_evidence: self.review_evidence,
			proposal_only: self.proposal_only.unwrap_or(true),
			created_at: self.created_at.unwrap_or(now),
		}
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyCompileProposalToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	proposal: AutonomyProposalCompileArgs,
	#[serde(default)]
	signal_ids: Vec<String>,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyProposalCompileArgs {
	objective_id: String,
	objective_version: u64,
	source_family: String,
	intended_surface: String,
	#[serde(default)]
	affected_identifiers: Vec<String>,
	summary: String,
	#[serde(default)]
	challenge_requirements: Vec<String>,
	#[serde(default)]
	rejected_alternatives: Vec<String>,
	rollback_path: String,
	#[serde(default)]
	weakened_validation_or_review: Vec<String>,
	created_at: Option<String>,
}
impl AutonomyProposalCompileArgs {
	fn into_compile_input(self, project_id: &str) -> Result<AutonomyProposalCompileInput, Value> {
		if self.objective_version == 0 {
			return Err(invalid_tool_arguments(
				TOOL_AUTONOMY_COMPILE_PROPOSAL,
				"`proposal.objectiveVersion` must be greater than zero.",
			));
		}

		Ok(AutonomyProposalCompileInput {
			project_id: project_id.to_owned(),
			objective_id: self.objective_id,
			objective_version: self.objective_version,
			source_family: self.source_family,
			intended_surface: self.intended_surface,
			affected_identifiers: self.affected_identifiers,
			summary: self.summary,
			challenge_requirements: self.challenge_requirements,
			rejected_alternatives: self.rejected_alternatives,
			rollback_path: self.rollback_path,
			weakened_validation_or_review: self.weakened_validation_or_review,
			created_at: self.created_at.unwrap_or_else(mcp_now_rfc3339),
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyChallengeProposalToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	proposal_id: String,
	challenge: AutonomyProposalChallengeArgs,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyProposalChallengeArgs {
	source: AutonomyProposalChallengeSource,
	actor: String,
	summary: String,
	#[serde(default)]
	objections: Vec<String>,
	#[serde(default)]
	evidence_refs: Vec<String>,
	recorded_at: Option<String>,
}
impl AutonomyProposalChallengeArgs {
	fn into_challenge_input(self) -> Result<AutonomyProposalChallengeInput, Value> {
		if non_empty_string(Some(self.actor.as_str())).is_none()
			|| non_empty_string(Some(self.summary.as_str())).is_none()
		{
			return Err(invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"`challenge.actor` and `challenge.summary` are required.",
			));
		}

		Ok(AutonomyProposalChallengeInput {
			source: self.source,
			actor: self.actor,
			summary: self.summary,
			objections: self.objections,
			evidence_refs: self.evidence_refs,
			recorded_at: self.recorded_at.unwrap_or_else(mcp_now_rfc3339),
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyRequestPromotionToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	proposal_id: String,
	authority: Option<AutonomyProposalAcceptanceArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyProposalAcceptanceArgs {
	accepted_by: String,
	accepted_by_kind: AutonomyProposalAuthorityActorKind,
	accepted_at: Option<String>,
	acceptance_source: String,
	reason: String,
	proposal_actor: String,
	proposal_actor_kind: AutonomyProposalAuthorityActorKind,
	accepted_project_policy: Option<Value>,
}
impl AutonomyProposalAcceptanceArgs {
	fn into_decision_bridge_authority(
		self,
	) -> Result<AutonomyProposalDecisionBridgeAuthority, Value> {
		if self.accepted_project_policy.is_some() {
			return Err(tool_refusal(
				"autonomy_policy_authority_refused",
				"acceptedProjectPolicy must be resolved from trusted Decodex authority state; MCP request payloads cannot prove accepted policy authority.",
			));
		}

		AutonomyProposalDecisionBridgeAuthority::new(
			self.accepted_by,
			self.accepted_by_kind,
			self.accepted_at.unwrap_or_else(mcp_now_rfc3339),
			self.acceptance_source,
			self.reason,
			self.proposal_actor,
			self.proposal_actor_kind,
			None,
		)
		.map_err(|error| {
			tool_refusal(
				"autonomy_acceptance_authority_refused",
				format!("Autonomy proposal acceptance authority was refused: {error}"),
			)
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanningAuthorityArgs {
	source: Option<String>,
	reason: Option<String>,
	accepted_by: Option<String>,
	accepted_at: Option<String>,
	acceptance_source: Option<String>,
	run_id: Option<String>,
	expected_turn_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LaneControlToolArgs {
	action: String,
	project_id: Option<String>,
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
	allow_hard_fallback: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectControlToolArgs {
	action: String,
	project_id: Option<String>,
	authority: Option<ProjectControlAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectControlAuthorityArgs {
	reason: Option<String>,
	source: Option<String>,
	acknowledge_future_dispatch_only: Option<bool>,
}

struct McpTool {
	required_profile: McpCapabilityProfile,
	value: Value,
}

struct McpDryRunTracker;
impl IssueTracker for McpDryRunTracker {
	fn list_issues_with_label(
		&self,
		_label_name: &str,
	) -> crate::prelude::Result<Vec<TrackerIssue>> {
		Ok(Vec::new())
	}

	fn find_team_label_id(
		&self,
		_team_id: &str,
		_label_name: &str,
	) -> crate::prelude::Result<Option<String>> {
		Ok(None)
	}

	fn get_issue_by_identifier(
		&self,
		_issue_identifier: &str,
	) -> crate::prelude::Result<Option<TrackerIssue>> {
		Ok(None)
	}

	fn refresh_issues(&self, _issue_ids: &[String]) -> crate::prelude::Result<Vec<TrackerIssue>> {
		Ok(Vec::new())
	}

	fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
		Ok(Vec::new())
	}

	fn update_issue_state(&self, _issue_id: &str, _state_id: &str) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not mutate issue state.")
	}

	fn add_issue_labels(
		&self,
		_issue_id: &str,
		_label_ids: &[String],
	) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not mutate labels.")
	}

	fn remove_issue_labels(
		&self,
		_issue_id: &str,
		_label_ids: &[String],
	) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not mutate labels.")
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not create comments.")
	}

	fn create_issue(&self, _request: &TrackerIssueCreate) -> crate::prelude::Result<TrackerIssue> {
		eyre::bail!("MCP dry-run tracker does not create issues.")
	}

	fn update_issue_brief(
		&self,
		_issue_id: &str,
		_request: &TrackerIssueBriefUpdate,
	) -> crate::prelude::Result<TrackerIssue> {
		eyre::bail!("MCP dry-run tracker does not update issue briefs.")
	}
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

#[derive(Clone, Default)]
struct McpHttpAuthorization {
	token: Option<String>,
}
impl McpHttpAuthorization {
	fn disabled() -> Self {
		Self { token: None }
	}

	fn from_env_var_name(env_var: Option<&str>) -> crate::prelude::Result<Self> {
		let Some(env_var) = env_var else {
			return Ok(Self::disabled());
		};

		validate_mcp_bearer_token_env_var_name(env_var)?;

		let token = env::var(env_var).map_err(|_| {
			eyre::eyre!(
				"Streamable HTTP bearer token env var `{env_var}` is not set; set it or remove --bearer-token-env."
			)
		})?;

		validate_mcp_bearer_token(&token, env_var)?;

		Ok(Self { token: Some(token) })
	}

	fn is_required(&self) -> bool {
		self.token.is_some()
	}

	fn request_is_authorized(&self, request: &McpHttpRequest) -> bool {
		let Some(expected) = self.token.as_deref() else {
			return true;
		};
		let Some(header) = request.header(MCP_AUTHORIZATION_HEADER) else {
			return false;
		};
		let Some((scheme, supplied)) = header.trim().split_once(' ') else {
			return false;
		};

		scheme.eq_ignore_ascii_case("Bearer") && supplied == expected
	}

	fn unauthorized_response() -> McpHttpResponse {
		let mut response = McpHttpResponse::json_error(
			"401 Unauthorized",
			json_rpc_error(Value::Null, -32_000, "Unauthorized"),
		);

		response.headers.push(("WWW-Authenticate", String::from(MCP_WWW_AUTHENTICATE_HEADER)));

		response
	}

	#[cfg(test)]
	fn from_token_for_test(token: &str) -> Self {
		Self { token: Some(token.to_owned()) }
	}
}

struct McpHttpHandler {
	server: McpServer,
	sessions: McpHttpSessions,
	allowed_origins: Vec<String>,
	listen_address: Option<String>,
	authorization: McpHttpAuthorization,
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
			Err(()) => {
				return Ok(McpHttpResponse::json_error(
					"403 Forbidden",
					json_rpc_error(Value::Null, -32_000, "Forbidden origin"),
				));
			},
		};
		let mut response = if request.path != MCP_HTTP_ENDPOINT_PATH {
			McpHttpResponse::empty("404 Not Found")
		} else if request.method != "OPTIONS" && !self.authorization.request_is_authorized(&request)
		{
			McpHttpAuthorization::unauthorized_response()
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
			Err(_) => {
				return Ok(McpHttpResponse::json_error(
					"400 Bad Request",
					json_rpc_error(Value::Null, -32_700, "Parse error"),
				));
			},
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

struct PromotionAuthority<'a> {
	accepted_by: &'a str,
	accepted_at: Option<&'a String>,
	acceptance_source: &'a str,
	reason: Option<&'a String>,
}

struct LaneControlAuthority<'a> {
	reason: &'a str,
	source: &'a str,
	inspected_run_id: &'a str,
	expected_turn_id: Option<&'a str>,
	allow_hard_fallback: bool,
}

struct ProjectControlAuthority<'a> {
	reason: &'a str,
	source: &'a str,
	acknowledge_future_dispatch_only: bool,
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

fn docs_resource_templates() -> Vec<Value> {
	resource_template_values(&[
		(
			"decodex://docs/spec/{topic}",
			"Decodex specs",
			"Checked-in normative Decodex specification concepts.",
			"text/markdown",
		),
		(
			"decodex://docs/runbook/{topic}",
			"Decodex runbooks",
			"Checked-in Decodex operator procedures.",
			"text/markdown",
		),
		(
			"decodex://docs/reference/{topic}",
			"Decodex references",
			"Checked-in Decodex implementation and current-state references.",
			"text/markdown",
		),
		(
			"decodex://docs/decisions/{topic}",
			"Decodex decisions",
			"Checked-in Decodex design-rationale concepts.",
			"text/markdown",
		),
		(
			"decodex://research/{concept}",
			"Decodex research concepts",
			"Checked-in Markdown Research Contract concepts.",
			"text/markdown",
		),
	])
}

fn runtime_resource_templates() -> Vec<Value> {
	resource_template_values(&[
		(
			"decodex://decision-contracts/{contract_id}",
			"Runtime Decision Contracts",
			"Local runtime Decision Contract readback by contract id.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/status",
			"Project status",
			"Local runtime project status readback.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/status_live",
			"Project live status",
			"Remote-safe current operation, phase, event counts, progress diagnostics, and validation status.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/activity_tail",
			"Project activity tail",
			"Remote-safe activity readback for current and recent runs.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/lane_inspect/{issue}",
			"Lane inspect readback",
			"Read-only lane inspect alias for remote-safe current-lane state.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/lane-control/{issue}",
			"Lane-control readback",
			"Inspect one local lane before requesting guarded lane-control actions.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/events",
			"Run event readback",
			"Remote-safe event counts for a run visible in the current/recent status snapshot.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/protocol_activity",
			"Run protocol activity",
			"Remote-safe protocol activity for a run visible in the current/recent status snapshot, without hidden reasoning or raw payloads.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/child_agent_activity",
			"Run child-agent activity",
			"Remote-safe child-agent activity for a run visible in the current/recent status snapshot.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/progress_diagnostics",
			"Run progress diagnostics",
			"Remote-safe progress and suspected-stall diagnostics for a run visible in the current/recent status snapshot.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/pr_review_state",
			"PR/review state",
			"Remote-safe PR and review-state readback.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy",
			"Autonomy summaries",
			"Read-only project autonomy objective, signal, proposal, and evidence summaries.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/objectives/{objective_id}/current",
			"Current autonomy objective",
			"Read-only current accepted Objective Contract summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/objectives/{objective_id}/{version}",
			"Autonomy objective version",
			"Read-only Objective Contract version summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/signals",
			"Autonomy signals",
			"Read-only recent autonomy signal summaries.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/signals/{signal_id}",
			"Autonomy signal",
			"Read-only autonomy signal summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/proposals",
			"Autonomy proposals",
			"Read-only recent autonomy proposal summaries.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/proposals/{proposal_id}",
			"Autonomy proposal",
			"Read-only autonomy proposal summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/evidence",
			"Autonomy evidence summaries",
			"Read-only evidence summary counts and refs derived from recent signals and proposals.",
			"application/json",
		),
	])
}

fn resource_template_values(templates: &[(&str, &str, &str, &str)]) -> Vec<Value> {
	templates
		.iter()
		.map(|(uri_template, name, description, mime_type)| {
			serde_json::json!({
				"uriTemplate": uri_template,
				"name": name,
				"description": description,
				"mimeType": mime_type
			})
		})
		.collect()
}

fn validate_mcp_bearer_token_env_var_name(env_var: &str) -> crate::prelude::Result<()> {
	if env_var.is_empty() || env_var.trim() != env_var {
		eyre::bail!("--bearer-token-env must name a non-empty environment variable.");
	}

	let mut chars = env_var.chars();
	let Some(first) = chars.next() else {
		eyre::bail!("--bearer-token-env must name a non-empty environment variable.");
	};

	if !(first.is_ascii_alphabetic() || first == '_')
		|| !chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
	{
		eyre::bail!(
			"--bearer-token-env must start with an ASCII letter or underscore and contain only ASCII letters, digits, or underscores."
		);
	}

	Ok(())
}

fn validate_mcp_bearer_token(token: &str, env_var: &str) -> crate::prelude::Result<()> {
	if token.is_empty() || token.trim().is_empty() {
		eyre::bail!("Streamable HTTP bearer token env var `{env_var}` is empty.");
	}
	if token.chars().any(char::is_whitespace) {
		eyre::bail!(
			"Streamable HTTP bearer token env var `{env_var}` must not contain whitespace."
		);
	}

	Ok(())
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
	let mut tools = mcp_foundation_tools();

	tools.extend(mcp_autonomy_tools());
	tools.extend(mcp_operator_tools());

	tools
}

fn mcp_foundation_tools() -> Vec<McpTool> {
	vec![
		mcp_tool_entry(
			McpCapabilityProfile::Observe,
			TOOL_OBSERVE,
			"Decodex Observe",
			"Read public-safe local Decodex runtime observability without private evidence payloads.",
			observe_tool_input_schema(),
			observe_tool_output_schema(),
			true,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_PLAN,
			"Decodex Plan",
			"Return the Decodex prompt/resource route for a requested workflow intent.",
			plan_tool_input_schema(),
			plan_tool_output_schema(),
			true,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_RESEARCH_COMPILE,
			"Decodex Research Compile",
			"Validate or persist a latent Decodex Decision Contract from bounded research input.",
			research_compile_tool_input_schema(),
			research_compile_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_RESEARCH_PROMOTE,
			"Decodex Research Promote",
			"Inspect or explicitly promote a latent Decision Contract through Decodex authority checks.",
			research_promote_tool_input_schema(),
			research_promote_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_INTAKE_GOAL,
			"Decodex Goal Intake",
			"Dry-run or explicitly apply promoted-goal Program Intake through Decodex authority gates.",
			intake_goal_tool_input_schema(),
			intake_goal_tool_output_schema(),
			false,
		),
	]
}

fn mcp_autonomy_tools() -> Vec<McpTool> {
	vec![
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_DRAFT_OBJECTIVE,
			"Decodex Autonomy Draft Objective",
			"Validate or persist a draft Objective Contract without granting acceptance authority.",
			autonomy_draft_objective_tool_input_schema(),
			autonomy_objective_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
			"Decodex Autonomy Accept Objective",
			"Accept a draft Objective Contract version as project-level autonomy authority without starting execution.",
			autonomy_accept_objective_tool_input_schema(),
			autonomy_objective_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_SUBMIT_SIGNAL,
			"Decodex Autonomy Submit Signal",
			"Validate or persist proposal-only autonomy signal evidence under an accepted objective.",
			autonomy_submit_signal_tool_input_schema(),
			autonomy_signal_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_COMPILE_PROPOSAL,
			"Decodex Autonomy Compile Proposal",
			"Compile or persist non-executable autonomy proposal evidence from accepted objective-bound signals.",
			autonomy_compile_proposal_tool_input_schema(),
			autonomy_proposal_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
			"Decodex Autonomy Challenge Proposal",
			"Dry-run or record challenge evidence for an autonomy proposal without making it acceptance authority.",
			autonomy_challenge_proposal_tool_input_schema(),
			autonomy_challenge_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_REQUEST_PROMOTION,
			"Decodex Autonomy Request Promotion",
			"Inspect or explicitly accept an autonomy proposal into a latent Decision Contract candidate.",
			autonomy_request_promotion_tool_input_schema(),
			autonomy_promotion_request_tool_output_schema(),
			false,
		),
	]
}

fn mcp_operator_tools() -> Vec<McpTool> {
	vec![
		mcp_tool_entry(
			McpCapabilityProfile::Operate,
			TOOL_LANE_CONTROL,
			"Decodex Lane Control",
			"Inspect a lane or request guarded soft lane-control actions with explicit authority.",
			lane_control_tool_input_schema(),
			lane_control_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Admin,
			TOOL_PROJECT_CONTROL,
			"Decodex Project Control",
			"Pause or resume future project dispatch through the registered project enablement guard.",
			project_control_tool_input_schema(),
			project_control_tool_output_schema(),
			false,
		),
	]
}

fn mcp_tool_entry(
	profile: McpCapabilityProfile,
	name: &str,
	title: &str,
	description: &str,
	input_schema: Value,
	output_schema: Value,
	read_only: bool,
) -> McpTool {
	McpTool {
		required_profile: profile,
		value: mcp_tool_value(
			name,
			title,
			description,
			profile,
			input_schema,
			output_schema,
			read_only,
		),
	}
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
		TOOL_RESEARCH_COMPILE
		| TOOL_RESEARCH_PROMOTE
		| TOOL_INTAKE_GOAL
		| TOOL_AUTONOMY_DRAFT_OBJECTIVE
		| TOOL_AUTONOMY_ACCEPT_OBJECTIVE
		| TOOL_AUTONOMY_SUBMIT_SIGNAL
		| TOOL_AUTONOMY_COMPILE_PROPOSAL
		| TOOL_AUTONOMY_CHALLENGE_PROPOSAL
		| TOOL_AUTONOMY_REQUEST_PROMOTION => Some(McpCapabilityProfile::Plan),
		TOOL_LANE_CONTROL => Some(McpCapabilityProfile::Operate),
		TOOL_PROJECT_CONTROL => Some(McpCapabilityProfile::Admin),
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

fn research_compile_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run validates without persistence; apply persists a latent Decision Contract."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"input": {
				"type": "object",
				"additionalProperties": true,
				"description": "Structured Decodex research/design input."
			},
			"intent": {
				"type": "string",
				"description": "Minimal natural-language research/design intent."
			},
			"sourceIssue": {
				"type": "string",
				"description": "Optional source tracker issue identifier for minimal intent intake."
			},
			"outcome": {
				"type": "string",
				"enum": ["decision_ready", "not_decision_ready", "blocked", "needs_human_decision"]
			},
			"authority": planning_authority_input_schema()
		}
	})
}

fn research_promote_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run inspects readiness; apply records explicit acceptance."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"contractId": {
				"type": "string",
				"description": "Decision Contract identifier to inspect or promote."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["contractId"]
	})
}

fn intake_goal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run previews generated issues; apply materializes only with explicit authority."
			},
			"contractId": {
				"type": "string",
				"description": "Promoted Decision Contract identifier to materialize."
			},
			"teamIssueIdentifier": {
				"type": "string",
				"description": "Optional source issue used to anchor generated issue team/state on apply."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["contractId"]
	})
}

fn autonomy_draft_objective_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run validates the Objective Contract; apply persists a draft only."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"objective": {
				"type": "object",
				"additionalProperties": true,
				"description": "decodex.autonomy_objective/1 payload with state=draft."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["objective"]
	})
}

fn autonomy_accept_objective_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run inspects the draft acceptance target; apply accepts the draft Objective Contract version."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"objectiveId": {
				"type": "string",
				"description": "Objective Contract id to accept."
			},
			"objectiveVersion": {
				"type": "integer",
				"minimum": 1,
				"description": "Draft Objective Contract version to accept."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"description": "Explicit human/operator objective acceptance authority. Runtime-policy acceptance requires trusted Decodex state and is not accepted from caller-supplied fields.",
				"properties": {
					"acceptedBy": {
						"type": "string",
						"description": "Human or operator actor accepting the Objective Contract."
					},
					"acceptedByKind": {
						"type": "string",
						"enum": ["user"],
						"description": "Only direct user/operator acceptance is accepted through this tool until trusted runtime-policy resolution exists."
					},
					"acceptedAt": {
						"type": "string",
						"description": "Optional RFC3339 acceptance timestamp; Decodex fills the current time when omitted."
					},
					"acceptanceSource": {
						"type": "string",
						"description": "Source of the explicit acceptance, such as conversation or operator command."
					}
				},
				"required": ["acceptedBy", "acceptedByKind", "acceptanceSource"]
			}
		},
		"required": ["objectiveId", "objectiveVersion"]
	})
}

fn autonomy_submit_signal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run validates the signal; apply persists proposal-only signal evidence."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"kind": {
				"type": "string",
				"enum": [
					"runtime_health",
					"validation_regression",
					"review_feedback_cluster",
					"user_feedback_cluster",
					"spec_drift",
					"protocol_drift",
					"metric_regression",
					"execution_friction",
					"docs_skill_drift"
				]
			},
			"signal": {
				"type": "object",
				"additionalProperties": true,
				"description": "Signal input without derived id/fingerprint; Decodex derives stable identity."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["kind", "signal"]
	})
}

fn autonomy_compile_proposal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run compiles non-executable proposal evidence; apply persists it."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"proposal": {
				"type": "object",
				"additionalProperties": true,
				"description": "Autonomy proposal compile input."
			},
			"signalIds": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Persisted autonomy signal ids to bind into the proposal."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["proposal"]
	})
}

fn autonomy_challenge_proposal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run previews the challenge effect; apply records challenge evidence."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"proposalId": {
				"type": "string",
				"description": "Stable autonomy proposal id."
			},
			"challenge": {
				"type": "object",
				"additionalProperties": true,
				"description": "Challenge evidence. It is not acceptance authority."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["proposalId", "challenge"]
	})
}

fn autonomy_request_promotion_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run explains required authority; apply creates a latent Decision Contract candidate only with explicit proposal acceptance authority."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"proposalId": {
				"type": "string",
				"description": "Stable autonomy proposal id."
			},
			"authority": {
				"type": "object",
				"additionalProperties": true,
				"description": "Explicit proposal acceptance authority, including acceptedBy, acceptedByKind, acceptanceSource, reason, proposalActor, and proposalActorKind. acceptedProjectPolicy payloads are refused because trusted policy authority must be resolved from Decodex state."
			}
		},
		"required": ["proposalId"]
	})
}

fn planning_authority_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"source": {
				"type": "string",
				"description": "Explicit remote client or operator source for an apply-style call."
			},
			"reason": {
				"type": "string",
				"description": "Explicit reason authorizing an apply-style call."
			},
			"acceptedBy": {
				"type": "string",
				"description": "Actor accepting a Decision Contract promotion."
			},
			"acceptedAt": {
				"type": "string",
				"description": "Optional RFC3339 acceptance timestamp."
			},
			"acceptanceSource": {
				"type": "string",
				"description": "Conversation, issue, or policy source for explicit promotion authority."
			},
			"runId": {
				"type": "string",
				"description": "Current lane run id when a future planning mutation is lane-scoped."
			},
			"expectedTurnId": {
				"type": "string",
				"description": "Current lane turn id when a future planning mutation is lane-scoped."
			}
		}
	})
}

fn lane_control_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"action": {
				"type": "string",
				"enum": ["inspect", "interrupt", "steer", "manual_attention", "retained_resume"]
			},
			"projectId": {
				"type": "string",
				"description": "Optional project id precondition. When supplied, it must match the MCP gateway project context."
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
					},
					"allowHardFallback": {
						"type": "boolean",
						"description": "Explicit acknowledgement required with force=true before hard interrupt fallback can run."
					}
				}
			}
		},
		"required": ["action"]
	})
}

fn project_control_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"action": {
				"type": "string",
				"enum": ["status", "pause", "resume", "scan"],
				"description": "Project-control action. Pause/resume only affect future dispatch."
			},
			"projectId": {
				"type": "string",
				"description": "Registered Decodex project id. Optional only when the gateway was started with a project config."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"reason": {
						"type": "string",
						"description": "Explicit operator reason for pause or resume."
					},
					"source": {
						"type": "string",
						"description": "Remote client or operator source identifier."
					},
					"acknowledgeFutureDispatchOnly": {
						"type": "boolean",
						"description": "Must be true for pause/resume; active lanes are not killed."
					}
				}
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

fn research_compile_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.research_compile_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"contract_id": { "type": "string" },
			"contract_status": {
				"type": "string",
				"enum": ["draft_latent", "accepted_promoted", "rejected_superseded", "needs_human_decision"]
			},
			"ready_for_issue_shaping": { "type": "boolean" },
			"issue_generation_ready_after_promotion": { "type": "boolean" },
			"execution_authority_granted": { "type": "boolean" },
			"proposed_issue_count": { "type": "integer", "minimum": 0 },
			"promotion_targets": { "type": "array", "items": { "type": "string" } },
			"conflict_domains": { "type": "array", "items": { "type": "string" } },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"contract_id",
			"contract_status",
			"ready_for_issue_shaping",
			"issue_generation_ready_after_promotion",
			"execution_authority_granted",
			"proposed_issue_count",
			"promotion_targets",
			"conflict_domains",
			"next_action"
		]
	}))
}

fn research_promote_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.research_promote_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"contract_id": { "type": "string" },
			"contract_status": {
				"type": "string",
				"enum": ["draft_latent", "accepted_promoted", "rejected_superseded", "needs_human_decision"]
			},
			"execution_authority_granted": { "type": "boolean" },
			"ready_for_issue_shaping": { "type": "boolean" },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"contract_id",
			"contract_status",
			"execution_authority_granted",
			"ready_for_issue_shaping",
			"next_action"
		]
	}))
}

fn intake_goal_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.intake_goal_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"service_id": { "type": "string" },
			"contract_id": { "type": "string" },
			"dry_run": { "type": "boolean" },
			"applied": { "type": "boolean" },
			"persisted": { "type": "boolean" },
			"issue_count": { "type": "integer", "minimum": 0 },
			"issues": {
				"type": "array",
				"items": {
					"type": "object",
					"additionalProperties": false,
					"properties": {
						"title": { "type": "string" },
						"objective": { "type": "string" },
						"issue_identifier": { "type": ["string", "null"] },
						"action": { "type": "string" },
						"dependencies": { "type": "array", "items": { "type": "string" } },
						"conflict_domains": { "type": "array", "items": { "type": "string" } },
						"acceptance": { "type": "array", "items": { "type": "string" } },
						"validation": { "type": "array", "items": { "type": "string" } },
						"reasons": { "type": "array", "items": { "type": "string" } }
					},
					"required": [
						"title",
						"objective",
						"issue_identifier",
						"action",
						"dependencies",
						"conflict_domains",
						"acceptance",
						"validation",
						"reasons"
					]
				}
			},
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"service_id",
			"contract_id",
			"dry_run",
			"applied",
			"persisted",
			"issue_count",
			"issues",
			"next_action"
		]
	}))
}

fn autonomy_objective_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_objective_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"objective": { "type": "object", "additionalProperties": true },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"objective",
			"authority_effect",
			"next_action"
		]
	}))
}

fn autonomy_signal_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_signal_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"signal": { "type": "object", "additionalProperties": true },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"signal",
			"authority_effect",
			"next_action"
		]
	}))
}

fn autonomy_proposal_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_proposal_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal": { "type": "object", "additionalProperties": true },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"proposal",
			"authority_effect",
			"next_action"
		]
	}))
}

fn autonomy_challenge_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_challenge_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal": { "type": "object", "additionalProperties": true },
			"challenge_evidence_count": { "type": "integer", "minimum": 0 },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"proposal",
			"challenge_evidence_count",
			"authority_effect",
			"next_action"
		]
	}))
}

fn autonomy_promotion_request_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_promotion_request_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal": { "type": "object", "additionalProperties": true },
			"decision_contract_id": { "type": ["string", "null"] },
			"execution_authority_granted": { "type": "boolean" },
			"required_authority": { "type": "array", "items": { "type": "string" } },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"proposal",
			"execution_authority_granted",
			"required_authority",
			"authority_effect",
			"next_action"
		]
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
				"enum": ["ok", "queued", "refused"]
			},
			"reason": {
				"type": "string"
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
				"enum": ["inspect", "interrupt", "steer", "manual_attention", "retained_resume"]
			},
			"project_id": {
				"type": ["string", "null"]
			},
			"issue": {
				"type": ["string", "null"]
			},
			"run_id": {
				"type": ["string", "null"]
			},
			"preconditions": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"project_id_present": { "type": "boolean" },
					"issue_present": { "type": "boolean" },
					"run_id_present": { "type": "boolean" },
					"expected_turn_id_present": { "type": "boolean" },
					"message_present": { "type": "boolean" },
					"force_requested": { "type": "boolean" },
					"authority_reason_present": { "type": "boolean" },
					"authority_source_present": { "type": "boolean" },
					"authority_inspected_run_id_present": { "type": "boolean" },
					"authority_expected_turn_id_present": { "type": "boolean" },
					"authority_allow_hard_fallback": { "type": "boolean" }
				},
				"required": [
					"project_id_present",
					"issue_present",
					"run_id_present",
					"expected_turn_id_present",
					"message_present",
					"force_requested",
					"authority_reason_present",
					"authority_source_present",
					"authority_inspected_run_id_present",
					"authority_expected_turn_id_present",
					"authority_allow_hard_fallback"
				]
			},
			"result": {
				"type": "object",
				"additionalProperties": true
			}
		},
		"required": [
			"schema",
			"status",
			"reason",
			"message",
			"capability_profile",
			"action",
			"preconditions",
			"result"
		]
	}))
}

fn project_control_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.project_control_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok", "refused"]
			},
			"reason": {
				"type": "string"
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
				"enum": ["status", "pause", "resume", "scan"]
			},
			"project_id": {
				"type": ["string", "null"]
			},
			"future_dispatch_only": {
				"type": "boolean"
			},
			"result": {
				"type": "object",
				"additionalProperties": true
			}
		},
		"required": [
			"schema",
			"status",
			"reason",
			"message",
			"capability_profile",
			"action",
			"future_dispatch_only",
			"result"
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
		Err(_) => {
			return invalid_tool_arguments(
				TOOL_PLAN,
				"`intent` is required and must be one of research, validation_ready, handoff, or lane_control.",
			);
		},
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
			"Inspect first; then call guarded MCP lane-control with explicit authority and current run/turn preconditions.",
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

fn research_compile_input(
	params: ResearchCompileToolArgs,
) -> Result<ResearchDesignRunInput, Value> {
	match (params.input, params.intent) {
		(Some(input), None) => Ok(input),
		(None, Some(intent)) => Ok(ResearchDesignRunInput::from_intent(
			intent,
			params.source_issue,
			params.outcome.unwrap_or(ResearchDesignOutcome::NotDecisionReady),
		)),
		(None, None) => Err(invalid_tool_arguments(
			TOOL_RESEARCH_COMPILE,
			"research_compile requires either `input` or `intent`.",
		)),
		(Some(_), Some(_)) => Err(invalid_tool_arguments(
			TOOL_RESEARCH_COMPILE,
			"research_compile accepts `input` or `intent`, not both.",
		)),
	}
}

fn research_compile_result(report: &ResearchDesignRunReport, persisted: bool, mode: &str) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.research_compile_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"contract_id": report.contract_id,
		"contract_status": report.contract_status.as_str(),
		"ready_for_issue_shaping": report.ready_for_issue_shaping,
		"issue_generation_ready_after_promotion": report.issue_generation_ready_after_promotion,
		"execution_authority_granted": report.execution_authority_granted,
		"proposed_issue_count": report.proposed_issues.len(),
		"promotion_targets": report.promotion_targets,
		"conflict_domains": report.conflict_domains,
		"next_action": if persisted {
			"Promote the Decision Contract only after explicit acceptance."
		} else {
			"Re-run with mode=apply and explicit authority to persist a latent Decision Contract."
		}
	})
}

fn research_promote_readiness_result(
	contract_id: &str,
	contract_status: &str,
	ready_for_issue_shaping: bool,
	persisted: bool,
	mode: &str,
) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.research_promote_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"contract_id": contract_id,
		"contract_status": contract_status,
		"execution_authority_granted": persisted && contract_status == "accepted_promoted",
		"ready_for_issue_shaping": ready_for_issue_shaping,
		"next_action": if persisted {
			"Use intake_goal dry_run to inspect issue shaping before apply."
		} else {
			"Re-run with mode=apply and explicit acceptance authority to promote."
		}
	})
}

fn intake_goal_result(report: &GoalIntakeReport, mode: &str) -> Value {
	let issues = report.issues.iter().map(intake_goal_issue_result).collect::<Vec<_>>();

	serde_json::json!({
		"schema": "decodex.mcp.intake_goal_result/1",
		"status": "ok",
		"mode": mode,
		"service_id": report.service_id,
		"contract_id": report.contract_id,
		"dry_run": report.dry_run,
		"applied": report.applied,
		"persisted": report.persisted,
		"issue_count": issues.len(),
		"issues": issues,
		"next_action": if report.persisted {
			"Let the Program scheduler dispatch ready mapped issues; do not add queue labels manually."
		} else {
			"Review the public issue split, then re-run with mode=apply and explicit authority if accepted."
		}
	})
}

fn intake_goal_issue_result(row: &GoalIntakeIssueReport) -> Value {
	serde_json::json!({
		"title": row.title,
		"objective": row.objective,
		"issue_identifier": row.issue_identifier,
		"action": goal_intake_action_name(row.action),
		"dependencies": row.dependencies,
		"conflict_domains": row.conflict_domains,
		"acceptance": row.acceptance,
		"validation": row.validation,
		"reasons": row.reasons
	})
}

fn autonomy_signal_from_tool_args(
	kind: AutonomySignalKind,
	input: AutonomySignalInputArgs,
	project_id: &str,
) -> Result<AutonomySignal, Value> {
	let input = input.into_signal_input(project_id);
	let signal = match kind {
		AutonomySignalKind::RuntimeHealth => AutonomySignal::runtime_health(input),
		AutonomySignalKind::ValidationRegression => AutonomySignal::validation_regression(input),
		AutonomySignalKind::ReviewFeedbackCluster => AutonomySignal::review_feedback_cluster(input),
		AutonomySignalKind::UserFeedbackCluster => AutonomySignal::user_feedback_cluster(input),
		AutonomySignalKind::SpecDrift => AutonomySignal::spec_drift(input),
		AutonomySignalKind::ProtocolDrift => AutonomySignal::protocol_drift(input),
		AutonomySignalKind::MetricRegression => AutonomySignal::metric_regression(input),
		AutonomySignalKind::ExecutionFriction => AutonomySignal::execution_friction(input),
		AutonomySignalKind::DocsSkillDrift => AutonomySignal::docs_skill_drift(input),
	};

	signal.map_err(|error| {
		tool_refusal(
			"autonomy_signal_refused",
			format!("Autonomy signal did not satisfy Decodex signal requirements: {error}"),
		)
	})
}

fn autonomy_objective_tool_result(
	project_id: &str,
	objective: &AutonomyObjectiveContract,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"objective": mcp_autonomy_objective_summary(objective, updated_at),
		"authority_effect": "draft_only_no_execution_authority",
		"next_action": "Accept an Objective Contract only through explicit human or accepted-policy authority; MCP profile access is not acceptance authority.",
		"updated_at": updated_at
	}))
}

fn autonomy_objective_accept_tool_result(
	project_id: &str,
	objective: &AutonomyObjectiveContract,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"objective": mcp_autonomy_objective_summary(objective, updated_at),
		"authority_effect": "accepted_objective_no_execution_authority",
		"next_action": "Accepted Objective Contracts allow objective-bound signals and proposals; execution still requires proposal acceptance, Decision Contract promotion, and Program Intake.",
		"updated_at": updated_at
	}))
}

fn autonomy_signal_tool_result(
	project_id: &str,
	signal: &AutonomySignal,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_signal_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"signal": mcp_autonomy_signal_summary(signal, updated_at),
		"authority_effect": "proposal_only_evidence_no_execution_authority",
		"next_action": "Cluster accepted-objective signals into a non-executable proposal before any Decision Contract promotion.",
		"updated_at": updated_at
	}))
}

fn autonomy_proposal_tool_result(
	project_id: &str,
	proposal: &AutonomyProposal,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_proposal_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal": mcp_autonomy_proposal_summary(proposal, updated_at),
		"authority_effect": "non_executable_proposal_evidence",
		"next_action": "Challenge the proposal and request explicit promotion authority before creating a latent Decision Contract candidate.",
		"updated_at": updated_at
	}))
}

fn autonomy_challenge_tool_result(
	project_id: &str,
	proposal: &AutonomyProposal,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_challenge_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal": mcp_autonomy_proposal_summary(proposal, updated_at),
		"challenge_evidence_count": proposal.challenge_evidence().len(),
		"authority_effect": "challenge_evidence_not_acceptance_authority",
		"next_action": "Carry challenge objections as promotion constraints and request explicit promotion authority before creating execution work.",
		"updated_at": updated_at
	}))
}

fn autonomy_promotion_request_result(
	project_id: &str,
	proposal: &AutonomyProposal,
	mode: &str,
	persisted: bool,
	decision_contract_id: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_promotion_request_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal": mcp_autonomy_proposal_summary(proposal, None),
		"decision_contract_id": decision_contract_id,
		"execution_authority_granted": false,
		"required_authority": [
			"acceptedBy",
			"acceptedByKind",
			"acceptanceSource",
			"reason",
			"proposalActor",
			"proposalActorKind",
			"trusted Decodex policy authority when runtime policy or external-agent self-acceptance is involved"
		],
		"authority_effect": if persisted {
			"latent_decision_contract_candidate_only"
		} else {
			"promotion_requirements_readback_only"
		},
		"next_action": if persisted {
			"Promote the resulting Decision Contract through research_promote before Program Intake or issue work."
		} else {
			"Re-run with mode=apply only after explicit proposal acceptance authority is available."
		}
	}))
}

fn goal_intake_action_name(action: program_intake::GoalIntakeIssueAction) -> &'static str {
	match action {
		program_intake::GoalIntakeIssueAction::WouldCreate => "would_create",
		program_intake::GoalIntakeIssueAction::WouldUpdate => "would_update",
		program_intake::GoalIntakeIssueAction::Created => "created",
		program_intake::GoalIntakeIssueAction::Updated => "updated",
	}
}

fn planning_mode(
	mode: Option<&str>,
	default_mode: &'static str,
	tool: &str,
) -> Result<&'static str, Value> {
	let mode = mode.map(str::trim).filter(|mode| !mode.is_empty()).unwrap_or(default_mode);

	match mode {
		"dry_run" => Ok("dry_run"),
		"apply" => Ok("apply"),
		_ => Err(invalid_tool_arguments(tool, "`mode` must be dry_run or apply.")),
	}
}

fn planning_project_id(
	context: &McpContext,
	explicit_project_id: Option<&str>,
	tool: &str,
) -> Result<String, Value> {
	let project_id = explicit_project_id
		.and_then(|value| non_empty_string(Some(value)))
		.or_else(|| context.project_id())
		.ok_or_else(|| {
			tool_refusal(
				"missing_project_context",
				"Planning tools require a project-scoped MCP context or explicit projectId.",
			)
		})?;

	if safe_runtime_identifier(project_id) {
		Ok(project_id.to_owned())
	} else {
		Err(invalid_tool_arguments(tool, "`projectId` must be a safe Decodex runtime identifier."))
	}
}

fn planning_state_store<'a>(context: &'a McpContext, _tool: &str) -> Result<&'a StateStore, Value> {
	context.state_store.as_ref().ok_or_else(|| {
		tool_refusal(
			"missing_runtime_store",
			"Planning apply/readback requires the Decodex runtime store.",
		)
	})
}

fn planning_authority_present(authority: Option<&PlanningAuthorityArgs>) -> bool {
	let Some(authority) = authority else {
		return false;
	};
	let _lane_preconditions = (
		non_empty_string(authority.run_id.as_deref()),
		non_empty_string(authority.expected_turn_id.as_deref()),
	);

	non_empty_string(authority.source.as_deref()).is_some()
		&& non_empty_string(authority.reason.as_deref()).is_some()
}

fn promotion_authority(
	authority: Option<&PlanningAuthorityArgs>,
) -> Result<PromotionAuthority<'_>, Value> {
	let Some(authority) = authority else {
		return Err(missing_authority_refusal(
			TOOL_RESEARCH_PROMOTE,
			"research_promote apply requires authority.acceptedBy and authority.acceptanceSource.",
		));
	};
	let accepted_by = non_empty_string(authority.accepted_by.as_deref()).ok_or_else(|| {
		missing_authority_refusal(
			TOOL_RESEARCH_PROMOTE,
			"research_promote apply requires authority.acceptedBy.",
		)
	})?;
	let acceptance_source =
		non_empty_string(authority.acceptance_source.as_deref()).ok_or_else(|| {
			missing_authority_refusal(
				TOOL_RESEARCH_PROMOTE,
				"research_promote apply requires authority.acceptanceSource.",
			)
		})?;

	Ok(PromotionAuthority {
		accepted_by,
		accepted_at: authority.accepted_at.as_ref(),
		acceptance_source,
		reason: authority.reason.as_ref(),
	})
}

fn missing_authority_refusal(tool: &str, message: &str) -> Value {
	tool_refusal_value(serde_json::json!({
		"schema": "decodex.mcp.refusal/1",
		"status": "refused",
		"reason": "missing_authority",
		"tool": tool,
		"message": message
	}))
}

fn mcp_autonomy_project_resource(
	state_store: &StateStore,
	project_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let objectives = state_store
		.recent_autonomy_objectives_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;
	let signals = state_store
		.recent_autonomy_signals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;
	let proposals = state_store
		.recent_autonomy_proposals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_summary/1",
		"project_id": project_id,
		"read_only": true,
		"authority_boundary": mcp_autonomy_authority_boundary(),
		"objectives": objectives
			.iter()
			.map(|record| mcp_autonomy_objective_summary(record.objective(), Some(record.updated_at())))
			.collect::<Vec<_>>(),
		"signals": signals
			.iter()
			.map(|record| mcp_autonomy_signal_summary(record.signal(), Some(record.updated_at())))
			.collect::<Vec<_>>(),
		"proposals": proposals
			.iter()
			.map(|record| mcp_autonomy_proposal_summary(record.proposal(), Some(record.updated_at())))
			.collect::<Vec<_>>(),
		"evidence": mcp_autonomy_evidence_summary(&signals, &proposals)
	}))
}

fn mcp_autonomy_current_objective_resource(
	state_store: &StateStore,
	project_id: &str,
	objective_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let Some(record) = state_store
		.current_accepted_autonomy_objective(project_id, objective_id)
		.map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_resource/1",
		"project_id": project_id,
		"read_only": true,
		"authority_boundary": mcp_autonomy_authority_boundary(),
		"objective": mcp_autonomy_objective_summary(record.objective(), Some(record.updated_at()))
	}))
}

fn mcp_autonomy_objective_version_resource(
	state_store: &StateStore,
	project_id: &str,
	objective_id: &str,
	version: &str,
) -> crate::prelude::Result<Value, McpError> {
	let version = version.parse::<u64>().map_err(|_| McpError::resource_not_found())?;
	let Some(record) = state_store
		.autonomy_objective(project_id, objective_id, version)
		.map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_resource/1",
		"project_id": project_id,
		"read_only": true,
		"authority_boundary": mcp_autonomy_authority_boundary(),
		"objective": mcp_autonomy_objective_summary(record.objective(), Some(record.updated_at()))
	}))
}

fn mcp_autonomy_signals_resource(
	state_store: &StateStore,
	project_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let signals = state_store
		.recent_autonomy_signals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_signals_resource/1",
		"project_id": project_id,
		"read_only": true,
		"signals": signals
			.iter()
			.map(|record| mcp_autonomy_signal_summary(record.signal(), Some(record.updated_at())))
			.collect::<Vec<_>>()
	}))
}

fn mcp_autonomy_signal_resource(
	state_store: &StateStore,
	project_id: &str,
	signal_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let Some(record) =
		state_store.autonomy_signal(project_id, signal_id).map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_signal_resource/1",
		"project_id": project_id,
		"read_only": true,
		"signal": mcp_autonomy_signal_summary(record.signal(), Some(record.updated_at()))
	}))
}

fn mcp_autonomy_proposals_resource(
	state_store: &StateStore,
	project_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let proposals = state_store
		.recent_autonomy_proposals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_proposals_resource/1",
		"project_id": project_id,
		"read_only": true,
		"proposals": proposals
			.iter()
			.map(|record| mcp_autonomy_proposal_summary(record.proposal(), Some(record.updated_at())))
			.collect::<Vec<_>>()
	}))
}

fn mcp_autonomy_proposal_resource(
	state_store: &StateStore,
	project_id: &str,
	proposal_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let Some(record) =
		state_store.autonomy_proposal(project_id, proposal_id).map_err(McpError::internal)?
	else {
		return Err(McpError::resource_not_found());
	};

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_proposal_resource/1",
		"project_id": project_id,
		"read_only": true,
		"proposal": mcp_autonomy_proposal_summary(record.proposal(), Some(record.updated_at()))
	}))
}

fn mcp_autonomy_evidence_resource(
	state_store: &StateStore,
	project_id: &str,
) -> crate::prelude::Result<Value, McpError> {
	let signals = state_store
		.recent_autonomy_signals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;
	let proposals = state_store
		.recent_autonomy_proposals_for_project(project_id, DEFAULT_MCP_STATUS_LIMIT)
		.map_err(McpError::internal)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.autonomy_evidence_resource/1",
		"project_id": project_id,
		"read_only": true,
		"evidence": mcp_autonomy_evidence_summary(&signals, &proposals)
	}))
}

fn mcp_autonomy_objective_summary(
	objective: &AutonomyObjectiveContract,
	updated_at: Option<&str>,
) -> Value {
	serde_json::json!({
		"objective_id": objective.id(),
		"objective_version": objective.version(),
		"state": objective.state().as_str(),
		"summary": objective.summary(),
		"goals": objective.goals(),
		"non_goals": objective.non_goals(),
		"metrics": objective.metrics(),
		"allowed_surfaces": objective.allowed_surfaces(),
		"allowed_signal_kinds": objective.allowed_signal_kinds(),
		"validation_gates": objective.validation_gates(),
		"review_policy": objective.review_policy(),
		"acceptance_present": objective.acceptance().is_some(),
		"updated_at": updated_at
	})
}

fn mcp_autonomy_signal_summary(signal: &AutonomySignal, updated_at: Option<&str>) -> Value {
	let (source_refs, primary_source_refs, source_ref_count, primary_source_ref_count) =
		mcp_autonomy_signal_ref_summary(signal);

	serde_json::json!({
		"signal_id": signal.id(),
		"objective_id": signal.objective_id(),
		"objective_version": signal.objective_version(),
		"kind": signal.kind().as_str(),
		"source_type": signal.source_type().as_str(),
		"source_refs": source_refs,
		"source_ref_count": source_ref_count,
		"primary_source_refs": primary_source_refs,
		"primary_source_ref_count": primary_source_ref_count,
		"freshness": signal.freshness().as_str(),
		"summary": signal.summary(),
		"evidence_class": signal.evidence_class().as_str(),
		"confidence": signal.confidence().as_str(),
		"redaction_level": signal.privacy().as_str(),
		"gaps": signal.gaps(),
		"contradictions": signal.contradictions(),
		"review_evidence_present": signal.review_evidence().is_some(),
		"updated_at": updated_at
	})
}

fn mcp_autonomy_signal_ref_summary(signal: &AutonomySignal) -> (Value, Value, usize, usize) {
	let source_ref_count = signal.source_refs().len();
	let primary_source_ref_count = signal.primary_source_refs().len();

	if signal.privacy() == AutonomySignalPrivacy::LocalPrivate {
		return (
			serde_json::json!([]),
			serde_json::json!([]),
			source_ref_count,
			primary_source_ref_count,
		);
	}

	(
		serde_json::json!(signal.source_refs()),
		serde_json::json!(signal.primary_source_refs()),
		source_ref_count,
		primary_source_ref_count,
	)
}

fn mcp_autonomy_proposal_summary(proposal: &AutonomyProposal, updated_at: Option<&str>) -> Value {
	serde_json::json!({
		"proposal_id": proposal.id(),
		"objective_id": proposal.objective_id(),
		"objective_version": proposal.objective_version(),
		"state": proposal.state().as_str(),
		"summary": proposal.summary(),
		"source_family": proposal.source_family(),
		"intended_surface": proposal.intended_surface(),
		"affected_identifiers": proposal.affected_identifiers(),
		"source_signal_ids": proposal.source_signal_ids(),
		"allowed_surfaces": proposal.allowed_surfaces(),
		"validation_gates": proposal.validation_gates(),
		"refusal_reasons": proposal
			.refusal_reasons()
			.iter()
			.map(|refusal| refusal.reason().as_str())
			.collect::<Vec<_>>(),
		"refusals": proposal
			.refusal_reasons()
			.iter()
			.map(|refusal| {
				serde_json::json!({
					"reason": refusal.reason().as_str(),
					"detail": refusal.detail(),
					"evidence_refs": refusal.evidence_refs()
				})
			})
			.collect::<Vec<_>>(),
		"gaps": proposal.gaps(),
		"contradictions": proposal.contradictions(),
		"challenge_evidence_count": proposal.challenge_evidence().len(),
		"updated_at": updated_at
	})
}

fn mcp_autonomy_evidence_summary(
	signals: &[AutonomySignalRecord],
	proposals: &[AutonomyProposalRecord],
) -> Value {
	serde_json::json!({
		"signal_count": signals.len(),
		"proposal_count": proposals.len(),
		"signal_refs": signals
			.iter()
			.map(|record| {
				serde_json::json!({
					"signal_id": record.signal_id(),
					"kind": record.kind().as_str(),
					"freshness": record.freshness().as_str(),
					"evidence_class": record.evidence_class().as_str(),
					"confidence": record.confidence().as_str(),
					"redaction_level": record.privacy().as_str()
				})
			})
			.collect::<Vec<_>>(),
		"proposal_refs": proposals
			.iter()
			.map(|record| {
				serde_json::json!({
					"proposal_id": record.proposal_id(),
					"state": record.state().as_str(),
					"objective_id": record.objective_id(),
					"objective_version": record.objective_version()
				})
			})
			.collect::<Vec<_>>(),
		"authority_effect": "evidence_summary_only_no_execution_authority"
	})
}

fn mcp_autonomy_authority_boundary() -> Value {
	serde_json::json!({
		"mcp_authentication": "access_boundary_only",
		"capability_profile": "tool_visibility_boundary_only",
		"acceptance_authority": "explicit_human_or_trusted_accepted_project_policy_required",
		"execution_authority": "Decision Contract promotion and Program Intake remain separate"
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
	let limit = snapshot
		.get("run_limit")
		.and_then(Value::as_u64)
		.and_then(|limit| usize::try_from(limit).ok())
		.unwrap_or(DEFAULT_MCP_STATUS_LIMIT);
	let mut activity = Vec::new();

	for run in mcp_all_runs(&snapshot).into_iter().take(limit) {
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
			let review = mcp_loop_review_status(run)?;

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
			.and_then(mcp_loop_review_status_from_loop_status)
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
		"autonomy": mcp_public_autonomy_status(run),
		"loop_review": run
			.get("loop_status")
			.and_then(mcp_loop_review_status_from_loop_status)
			.map(mcp_public_review_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_public_autonomy_status(run_or_lane: &Value) -> Value {
	let Some(loop_status) = run_or_lane.get("loop_status").filter(|status| status.is_object())
	else {
		return Value::Null;
	};

	serde_json::json!({
		"status": loop_status.get("autonomy").cloned().unwrap_or(Value::Null),
		"summary": loop_status.get("summary").cloned().unwrap_or(Value::Null),
		"objective": loop_status
			.get("autonomy_objective")
			.map(mcp_public_autonomy_objective)
			.unwrap_or(Value::Null),
		"signals": mcp_public_autonomy_signals(loop_status.get("autonomy_signals")),
		"proposals": mcp_public_autonomy_proposals(loop_status.get("autonomy_proposals")),
		"lineage": mcp_public_autonomy_lineage(loop_status.get("autonomy_lineage")),
		"report": loop_status
			.get("autonomy_report")
			.map(mcp_public_autonomy_report)
			.unwrap_or(Value::Null)
	})
}

fn mcp_public_autonomy_objective(objective: &Value) -> Value {
	serde_json::json!({
		"objective_id": objective.get("objective_id").cloned().unwrap_or(Value::Null),
		"objective_version": objective
			.get("objective_version")
			.cloned()
			.unwrap_or(Value::Null),
		"state": objective.get("state").cloned().unwrap_or(Value::Null),
		"source_ref": objective.get("source_ref").cloned().unwrap_or(Value::Null),
		"completeness": objective.get("completeness").cloned().unwrap_or(Value::Null),
		"known_gaps": objective.get("known_gaps").cloned().unwrap_or_else(|| serde_json::json!([]))
	})
}

fn mcp_public_autonomy_signals(signals: Option<&Value>) -> Vec<Value> {
	signals
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.map(|signal| {
			let (source_refs, primary_source_refs) = mcp_public_autonomy_signal_refs(signal);

			serde_json::json!({
				"signal_id": signal.get("signal_id").cloned().unwrap_or(Value::Null),
				"objective_id": signal.get("objective_id").cloned().unwrap_or(Value::Null),
				"objective_version": signal.get("objective_version").cloned().unwrap_or(Value::Null),
				"kind": signal.get("kind").cloned().unwrap_or(Value::Null),
				"source_type": signal.get("source_type").cloned().unwrap_or(Value::Null),
				"source_refs": source_refs,
				"source_ref_count": signal_ref_count(signal, "source_refs", "source_ref_count"),
				"primary_source_refs": primary_source_refs,
				"primary_source_ref_count": signal_ref_count(
					signal,
					"primary_source_refs",
					"primary_source_ref_count"
				),
				"freshness": signal.get("freshness").cloned().unwrap_or(Value::Null),
				"evidence_class": signal.get("evidence_class").cloned().unwrap_or(Value::Null),
				"confidence": signal.get("confidence").cloned().unwrap_or(Value::Null),
				"redaction_level": signal
					.get("redaction_level")
					.cloned()
					.unwrap_or(Value::Null),
				"completeness": signal.get("completeness").cloned().unwrap_or(Value::Null),
				"known_gaps": signal
					.get("known_gaps")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"updated_at": signal.get("updated_at").cloned().unwrap_or(Value::Null)
			})
		})
		.collect()
}

fn mcp_public_autonomy_signal_refs(signal: &Value) -> (Value, Value) {
	if signal.get("redaction_level").and_then(Value::as_str) == Some("local_private") {
		return (serde_json::json!([]), serde_json::json!([]));
	}

	(
		signal.get("source_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
		signal.get("primary_source_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
	)
}

fn signal_ref_count(signal: &Value, refs_key: &str, count_key: &str) -> u64 {
	signal.get(count_key).and_then(Value::as_u64).unwrap_or_else(|| {
		signal.get(refs_key).and_then(Value::as_array).map_or(0, |refs| refs.len() as u64)
	})
}

fn mcp_public_autonomy_proposals(proposals: Option<&Value>) -> Vec<Value> {
	proposals
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.map(|proposal| {
			serde_json::json!({
				"proposal_id": proposal.get("proposal_id").cloned().unwrap_or(Value::Null),
				"objective_id": proposal.get("objective_id").cloned().unwrap_or(Value::Null),
				"objective_version": proposal.get("objective_version").cloned().unwrap_or(Value::Null),
				"state": proposal.get("state").cloned().unwrap_or(Value::Null),
				"summary": proposal.get("summary").cloned().unwrap_or(Value::Null),
				"source_family": proposal.get("source_family").cloned().unwrap_or(Value::Null),
				"intended_surface": proposal
					.get("intended_surface")
					.cloned()
					.unwrap_or(Value::Null),
				"source_signal_ids": proposal
					.get("source_signal_ids")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"refusal_reasons": proposal
					.get("refusal_reasons")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"refusals": proposal
					.get("refusals")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"completeness": proposal.get("completeness").cloned().unwrap_or(Value::Null),
				"known_gaps": proposal
					.get("known_gaps")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"updated_at": proposal.get("updated_at").cloned().unwrap_or(Value::Null)
			})
		})
		.collect()
}

fn mcp_public_autonomy_lineage(lineage: Option<&Value>) -> Vec<Value> {
	lineage.and_then(Value::as_array).into_iter().flatten().cloned().collect()
}

fn mcp_public_autonomy_report(report: &Value) -> Value {
	serde_json::json!({
		"surface": report.get("surface").cloned().unwrap_or(Value::Null),
		"authority": report.get("authority").cloned().unwrap_or(Value::Null),
		"audit_authority": report.get("audit_authority").cloned().unwrap_or(Value::Null),
		"source_refs": report.get("source_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
		"redaction_level": report.get("redaction_level").cloned().unwrap_or(Value::Null),
		"completeness": report.get("completeness").cloned().unwrap_or(Value::Null),
		"known_gaps": report.get("known_gaps").cloned().unwrap_or_else(|| serde_json::json!([]))
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

fn mcp_loop_review_status(run_or_lane: &Value) -> Option<&Value> {
	run_or_lane.get("loop_status").and_then(mcp_loop_review_status_from_loop_status)
}

fn mcp_loop_review_status_from_loop_status(loop_status: &Value) -> Option<&Value> {
	loop_status.get("review").filter(|review| review.is_object())
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

fn lane_control_preconditions(params: &LaneControlToolArgs) -> Value {
	let authority = params.authority.as_ref();

	serde_json::json!({
		"project_id_present": non_empty_string(params.project_id.as_deref()).is_some(),
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
			.is_some(),
		"authority_allow_hard_fallback": authority
			.and_then(|value| value.allow_hard_fallback)
			.unwrap_or(false)
	})
}

fn lane_control_authority(params: &LaneControlToolArgs) -> Option<LaneControlAuthority<'_>> {
	let authority = params.authority.as_ref()?;

	Some(LaneControlAuthority {
		reason: non_empty_string(authority.reason.as_deref())?,
		source: non_empty_string(authority.source.as_deref())?,
		inspected_run_id: non_empty_string(authority.inspected_run_id.as_deref())?,
		expected_turn_id: non_empty_string(authority.expected_turn_id.as_deref()),
		allow_hard_fallback: authority.allow_hard_fallback.unwrap_or(false),
	})
}

fn lane_control_mutating_preconditions(report: &Value) -> Vec<Value> {
	report
		.get("runs")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.map(|run| {
			serde_json::json!({
				"projectId": run.get("projectId").cloned().unwrap_or(Value::Null),
				"issueId": run.get("issueId").cloned().unwrap_or(Value::Null),
				"issueIdentifier": run.get("issueIdentifier").cloned().unwrap_or(Value::Null),
				"runId": run.get("runId").cloned().unwrap_or(Value::Null),
				"attemptNumber": run.get("attemptNumber").cloned().unwrap_or(Value::Null),
				"currentTurnId": run.get("turnId").cloned().unwrap_or(Value::Null),
				"laneControlNextAction": run
					.get("laneControlNextAction")
					.cloned()
					.unwrap_or(Value::Null),
				"softInterruptAvailable": run
					.get("softInterruptAvailable")
					.cloned()
					.unwrap_or(Value::Null),
				"hardInterruptAvailable": run
					.get("hardInterruptAvailable")
					.cloned()
					.unwrap_or(Value::Null),
				"hardInterruptRequiresForce": run
					.get("hardInterruptRequiresForce")
					.cloned()
					.unwrap_or(Value::Bool(true)),
				"authority": {
					"inspectedRunId": run.get("runId").cloned().unwrap_or(Value::Null),
					"expectedTurnId": run.get("turnId").cloned().unwrap_or(Value::Null)
				}
			})
		})
		.collect()
}

fn lane_control_refusal_result(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	reason: &str,
	message: impl Into<String>,
) -> Value {
	tool_refusal_value(lane_control_result_value(
		params,
		profile,
		"refused",
		reason,
		message,
		serde_json::json!({}),
	))
}

fn lane_control_interrupt_result(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	report: Value,
) -> Value {
	let soft = report.get("softInterrupt").unwrap_or(&Value::Null);
	let hard = report.get("hardInterrupt").unwrap_or(&Value::Null);
	let status =
		if hard.is_object() && hard.get("status").and_then(Value::as_str) != Some("unavailable") {
			"ok"
		} else {
			match soft.get("status").and_then(Value::as_str) {
				Some("delivered") => "ok",
				Some("pending") => "queued",
				_ => "refused",
			}
		};
	let reason =
		report.get("classification").and_then(Value::as_str).unwrap_or("lane_interrupt_result");
	let result = serde_json::json!({
		"projectId": report.get("projectId").cloned().unwrap_or(Value::Null),
		"issue": report.get("issue").cloned().unwrap_or(Value::Null),
		"issueId": report.get("issueId").cloned().unwrap_or(Value::Null),
		"issueIdentifier": report.get("issueIdentifier").cloned().unwrap_or(Value::Null),
		"runId": report.get("runId").cloned().unwrap_or(Value::Null),
		"attemptNumber": report.get("attemptNumber").cloned().unwrap_or(Value::Null),
		"force": report.get("force").cloned().unwrap_or(Value::Bool(false)),
		"classification": report.get("classification").cloned().unwrap_or(Value::Null),
		"softInterrupt": {
			"attempted": soft.get("attempted").cloned().unwrap_or(Value::Bool(false)),
			"available": soft.get("available").cloned().unwrap_or(Value::Bool(false)),
			"status": soft.get("status").cloned().unwrap_or(Value::Null),
			"classification": soft.get("classification").cloned().unwrap_or(Value::Null),
			"method": soft.get("method").cloned().unwrap_or(Value::Null),
			"requestId": soft.get("requestId").cloned().unwrap_or(Value::Null),
			"message": soft.get("message").cloned().unwrap_or(Value::Null),
			"errorClass": soft.get("errorClass").cloned().unwrap_or(Value::Null)
		},
		"hardInterrupt": if hard.is_object() {
			serde_json::json!({
				"attempted": hard.get("attempted").cloned().unwrap_or(Value::Bool(false)),
				"status": hard.get("status").cloned().unwrap_or(Value::Null),
				"classification": hard.get("classification").cloned().unwrap_or(Value::Null),
				"signals": hard.get("signals").cloned().unwrap_or_else(|| serde_json::json!([])),
				"message": hard.get("message").cloned().unwrap_or(Value::Null),
				"errorClass": hard.get("errorClass").cloned().unwrap_or(Value::Null)
			})
		} else {
			Value::Null
		},
		"nextAction": report.get("nextAction").cloned().unwrap_or(Value::Null)
	});
	let value = lane_control_result_value(
		params,
		profile,
		status,
		reason,
		"Lane interrupt completed through the existing lane-control guard path.",
		result,
	);

	if status == "refused" { tool_refusal_value(value) } else { tool_success(value) }
}

fn lane_control_steer_result(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	report: Value,
) -> Value {
	let outcome = report.get("outcome").and_then(Value::as_str).unwrap_or("unknown");
	let delivery_status = report.get("deliveryStatus").and_then(Value::as_str).unwrap_or("unknown");
	let failure_class = report.get("failureClass").and_then(Value::as_str);
	let status = if delivery_status == "queued" {
		"queued"
	} else if matches!(outcome, "rejected" | "failed" | "timed_out" | "fallback") {
		"refused"
	} else {
		"ok"
	};
	let reason = failure_class
		.or_else(|| report.get("reason").and_then(Value::as_str))
		.unwrap_or("lane_steer_result");
	let result = serde_json::json!({
		"projectId": report.get("projectId").cloned().unwrap_or(Value::Null),
		"issueId": report.get("issueId").cloned().unwrap_or(Value::Null),
		"issueIdentifier": report.get("issueIdentifier").cloned().unwrap_or(Value::Null),
		"runId": report.get("runId").cloned().unwrap_or(Value::Null),
		"attemptNumber": report.get("attemptNumber").cloned().unwrap_or(Value::Null),
		"expectedTurnId": report.get("expectedTurnId").cloned().unwrap_or(Value::Null),
		"currentTurnId": report.get("currentTurnId").cloned().unwrap_or(Value::Null),
		"responseTurnId": report.get("responseTurnId").cloned().unwrap_or(Value::Null),
		"auditRecordId": report.get("auditRecordId").cloned().unwrap_or(Value::Null),
		"requestId": report.get("requestId").cloned().unwrap_or(Value::Null),
		"outcome": report.get("outcome").cloned().unwrap_or(Value::Null),
		"reason": report.get("reason").cloned().unwrap_or(Value::Null),
		"failureClass": report.get("failureClass").cloned().unwrap_or(Value::Null),
		"deliveryStatus": report.get("deliveryStatus").cloned().unwrap_or(Value::Null),
		"messageByteCount": report.get("messageByteCount").cloned().unwrap_or(Value::Null),
		"messageLineCount": report.get("messageLineCount").cloned().unwrap_or(Value::Null)
	});
	let value = lane_control_result_value(
		params,
		profile,
		status,
		reason,
		"Lane steer returned without exposing the original steer message.",
		result,
	);

	if status == "refused" { tool_refusal_value(value) } else { tool_success(value) }
}

fn lane_control_result_value(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	status: &str,
	reason: &str,
	message: impl Into<String>,
	result: Value,
) -> Value {
	let mut value = serde_json::json!({
		"schema": "decodex.mcp.lane_control_result/1",
		"status": status,
		"reason": reason,
		"message": message.into(),
		"capability_profile": profile.as_str(),
		"action": params.action.as_str(),
		"project_id": params.project_id.as_deref(),
		"issue": params.issue.as_deref(),
		"run_id": params.run_id.as_deref(),
		"preconditions": lane_control_preconditions(params),
		"result": result
	});

	sanitize_mcp_observability_value(&mut value);

	value
}

fn project_control_authority(
	params: &ProjectControlToolArgs,
) -> Option<ProjectControlAuthority<'_>> {
	let authority = params.authority.as_ref()?;

	Some(ProjectControlAuthority {
		reason: non_empty_string(authority.reason.as_deref())?,
		source: non_empty_string(authority.source.as_deref())?,
		acknowledge_future_dispatch_only: authority
			.acknowledge_future_dispatch_only
			.unwrap_or(false),
	})
}

fn project_control_status_result(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	project_id: &str,
) -> Value {
	let state_store = match runtime::open_runtime_store_lazy() {
		Ok(state_store) => state_store,
		Err(error) => {
			return project_control_refusal_result(
				params,
				profile,
				"project_control_unavailable",
				format!("Project status failed closed: {error}"),
			);
		},
	};
	let projects = match state_store.list_projects() {
		Ok(projects) => projects,
		Err(error) => {
			return project_control_refusal_result(
				params,
				profile,
				"project_registry_unavailable",
				format!("Project registry read failed closed: {error}"),
			);
		},
	};
	let Some(project) = projects.iter().find(|project| project.service_id() == project_id) else {
		return project_control_refusal_result(
			params,
			profile,
			"project_not_registered",
			"Project control requires a registered Decodex project.",
		);
	};

	project_control_success_result(
		params,
		profile,
		project_id,
		serde_json::json!({
			"enabled": project.enabled(),
			"future_dispatch_only": true,
			"active_lanes_killed": false,
			"next_action": if project.enabled() {
				"Project is enabled for future dispatch."
			} else {
				"Project is paused for future dispatch. Existing lanes remain visible."
			}
		}),
	)
}

fn project_control_success_result(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	project_id: &str,
	result: Value,
) -> Value {
	tool_success(project_control_result_value(
		params,
		profile,
		project_id,
		"ok",
		params.action.as_str(),
		"Project control completed through the registered project enablement guard.",
		result,
	))
}

fn project_control_refusal_result(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	reason: &str,
	message: impl Into<String>,
) -> Value {
	let project_id = params.project_id.as_deref().unwrap_or("");

	tool_refusal_value(project_control_result_value(
		params,
		profile,
		project_id,
		"refused",
		reason,
		message,
		serde_json::json!({}),
	))
}

fn project_control_result_value(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	project_id: &str,
	status: &str,
	reason: &str,
	message: impl Into<String>,
	result: Value,
) -> Value {
	let mut value = serde_json::json!({
		"schema": "decodex.mcp.project_control_result/1",
		"status": status,
		"reason": reason,
		"message": message.into(),
		"capability_profile": profile.as_str(),
		"action": params.action.as_str(),
		"project_id": non_empty_string(Some(project_id)),
		"future_dispatch_only": true,
		"result": result
	});

	sanitize_mcp_observability_value(&mut value);

	value
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

fn mcp_now_rfc3339() -> String {
	OffsetDateTime::now_utc()
		.format(&Rfc3339)
		.unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
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
	authorization: McpHttpAuthorization,
) -> crate::prelude::Result<()> {
	let mut handler = McpHttpHandler {
		server: McpServer { context, capability_profile, transport: McpTransport::StreamableHttp },
		sessions: McpHttpSessions::default(),
		allowed_origins,
		listen_address: listener.local_addr().map(|address| address.to_string()).ok(),
		authorization,
	};

	for stream in listener.incoming() {
		match stream {
			Ok(mut stream) => {
				if let Err(error) = handle_mcp_http_stream(&mut stream, &mut handler) {
					tracing::warn!(?error, "Decodex MCP Streamable HTTP request failed.");
				}
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
	authorization: &McpHttpAuthorization,
) -> crate::prelude::Result<()> {
	if listen_address_host_is_loopback(address) {
		return Ok(());
	}
	if allowed_origins.is_empty() {
		eyre::bail!(
			"Refusing to bind Decodex MCP Streamable HTTP to `{address}` without --allow-origin; use the loopback default or set explicit trusted origins."
		)
	}
	if !authorization.is_required() {
		eyre::bail!(
			"Refusing to bind Decodex MCP Streamable HTTP to `{address}` without --bearer-token-env; direct non-loopback listeners require bearer authorization."
		)
	}

	Ok(())
}

fn validate_mcp_http_capability_profile(
	capability_profile: McpCapabilityProfile,
	authorization: &McpHttpAuthorization,
) -> crate::prelude::Result<()> {
	if capability_profile == McpCapabilityProfile::Observe || authorization.is_required() {
		return Ok(());
	}

	eyre::bail!(
		"Refusing to expose Decodex MCP Streamable HTTP profile `{}` without --bearer-token-env; elevated HTTP profiles require bearer authorization.",
		capability_profile.as_str()
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

fn mcp_sanitized_value(mut value: Value) -> Value {
	sanitize_mcp_observability_value(&mut value);

	value
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
max_concurrent_agents = 0
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
