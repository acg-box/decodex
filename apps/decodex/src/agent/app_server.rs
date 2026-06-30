mod activity;
mod protocol;
mod schema_probe;
mod turn_failure;

pub(crate) use turn_failure::AppServerTurnFailure;

pub(crate) use self::activity::protocol_activity_idle_timeout;
use self::activity::{ChildActivityAccumulator, ProtocolActivityAccumulator, redact_identifier};
use self::schema_probe::probe_app_server_schema;
#[cfg(test)]
use self::schema_probe::{
	APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS, APP_SERVER_REQUIRED_CLIENT_REQUESTS,
	APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS, APP_SERVER_REQUIRED_SERVER_REQUESTS,
	APP_SERVER_SCHEMA_REQUIRED_MARKERS, validate_generated_app_server_schema,
};

use std::{
	collections::BTreeMap,
	env,
	error::Error,
	fmt::{self, Display, Formatter},
	fs, mem,
	path::{Path, PathBuf},
	time::{Duration, Instant},
};

use color_eyre::Report;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use self::protocol::{
	AgentMessageDeltaNotification, AppServerClient, ChatgptAuthTokensRefreshParams,
	ChatgptAuthTokensRefreshResponse, CommandExecParams, CommandExecResponse,
	CommandExecutionApprovalDecision, CommandExecutionRequestApprovalResponse, ConfigReadParams,
	DynamicToolCallParams, EffectiveThreadConfig, ErrorNotification, FileChangeApprovalDecision,
	FileChangeRequestApprovalResponse, InitializeResponse, ItemCompletedNotification,
	ListMcpServerStatusParams, ListMcpServerStatusResponse, LoginAccountParams,
	McpServerElicitationAction, McpServerElicitationRequestResponse, McpServerStatusSummary,
	ModelListParams, ModelListResponse, ModelProviderCapabilitiesReadResponse, ModelSummary,
	PermissionGrantScope, PermissionsRequestApprovalResponse, PluginListParams, PluginListResponse,
	ProbeDynamicToolHandler, RunOutcome, RuntimeConfigSummary, SkillsListParams,
	SkillsListResponse, ThreadArchiveRequest, ThreadGoal, ThreadGoalClearParams,
	ThreadGoalGetParams, ThreadGoalSetParams, ThreadGoalStatus, ThreadGoalUpdatedNotification,
	ThreadResumeRequest, ThreadSessionResponse, ThreadStartRequest,
	ThreadStatusChangedNotification, ToolRequestUserInputResponse, TurnCompletedNotification,
	TurnError, TurnInterruptRequest, TurnStartRequest, TurnSteerRequest, UserInput,
};
use crate::{
	agent::{
		app_server::protocol::LoginAccountResponse,
		codex_accounts::{CodexAccountAuthFailure, CodexAccountLogin, CodexAccountProvider},
		json_rpc,
		json_rpc::{
			AppServerHomePreflightFailure, AppServerOutputTimeout, AppServerProcessEnv,
			JsonRpcConnection, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
			ResolvedAppServerCodexHomeEnv, WireMessage,
		},
		tracker_tool_bridge::{
			self, DynamicToolCallResponse, DynamicToolContentItem, DynamicToolHandler,
			DynamicToolSpec, TurnCompletionStatus,
		},
	},
	prelude::eyre,
	run_control::{
		self, LaneControlInterruptRequest, LaneControlInterruptResponse, LaneControlSteerRequest,
		LaneControlSteerResponse, LaneControlSteerResponseStatus, PendingLaneControlRequest,
		PendingLaneControlSteerRequest,
	},
	state::{
		self, CodexAccountActivitySummary, CodexAccountMarker, EffectiveRuntimeMarker,
		RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED, RUN_CONTROL_CHANNEL_DIR,
		RUN_CONTROL_CHANNEL_STATUS_COMPLETED, RUN_CONTROL_CHANNEL_STATUS_FAILED,
		RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE, RUN_OPERATION_AGENT_RUN,
		RUN_OPERATION_APP_SERVER_PREFLIGHT, RunControlActionOutcomeRequest, RunControlChannel,
		StateStore,
	},
};

pub(crate) const RUN_LEASE_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const MODEL_EXECUTION_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const RUN_CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(500);
const MCP_PREFLIGHT_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PLUGIN_PREFLIGHT_MAX_ATTEMPTS: u32 = 2;
const PROBE_RUN_ID: &str = "protocol-probe-run";
const PROBE_ISSUE_ID: &str = "protocol-probe";
const PROBE_EXPECTED_OUTPUT: &str = "PROBE_OK";
const PROBE_COMMAND_EXEC_EXPECTED_OUTPUT: &str = "COMMAND_EXEC_OK";
const PROBE_COMMAND_EXEC_TIMEOUT_MS: u64 = 5_000;
const PROBE_COMMAND_EXEC_OUTPUT_BYTES_CAP: u64 = 1_024;
const PROBE_DEVELOPER_INSTRUCTIONS: &str = "You are a protocol probe. You must call the dynamic tool `echo_probe` exactly once with the JSON argument `{\"text\":\"PROBE_OK\"}`. Do not use shell. Do not inspect files. After the tool response is returned, reply with the exact text PROBE_OK and nothing else.";
const PROBE_USER_INPUT: &str = "Call `echo_probe` with `{\\\"text\\\":\\\"PROBE_OK\\\"}`. After the tool succeeds, reply with the exact text PROBE_OK.";
const PREFLIGHT_EVENT_TYPE: &str = "app-server/preflight";
const PREFLIGHT_MODEL_PAGE_LIMIT: u32 = 200;
const PREFLIGHT_MCP_PAGE_LIMIT: u32 = 50;
const PREFLIGHT_MCP_DETAIL: &str = "toolsAndAuthOnly";
const PREFLIGHT_CHECK_CONFIG: &str = "config";
const PREFLIGHT_CHECK_MODEL: &str = "model";
const PREFLIGHT_CHECK_MODEL_PROVIDER: &str = "model_provider";
const PREFLIGHT_CHECK_SKILLS: &str = "skills";
const PREFLIGHT_CHECK_PLUGINS: &str = "plugins";
const PREFLIGHT_CHECK_MCP: &str = "mcp";
const PREFLIGHT_PLUGIN_MARKETPLACE_KIND: &str = "local";
const JSONRPC_METHOD_NOT_FOUND: i64 = -32_601;

pub(crate) trait TurnContinuationGuard {
	fn should_continue_turn(&self, turn_count: u32) -> crate::prelude::Result<bool>;
	fn validate_continuation_boundary(&self, _turn_count: u32) -> crate::prelude::Result<()> {
		Ok(())
	}
}

pub(crate) trait PhaseGoalController {
	fn initial_phase_goal(&self) -> crate::prelude::Result<Option<PhaseGoalSpec>>;
	fn phase_goal_completed(
		&self,
		phase: PhaseGoalKind,
	) -> crate::prelude::Result<PhaseGoalTransition>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhaseGoalKind {
	ImplementToValidationReady,
	RepairValidationFailures,
	RepairAcceptedReviewFindings,
	ReviewRepairEvidence,
	HandoffEvidence,
}
impl PhaseGoalKind {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::ImplementToValidationReady => "implement_to_validation_ready",
			Self::RepairValidationFailures => "repair_validation_failures",
			Self::RepairAcceptedReviewFindings => "repair_accepted_review_findings",
			Self::ReviewRepairEvidence => "review_repair_evidence",
			Self::HandoffEvidence => "handoff_evidence",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhaseGoalTransition {
	Continue(PhaseGoalSpec),
	CompleteRun,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppServerThreadArchiveOutcome {
	Archived,
	DiscardedMissingThread,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppServerPhaseGoalFailureKind {
	Unsupported { method: &'static str },
	MissingTerminalPath { phase: PhaseGoalKind },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppServerDynamicToolFailureKind {
	Protocol,
	Tool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AppServerCapabilityPreflightStatus {
	Ok,
	Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppServerCapabilityPreflightFailureKind {
	MethodFailed { method: &'static str, error: String, timed_out: bool },
	BlockedState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestWaitPhase {
	Initialize,
	AccountLogin,
	ThreadStart,
	ThreadResume,
	TurnStart,
	TurnExecution,
}
impl RequestWaitPhase {
	fn label(self) -> &'static str {
		match self {
			Self::Initialize => "initialize",
			Self::AccountLogin => "account/login/start",
			Self::ThreadStart => "thread/start",
			Self::ThreadResume => "thread/resume",
			Self::TurnStart => "turn/start",
			Self::TurnExecution => "turn execution",
		}
	}

	fn transport_failure_is_retryable_startup(self) -> bool {
		matches!(
			self,
			Self::Initialize | Self::AccountLogin | Self::ThreadStart | Self::ThreadResume
		)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhaseGoalSpec {
	pub(crate) phase: PhaseGoalKind,
	pub(crate) objective: String,
	pub(crate) token_budget: Option<i64>,
}
impl PhaseGoalSpec {
	pub(crate) fn new(
		phase: PhaseGoalKind,
		objective: impl Into<String>,
		token_budget: Option<i64>,
	) -> Self {
		Self { phase, objective: objective.into(), token_budget }
	}
}

#[derive(Debug)]
pub(crate) struct AppServerPhaseGoalFailure {
	kind: AppServerPhaseGoalFailureKind,
}
impl AppServerPhaseGoalFailure {
	fn unsupported(method: &'static str) -> Self {
		Self { kind: AppServerPhaseGoalFailureKind::Unsupported { method } }
	}

	#[cfg(test)]
	pub(crate) fn unsupported_for_test(method: &'static str) -> Self {
		Self::unsupported(method)
	}

	fn missing_terminal_path(phase: PhaseGoalKind) -> Self {
		Self { kind: AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } }
	}

	#[cfg(test)]
	pub(crate) fn missing_terminal_path_for_test(phase: PhaseGoalKind) -> Self {
		Self::missing_terminal_path(phase)
	}

	pub(crate) fn is_terminal_path_missing(&self) -> bool {
		matches!(self.kind, AppServerPhaseGoalFailureKind::MissingTerminalPath { .. })
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { .. } =>
				"app_server_phase_goal_unsupported",
			AppServerPhaseGoalFailureKind::MissingTerminalPath { .. } =>
				"phase_goal_terminal_path_missing",
		}
	}

	pub(crate) fn retry_next_action(&self) -> String {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { method } => format!(
				"select or upgrade to a Codex app-server that supports required phase-goal method `{method}`"
			),
			AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } => format!(
				"decodex will retry `{}` terminal-path recovery automatically; the next attempt must run the required review, handoff, closeout, or manual-attention terminal tool instead of treating phase-goal completion as issue completion",
				phase.as_str()
			),
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { method } => format!(
				"select or upgrade to a Codex app-server that supports required phase-goal method `{method}`, confirm with `decodex probe stdio://`, restart `decodex serve`, {recovery_gate}"
			),
			AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } => format!(
				"inspect the retained lane after phase goal `{}` completed without a terminal Decodex path, finish validation/review/handoff or route manual attention, {recovery_gate}",
				phase.as_str()
			),
		}
	}
}

impl Display for AppServerPhaseGoalFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		match self.kind {
			AppServerPhaseGoalFailureKind::Unsupported { method } => {
				write!(
					formatter,
					"Unsupported Codex app-server: required phase-goal method `{method}` is unavailable."
				)
			},
			AppServerPhaseGoalFailureKind::MissingTerminalPath { phase } => write!(
				formatter,
				"Phase goal `{}` completed without a Decodex terminal completion path.",
				phase.as_str()
			),
		}
	}
}

impl Error for AppServerPhaseGoalFailure {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AppServerCapabilityPreflightReport {
	checks: Vec<AppServerCapabilityPreflightCheck>,
}
impl AppServerCapabilityPreflightReport {
	pub(crate) fn new() -> Self {
		Self { checks: Vec::new() }
	}

	#[cfg(test)]
	fn checks(&self) -> &[AppServerCapabilityPreflightCheck] {
		&self.checks
	}

	pub(crate) fn check_count(&self) -> usize {
		self.checks.len()
	}

	fn push_ok(
		&mut self,
		name: &'static str,
		summary: impl Into<String>,
		details: BTreeMap<String, String>,
	) {
		self.checks.push(AppServerCapabilityPreflightCheck {
			name,
			status: AppServerCapabilityPreflightStatus::Ok,
			summary: summary.into(),
			details,
		});
	}

	fn push_blocked(
		&mut self,
		name: &'static str,
		summary: impl Into<String>,
		details: BTreeMap<String, String>,
	) {
		self.checks.push(AppServerCapabilityPreflightCheck {
			name,
			status: AppServerCapabilityPreflightStatus::Blocked,
			summary: summary.into(),
			details,
		});
	}

	fn has_blockers(&self) -> bool {
		self.checks.iter().any(|check| check.status == AppServerCapabilityPreflightStatus::Blocked)
	}

	fn blocker_summary(&self) -> String {
		let blockers = self
			.checks
			.iter()
			.filter(|check| check.status == AppServerCapabilityPreflightStatus::Blocked)
			.map(preflight_check_blocker_summary)
			.collect::<Vec<_>>();

		if blockers.is_empty() { String::from("no blockers recorded") } else { blockers.join("; ") }
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerCapabilityPreflightFailure {
	kind: AppServerCapabilityPreflightFailureKind,
	report: AppServerCapabilityPreflightReport,
}
impl AppServerCapabilityPreflightFailure {
	fn blocked(report: AppServerCapabilityPreflightReport) -> Self {
		Self { kind: AppServerCapabilityPreflightFailureKind::BlockedState, report }
	}

	fn method_failed(
		method: &'static str,
		error: String,
		report: AppServerCapabilityPreflightReport,
	) -> Self {
		Self {
			kind: AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				error,
				timed_out: false,
			},
			report,
		}
	}

	fn method_timed_out(
		method: &'static str,
		error: String,
		report: AppServerCapabilityPreflightReport,
	) -> Self {
		Self {
			kind: AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				error,
				timed_out: true,
			},
			report,
		}
	}

	#[cfg(test)]
	pub(crate) fn blocked_for_test(check: &'static str, summary: &str) -> Self {
		let mut report = AppServerCapabilityPreflightReport::new();

		report.push_blocked(check, summary, BTreeMap::new());

		Self::blocked(report)
	}

	#[cfg(test)]
	pub(crate) fn blocked_for_test_with_details(
		check: &'static str,
		summary: &str,
		details: BTreeMap<String, String>,
	) -> Self {
		let mut report = AppServerCapabilityPreflightReport::new();

		report.push_blocked(check, summary, details);

		Self::blocked(report)
	}

	#[cfg(test)]
	pub(crate) fn method_timed_out_for_test(method: &'static str, error: String) -> Self {
		let mut report = AppServerCapabilityPreflightReport::new();

		report.push_blocked(
			check_name_for_method(method),
			format!("`{method}` timed out."),
			BTreeMap::new(),
		);

		Self::method_timed_out(method, error, report)
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method: "plugin/list",
				timed_out: true,
				..
			} => "app_server_plugin_list_timeout",
			AppServerCapabilityPreflightFailureKind::MethodFailed { timed_out: true, .. } =>
				"app_server_preflight_timeout",
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. } =>
				"app_server_introspection_method_failed",
			AppServerCapabilityPreflightFailureKind::BlockedState =>
				"app_server_runtime_preflight_failed",
		}
	}

	pub(crate) fn is_retryable_timeout(&self) -> bool {
		matches!(
			self.kind,
			AppServerCapabilityPreflightFailureKind::MethodFailed { timed_out: true, .. }
		)
	}

	pub(crate) fn retry_next_action(&self) -> String {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method: "plugin/list",
				timed_out: true,
				..
			} => String::from(
				"decodex will retry app-server preflight automatically; inspect local app_server_preflight_failed evidence for the `plugin/list` timeout and restart `decodex serve` if the retry budget exhausts",
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				timed_out: true,
				..
			} => format!(
				"decodex will retry app-server preflight automatically; inspect local app_server_preflight_failed evidence for the `{method}` timeout and restart `decodex serve` if the retry budget exhausts"
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. }
			| AppServerCapabilityPreflightFailureKind::BlockedState =>
				String::from("app-server preflight requires operator recovery"),
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method: "plugin/list",
				timed_out: true,
				..
			} => format!(
				"inspect local app_server_preflight_failed evidence for the `plugin/list` timeout, restart `decodex serve` if the app-server is stale, run `decodex probe` to confirm plugin inventory recovers, {recovery_gate}"
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				timed_out: true,
				..
			} => format!(
				"inspect local app_server_preflight_failed evidence for the `{method}` timeout, restart `decodex serve` if the app-server is stale, run `decodex probe` to confirm app-server preflight recovers, {recovery_gate}"
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. } => format!(
				"inspect the Codex app-server preflight status, repair the local Codex runtime configuration, restart `decodex serve`, {recovery_gate}"
			),
			AppServerCapabilityPreflightFailureKind::BlockedState => {
				let blocker_summary = self.blocker_summary();

				format!(
					"inspect local app_server_preflight_failed evidence for `{blocker_summary}`, repair the local Codex runtime configuration, restart `decodex serve`, {recovery_gate}"
				)
			},
		}
	}

	fn blocker_summary(&self) -> String {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				error,
				timed_out: true,
			} => format!(
				"{}: `{method}` timed out during preflight: {error}",
				check_name_for_method(method)
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { method, error, .. } => {
				format!("{}: `{method}` returned {error}", check_name_for_method(method))
			},
			AppServerCapabilityPreflightFailureKind::BlockedState => self.report.blocker_summary(),
		}
	}

	#[cfg(test)]
	fn report(&self) -> &AppServerCapabilityPreflightReport {
		&self.report
	}
}

impl Display for AppServerCapabilityPreflightFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		write!(formatter, "app_server_preflight_failed: {}", self.blocker_summary())
	}
}

impl Error for AppServerCapabilityPreflightFailure {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerDynamicToolFailure {
	kind: AppServerDynamicToolFailureKind,
	tool: Option<String>,
	message: String,
}
impl AppServerDynamicToolFailure {
	fn protocol(tool: Option<String>, message: impl Into<String>) -> Self {
		Self { kind: AppServerDynamicToolFailureKind::Protocol, tool, message: message.into() }
	}

	fn tool(tool: Option<String>, message: impl Into<String>) -> Self {
		Self { kind: AppServerDynamicToolFailureKind::Tool, tool, message: message.into() }
	}

	#[cfg(test)]
	pub(crate) fn protocol_for_test(tool: Option<String>, message: impl Into<String>) -> Self {
		Self::protocol(tool, message)
	}

	#[cfg(test)]
	pub(crate) fn tool_for_test(tool: Option<String>, message: impl Into<String>) -> Self {
		Self::tool(tool, message)
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match self.kind {
			AppServerDynamicToolFailureKind::Protocol => "app_server_dynamic_tool_protocol_failure",
			AppServerDynamicToolFailureKind::Tool => "app_server_dynamic_tool_failed",
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.kind {
			AppServerDynamicToolFailureKind::Protocol => format!(
				"inspect the app-server dynamic tool declaration and `item/tool/call` payload, repair the protocol mismatch manually, {recovery_gate}"
			),
			AppServerDynamicToolFailureKind::Tool => format!(
				"inspect the dynamic tool response and lane state, correct the tool call or underlying service state manually, {recovery_gate}"
			),
		}
	}

	pub(crate) fn retry_next_action(&self) -> String {
		format!("decodex will retry automatically; {}", self.diagnostic_next_action())
	}

	fn diagnostic_next_action(&self) -> &'static str {
		match self.kind {
			AppServerDynamicToolFailureKind::Protocol =>
				"inspect the declared dynamic tool surface and item/tool/call payload before retrying the lane",
			AppServerDynamicToolFailureKind::Tool =>
				"inspect the tool response, correct the call arguments or backing state, and retry the tool call",
		}
	}
}

impl Display for AppServerDynamicToolFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		write!(formatter, "app_server_dynamic_tool_failure: {}", self.message)?;

		if let Some(tool) = self.tool.as_deref() {
			write!(formatter, " (tool `{tool}`)")?;
		}

		Ok(())
	}
}

impl Error for AppServerDynamicToolFailure {}

pub(crate) struct AppServerThreadArchiveRequest<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) listen: &'a str,
	pub(crate) process_env: &'a AppServerProcessEnv,
	pub(crate) thread_id: &'a str,
	pub(crate) sequence_number: i64,
}

#[derive(Clone)]
pub(crate) struct AppServerRunRequest<'a> {
	pub(crate) project_id: String,
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) listen: String,
	pub(crate) cwd: String,
	pub(crate) developer_instructions: String,
	pub(crate) user_input: String,
	pub(crate) max_turns: u32,
	pub(crate) timeout: Duration,
	pub(crate) process_env: AppServerProcessEnv,
	pub(crate) continuation_user_input: Option<String>,
	pub(crate) activity_marker_path: Option<PathBuf>,
	pub(crate) resume_thread_id: Option<String>,
	pub(crate) ephemeral_thread: bool,
	pub(crate) command_exec_health_check: Option<CommandExecHealthCheck>,
	pub(crate) dynamic_tool_handler: Option<&'a dyn DynamicToolHandler>,
	pub(crate) continuation_guard: Option<&'a dyn TurnContinuationGuard>,
	pub(crate) phase_goal_controller: Option<&'a dyn PhaseGoalController>,
	pub(crate) codex_account_provider: Option<&'a dyn CodexAccountProvider>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandExecHealthCheck {
	pub(crate) command: Vec<String>,
	pub(crate) expected_stdout: String,
	pub(crate) timeout_ms: u64,
	pub(crate) output_bytes_cap: u64,
}
impl CommandExecHealthCheck {
	fn probe() -> Self {
		Self {
			command: vec![
				String::from("/bin/sh"),
				String::from("-c"),
				format!("printf {PROBE_COMMAND_EXEC_EXPECTED_OUTPUT}"),
			],
			expected_stdout: String::from(PROBE_COMMAND_EXEC_EXPECTED_OUTPUT),
			timeout_ms: PROBE_COMMAND_EXEC_TIMEOUT_MS,
			output_bytes_cap: PROBE_COMMAND_EXEC_OUTPUT_BYTES_CAP,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerRunResult {
	pub(crate) user_agent: String,
	pub(crate) capability_preflight: AppServerCapabilityPreflightReport,
	pub(crate) thread_id: String,
	pub(crate) turn_id: String,
	pub(crate) turn_count: u32,
	pub(crate) event_count: i64,
	pub(crate) final_output: String,
	pub(crate) continuation_pending: bool,
	pub(crate) phase_goal_status: Option<PhaseGoalRunStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PhaseGoalRunStatus {
	pub(crate) phase: PhaseGoalKind,
	pub(crate) status: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AppServerCapabilityPreflightCheck {
	name: &'static str,
	status: AppServerCapabilityPreflightStatus,
	summary: String,
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	details: BTreeMap<String, String>,
}

struct RunRecorder<'a> {
	state_store: &'a StateStore,
	project_id: &'a str,
	issue_id: &'a str,
	run_id: &'a str,
	attempt_number: i64,
	activity_marker_path: Option<&'a PathBuf>,
	thread_id: Option<String>,
	turn_id: Option<String>,
	next_sequence: i64,
	child_activity: ChildActivityAccumulator,
	protocol_activity: ProtocolActivityAccumulator,
}
impl<'a> RunRecorder<'a> {
	#[cfg(test)]
	fn new(
		state_store: &'a StateStore,
		run_id: &'a str,
		attempt_number: i64,
		activity_marker_path: Option<&'a PathBuf>,
	) -> Self {
		Self::new_with_context(
			state_store,
			"unknown",
			"unknown",
			run_id,
			attempt_number,
			activity_marker_path,
		)
	}

	fn new_with_context(
		state_store: &'a StateStore,
		project_id: &'a str,
		issue_id: &'a str,
		run_id: &'a str,
		attempt_number: i64,
		activity_marker_path: Option<&'a PathBuf>,
	) -> Self {
		Self {
			state_store,
			project_id,
			issue_id,
			run_id,
			attempt_number,
			activity_marker_path,
			thread_id: None,
			turn_id: None,
			next_sequence: 1,
			child_activity: ChildActivityAccumulator::new(),
			protocol_activity: ProtocolActivityAccumulator::new(),
		}
	}

	fn project_id(&self) -> &str {
		self.project_id
	}

	fn issue_id(&self) -> &str {
		self.issue_id
	}

	fn mark_activity(&self) -> crate::prelude::Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			write_activity_marker_best_effort(marker_path, self.run_id, self.attempt_number);
		};

		Ok(())
	}

	fn set_thread_id(&mut self, thread_id: &str) -> crate::prelude::Result<()> {
		self.thread_id = Some(thread_id.to_owned());

		if let Some(marker_path) = self.activity_marker_path {
			write_thread_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				thread_id,
			);
		}

		Ok(())
	}

	fn set_turn_id(&mut self, turn_id: &str) -> crate::prelude::Result<()> {
		self.turn_id = Some(turn_id.to_owned());

		if let Some(marker_path) = self.activity_marker_path {
			write_turn_marker_best_effort(marker_path, self.run_id, self.attempt_number, turn_id);
		}

		Ok(())
	}

	fn set_thread_status(
		&mut self,
		status: &str,
		active_flags: &[String],
	) -> crate::prelude::Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			write_thread_status_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				self.thread_id.as_deref(),
				self.turn_id.as_deref(),
				status,
				active_flags,
			);
		}

		Ok(())
	}

	fn set_effective_runtime(
		&mut self,
		runtime: &EffectiveThreadConfig,
	) -> crate::prelude::Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			write_effective_runtime_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				self.thread_id.as_deref(),
				self.turn_id.as_deref(),
				runtime,
			);
		}

		Ok(())
	}

	fn set_codex_account(
		&mut self,
		summary: &CodexAccountActivitySummary,
		account_summaries: &[CodexAccountActivitySummary],
	) -> crate::prelude::Result<()> {
		if let Some(marker_path) = self.activity_marker_path {
			write_codex_account_marker_best_effort(
				marker_path,
				self.run_id,
				self.attempt_number,
				summary,
				account_summaries,
			);
		}

		Ok(())
	}

	fn record(&mut self, event_type: &str, payload: &str) -> crate::prelude::Result<()> {
		self.state_store.append_event(self.run_id, self.next_sequence, event_type, payload)?;

		let child_activity = self.child_activity.record(event_type, payload);
		let protocol_activity = self.protocol_activity.record(event_type, payload, &child_activity);

		self.state_store.record_run_activity_summary(
			self.run_id,
			self.attempt_number,
			Some(&child_activity),
			Some(&protocol_activity),
		)?;

		if let Some(marker_path) = self.activity_marker_path {
			let activity = state::ProtocolActivityMarker {
				run_id: self.run_id,
				attempt_number: self.attempt_number,
				thread_id: self.thread_id.as_deref(),
				turn_id: self.turn_id.as_deref(),
				event_count: self.next_sequence,
				last_event_type: event_type,
				child_agent_activity: Some(&child_activity),
				protocol_activity: Some(&protocol_activity),
			};

			write_protocol_activity_marker_best_effort(marker_path, &activity);
		}

		self.next_sequence += 1;

		Ok(())
	}
}

struct TurnLoopResult {
	turn_id: String,
	turn_count: u32,
	final_output: String,
	continuation_pending: bool,
	phase_goal_status: Option<PhaseGoalRunStatus>,
}

struct PhaseGoalRuntime<'a> {
	controller: &'a dyn PhaseGoalController,
	active_goal: PhaseGoalSpec,
}

#[derive(Clone, Copy)]
struct RequestDispatchContext<'a> {
	phase: RequestWaitPhase,
	dynamic_tool_handler: Option<&'a dyn DynamicToolHandler>,
	codex_account_provider: Option<&'a dyn CodexAccountProvider>,
	target_thread_id: Option<&'a str>,
	target_turn_id: Option<&'a str>,
}
impl<'a> RequestDispatchContext<'a> {
	fn new(
		phase: RequestWaitPhase,
		dynamic_tool_handler: Option<&'a dyn DynamicToolHandler>,
		codex_account_provider: Option<&'a dyn CodexAccountProvider>,
		target_thread_id: Option<&'a str>,
		target_turn_id: Option<&'a str>,
	) -> Self {
		Self {
			phase,
			dynamic_tool_handler,
			codex_account_provider,
			target_thread_id,
			target_turn_id,
		}
	}
}

#[derive(Debug)]
struct DynamicToolCallDispatch {
	response: DynamicToolCallResponse,
	diagnostic: Option<DynamicToolFailureDiagnostic>,
	terminal_failure: Option<AppServerDynamicToolFailure>,
}
impl DynamicToolCallDispatch {
	fn success(response: DynamicToolCallResponse) -> Self {
		Self { response, diagnostic: None, terminal_failure: None }
	}

	fn tool_failure(
		response: DynamicToolCallResponse,
		tool: Option<String>,
		namespace: Option<String>,
	) -> Self {
		let message = dynamic_tool_response_text(&response);
		let failure = AppServerDynamicToolFailure::tool(tool.clone(), message.clone());

		Self {
			response,
			diagnostic: Some(DynamicToolFailureDiagnostic::from_failure(&failure, namespace)),
			terminal_failure: None,
		}
	}

	fn protocol_failure(tool: Option<String>, namespace: Option<String>, message: String) -> Self {
		let failure = AppServerDynamicToolFailure::protocol(tool, message.clone());

		Self {
			response: DynamicToolCallResponse::failure(message),
			diagnostic: Some(DynamicToolFailureDiagnostic::from_failure(&failure, namespace)),
			terminal_failure: Some(failure),
		}
	}
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DynamicToolFailureDiagnostic {
	failure_class: &'static str,
	tool: Option<String>,
	namespace: Option<String>,
	message: String,
	next_action: &'static str,
}
impl DynamicToolFailureDiagnostic {
	fn from_failure(failure: &AppServerDynamicToolFailure, namespace: Option<String>) -> Self {
		Self {
			failure_class: failure.error_class(),
			tool: failure.tool.clone(),
			namespace,
			message: failure.message.clone(),
			next_action: failure.diagnostic_next_action(),
		}
	}
}


pub(crate) fn execute_app_server_run(
	request: &AppServerRunRequest<'_>,
	state_store: &StateStore,
) -> crate::prelude::Result<AppServerRunResult> {
	state_store.record_run_attempt(
		&request.run_id,
		&request.issue_id,
		request.attempt_number,
		"starting",
	)?;

	if let Some(marker_path) = request.activity_marker_path.as_ref() {
		write_activity_marker_best_effort(marker_path, &request.run_id, request.attempt_number);
	}

	let control_channel = publish_run_control_channel_for_request(request, state_store)?;
	let result = execute_app_server_run_inner(request, state_store);

	match &result {
		Ok(_result) =>
			if control_channel.is_some() {
				state_store.retire_run_control_channel_for_attempt(
					&request.run_id,
					request.attempt_number,
					RUN_CONTROL_CHANNEL_STATUS_COMPLETED,
				)?;
			},
		Err(_error) => {
			state_store.record_run_attempt(
				&request.run_id,
				&request.issue_id,
				request.attempt_number,
				"failed",
			)?;

			if control_channel.is_some() {
				state_store.retire_run_control_channel_for_attempt(
					&request.run_id,
					request.attempt_number,
					RUN_CONTROL_CHANNEL_STATUS_FAILED,
				)?;
			}

			if let Some(marker_path) = request.activity_marker_path.as_ref() {
				write_activity_marker_best_effort(
					marker_path,
					&request.run_id,
					request.attempt_number,
				);
			}
		},
	}

	result
}

pub(crate) fn archive_app_server_thread_after_success(
	request: &AppServerThreadArchiveRequest<'_>,
	state_store: &StateStore,
) -> crate::prelude::Result<AppServerThreadArchiveOutcome> {
	let result = match archive_app_server_thread_after_success_inner(request) {
		Ok(()) => Ok(AppServerThreadArchiveOutcome::Archived),
		Err(error) if thread_archive_error_allows_discard(&error) =>
			Ok(AppServerThreadArchiveOutcome::DiscardedMissingThread),
		Err(error) => Err(error),
	};

	record_thread_archive_result_best_effort(state_store, request, result.as_ref());

	result
}

pub(crate) fn probe_app_server(listen: &str) -> crate::prelude::Result<AppServerRunResult> {
	let state_store = StateStore::open_in_memory()?;
	let probe_tool_handler = ProbeDynamicToolHandler;

	probe_app_server_schema(&AppServerProcessEnv::default())?;

	let result = execute_app_server_run(
		&AppServerRunRequest {
			project_id: String::from("probe"),
			run_id: PROBE_RUN_ID.to_owned(),
			issue_id: PROBE_ISSUE_ID.to_owned(),
			attempt_number: 1,
			listen: listen.to_owned(),
			cwd: env::current_dir()?.display().to_string(),
			developer_instructions: PROBE_DEVELOPER_INSTRUCTIONS.to_owned(),
			user_input: PROBE_USER_INPUT.to_owned(),
			max_turns: 1,
			timeout: PROBE_TIMEOUT,
			process_env: AppServerProcessEnv::default(),
			continuation_user_input: None,
			activity_marker_path: None,
			resume_thread_id: None,
			ephemeral_thread: true,
			command_exec_health_check: Some(CommandExecHealthCheck::probe()),
			dynamic_tool_handler: Some(&probe_tool_handler),
			continuation_guard: None,
			phase_goal_controller: None,
			codex_account_provider: None,
		},
		&state_store,
	)?;

	if result.final_output.trim() != PROBE_EXPECTED_OUTPUT {
		eyre::bail!(
			"Protocol probe completed, but the final output was `{}` instead of `{PROBE_EXPECTED_OUTPUT}`.",
			result.final_output.trim()
		);
	}

	Ok(result)
}

fn annotate_transport_failure_phase<T>(
	result: crate::prelude::Result<T>,
	phase: RequestWaitPhase,
) -> crate::prelude::Result<T> {
	result.map_err(|error| transport_failure_at_phase(error, phase))
}

fn transport_failure_at_phase(error: Report, phase: RequestWaitPhase) -> Report {
	let Some(transport_failure) = error.downcast_ref::<json_rpc::AppServerTransportFailure>()
	else {
		return error;
	};

	Report::new(json_rpc::AppServerTransportFailure::with_phase(
		transport_failure.to_string(),
		phase.label(),
		phase.transport_failure_is_retryable_startup(),
	))
}


fn preflight_check_blocker_summary(check: &AppServerCapabilityPreflightCheck) -> String {
	let first_error_path = check.details.get("first_error_path");
	let first_error = check.details.get("first_error");
	let mut summary = format!("{}: {}", check.name, check.summary);

	if first_error_path.is_some() || first_error.is_some() {
		let path = first_error_path.map_or("unknown", String::as_str);
		let error = first_error.map_or("unknown", String::as_str);

		summary.push_str(" first_error_path=");
		summary.push_str(path);
		summary.push_str("; first_error=");
		summary.push_str(error);
	}

	summary
}

fn archive_app_server_thread_after_success_inner(
	request: &AppServerThreadArchiveRequest<'_>,
) -> crate::prelude::Result<()> {
	let expected_codex_home = request.process_env.resolve_codex_home_env()?;
	let mut client = AppServerClient::spawn(request.listen, request.process_env)?;
	let initialize_response = client.initialize(false)?;

	validate_initialize_codex_home(&expected_codex_home, &initialize_response)?;

	client.mark_initialized()?;
	client.archive_thread(ThreadArchiveRequest { thread_id: request.thread_id.to_owned() })?;

	Ok(())
}

fn record_thread_archive_result_best_effort(
	state_store: &StateStore,
	request: &AppServerThreadArchiveRequest<'_>,
	result: std::result::Result<&AppServerThreadArchiveOutcome, &Report>,
) {
	let (event_type, payload) = match result {
		Ok(AppServerThreadArchiveOutcome::Archived) => (
			"thread/archive",
			serde_json::json!({
				"threadId": request.thread_id,
				"issueId": request.issue_id,
				"attemptNumber": request.attempt_number,
			}),
		),
		Ok(AppServerThreadArchiveOutcome::DiscardedMissingThread) => (
			"thread/archive/discarded",
			serde_json::json!({
				"threadId": request.thread_id,
				"issueId": request.issue_id,
				"attemptNumber": request.attempt_number,
				"reason": "missing_thread_or_rollout",
			}),
		),
		Err(error) => (
			"thread/archive/failed",
			serde_json::json!({
				"threadId": request.thread_id,
				"issueId": request.issue_id,
				"attemptNumber": request.attempt_number,
				"error": error.to_string(),
			}),
		),
	};

	if let Err(record_error) = state_store.append_event(
		request.run_id,
		request.sequence_number,
		event_type,
		&payload.to_string(),
	) {
		tracing::warn!(
			?record_error,
			run_id = request.run_id,
			issue_id = request.issue_id,
			attempt = request.attempt_number,
			thread_id = request.thread_id,
			event_type,
			"Failed to record app-server thread archive event."
		);
	}
}

fn thread_archive_error_allows_discard(error: &Report) -> bool {
	let message = error.to_string().to_lowercase();

	thread_missing_error_message_allows_discard(&message) || message.contains("already archived")
}

fn publish_run_control_channel_for_request(
	request: &AppServerRunRequest<'_>,
	state_store: &StateStore,
) -> crate::prelude::Result<Option<RunControlChannel>> {
	let Some(marker_path) = request.activity_marker_path.as_ref() else {
		return Ok(None);
	};
	let channel_path =
		run_control_channel_path(marker_path, &request.run_id, request.attempt_number);

	write_run_control_channel_file(&channel_path, request)?;

	let channel = state_store.publish_run_control_channel_for_active_attempt(
		&request.run_id,
		request.attempt_number,
		&channel_path,
		RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
	)?;

	if let Some(channel) = channel.as_ref() {
		state_store.append_private_execution_event(
			channel.project_id(),
			channel.issue_id(),
			channel.run_id(),
			channel.attempt_number(),
			"control_channel_published",
			serde_json::json!({
				"schema": "decodex.run_control_channel/v1",
				"transport": channel.transport(),
				"channel_path": channel.channel_path().display().to_string(),
				"status": channel.status(),
				"published_at": channel.published_at(),
			}),
		)?;
	}

	Ok(channel)
}

fn run_control_channel_path(marker_path: &Path, run_id: &str, attempt_number: i64) -> PathBuf {
	marker_path
		.join(RUN_CONTROL_CHANNEL_DIR)
		.join(format!("{}-{attempt_number}.channel", sanitize_run_control_path_segment(run_id)))
}

fn sanitize_run_control_path_segment(value: &str) -> String {
	let sanitized = value
		.chars()
		.map(|character| {
			if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
				character
			} else {
				'_'
			}
		})
		.collect::<String>();

	if sanitized.is_empty() { String::from("run") } else { sanitized }
}

fn write_run_control_channel_file(
	channel_path: &Path,
	request: &AppServerRunRequest<'_>,
) -> crate::prelude::Result<()> {
	if let Some(parent) = channel_path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::write(
		channel_path,
		format!(
			"schema=decodex.run_control_channel/v1\nrun_id={}\nissue_id={}\nattempt_number={}\ntransport={}\n",
			request.run_id,
			request.issue_id,
			request.attempt_number,
			state::RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
		),
	)?;

	Ok(())
}

fn write_activity_marker_best_effort(marker_path: &Path, run_id: &str, attempt_number: i64) {
	if let Err(error) = state::write_run_operation_marker(
		marker_path,
		run_id,
		attempt_number,
		RUN_OPERATION_AGENT_RUN,
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree activity marker."
		);
	}
}

fn write_activity_marker_best_effort_for_request(request: &AppServerRunRequest<'_>) {
	if let Some(marker_path) = request.activity_marker_path.as_ref() {
		write_activity_marker_best_effort(marker_path, &request.run_id, request.attempt_number);
	}
}

fn write_capability_preflight_marker_best_effort(request: &AppServerRunRequest<'_>) {
	if let Some(marker_path) = request.activity_marker_path.as_ref()
		&& let Err(error) = state::write_run_operation_marker(
			marker_path,
			&request.run_id,
			request.attempt_number,
			RUN_OPERATION_APP_SERVER_PREFLIGHT,
		) {
		tracing::warn!(
			?error,
			run_id = request.run_id,
			attempt_number = request.attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree app-server preflight marker."
		);
	}
}

fn write_protocol_activity_marker_best_effort(
	marker_path: &Path,
	activity: &state::ProtocolActivityMarker<'_>,
) {
	if let Err(error) = state::write_run_protocol_activity_marker(marker_path, activity) {
		tracing::warn!(
			?error,
			run_id = activity.run_id,
			attempt_number = activity.attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree protocol-activity marker."
		);
	}
}

fn write_turn_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	turn_id: &str,
) {
	if let Err(error) = state::write_run_turn_marker(marker_path, run_id, attempt_number, turn_id) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree turn marker."
		);
	}
}

fn write_thread_status_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: Option<&str>,
	turn_id: Option<&str>,
	thread_status: &str,
	thread_active_flags: &[String],
) {
	if let Err(error) = state::write_run_thread_status_marker(
		marker_path,
		run_id,
		attempt_number,
		thread_id,
		turn_id,
		thread_status,
		thread_active_flags,
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree thread-status marker."
		);
	}
}

fn write_effective_runtime_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: Option<&str>,
	turn_id: Option<&str>,
	runtime: &EffectiveThreadConfig,
) {
	if let Err(error) = state::write_run_effective_runtime_marker(
		marker_path,
		run_id,
		attempt_number,
		&EffectiveRuntimeMarker {
			thread_id,
			turn_id,
			effective_model: &runtime.model,
			effective_model_provider: &runtime.model_provider,
			effective_cwd: &runtime.cwd,
			effective_approval_policy: &runtime.approval_policy,
			effective_approvals_reviewer: &runtime.approvals_reviewer,
			effective_sandbox_mode: &runtime.sandbox_mode,
		},
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree effective-runtime marker."
		);
	}
}

fn write_codex_account_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	summary: &CodexAccountActivitySummary,
	account_summaries: &[CodexAccountActivitySummary],
) {
	if let Err(error) = state::write_run_account_marker(
		marker_path,
		&CodexAccountMarker {
			run_id,
			attempt_number,
			account: summary,
			accounts: account_summaries,
		},
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree Codex account marker."
		);
	}
}

fn write_thread_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: &str,
) {
	if let Err(error) =
		state::write_run_thread_marker(marker_path, run_id, attempt_number, thread_id)
	{
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree thread marker."
		);
	}
}

fn execute_app_server_run_inner(
	request: &AppServerRunRequest<'_>,
	state_store: &StateStore,
) -> crate::prelude::Result<AppServerRunResult> {
	let mut recorder = RunRecorder::new_with_context(
		state_store,
		&request.project_id,
		&request.issue_id,
		&request.run_id,
		request.attempt_number,
		request.activity_marker_path.as_ref(),
	);
	let expected_codex_home = request.process_env.resolve_codex_home_env()?;
	let mut client = AppServerClient::spawn(&request.listen, &request.process_env)?;
	let initialize_response = initialize_client_for_run(
		&mut client,
		&mut recorder,
		request.dynamic_tool_handler,
		&expected_codex_home,
	)?;

	client.mark_initialized()?;

	write_capability_preflight_marker_best_effort(request);

	let capability_preflight =
		run_app_server_capability_preflight(&mut client, &mut recorder, &request.cwd)?;

	write_activity_marker_best_effort_for_request(request);

	if let Some(health_check) = request.command_exec_health_check.as_ref() {
		run_command_exec_health_check(&mut client, &mut recorder, request, health_check)?;
	}

	flush_pending_messages(&mut client, &mut recorder, None)?;
	login_codex_account_for_run(&mut client, &mut recorder, request)?;
	flush_pending_messages(&mut client, &mut recorder, None)?;

	let thread_response = start_or_resume_thread_session(&mut client, &mut recorder, request)?;
	let thread_id = thread_response.thread.id.clone();
	let effective_thread_config = thread_response.effective_config();

	record_thread_session_start(
		state_store,
		request,
		&mut recorder,
		&thread_id,
		&effective_thread_config,
	)?;
	flush_pending_messages(&mut client, &mut recorder, Some(&thread_id))?;

	state_store.record_run_attempt(
		&request.run_id,
		&request.issue_id,
		request.attempt_number,
		"running",
	)?;
	recorder.mark_activity()?;

	let turn_result =
		execute_turn_loop(&mut client, &mut recorder, request, state_store, &thread_id)?;

	state_store.record_run_attempt(
		&request.run_id,
		&request.issue_id,
		request.attempt_number,
		"succeeded",
	)?;
	recorder.mark_activity()?;

	Ok(AppServerRunResult {
		user_agent: initialize_response.user_agent,
		capability_preflight,
		thread_id,
		turn_id: turn_result.turn_id,
		turn_count: turn_result.turn_count,
		event_count: state_store.event_count(&request.run_id)?,
		final_output: turn_result.final_output,
		continuation_pending: turn_result.continuation_pending,
		phase_goal_status: turn_result.phase_goal_status,
	})
}

fn initialize_client_for_run(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	expected_codex_home: &ResolvedAppServerCodexHomeEnv,
) -> crate::prelude::Result<InitializeResponse> {
	let response = annotate_transport_failure_phase(
		client.initialize_with_handler(
			dynamic_tool_handler.is_some(),
			|connection, wire_message, server_request| {
				handle_server_request_while_waiting(
					connection,
					recorder,
					wire_message,
					server_request,
					RequestDispatchContext::new(
						RequestWaitPhase::Initialize,
						dynamic_tool_handler,
						None,
						None,
						None,
					),
				)
			},
		),
		RequestWaitPhase::Initialize,
	)?;

	validate_initialize_codex_home(expected_codex_home, &response)?;

	Ok(response)
}

fn run_app_server_capability_preflight(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	cwd: &str,
) -> crate::prelude::Result<AppServerCapabilityPreflightReport> {
	let mut report = AppServerCapabilityPreflightReport::new();
	let config = preflight_request(recorder, &report, "config/read", || {
		client.read_config(&ConfigReadParams { cwd: Some(cwd.to_owned()), include_layers: false })
	})?;

	record_config_preflight(&mut report, &config.config);

	let models = list_all_models_for_preflight(client, recorder, &report)?;

	record_model_preflight(&mut report, &config.config, &models);

	let provider_capabilities =
		preflight_request(recorder, &report, "modelProvider/capabilities/read", || {
			client.read_model_provider_capabilities()
		})?;

	record_model_provider_preflight(&mut report, &provider_capabilities);

	let skills = preflight_request(recorder, &report, "skills/list", || {
		client.list_skills(&SkillsListParams {
			cwds: vec![cwd.to_owned()],
			force_reload: false,
			per_cwd_extra_user_roots: None,
		})
	})?;

	record_skills_preflight(&mut report, cwd, &skills);

	let plugins = preflight_request_with_timeout_retry(
		recorder,
		&report,
		"plugin/list",
		REQUEST_TIMEOUT,
		PLUGIN_PREFLIGHT_MAX_ATTEMPTS,
		|| client.list_plugins(&plugin_list_params_for_preflight(cwd)),
	)?;

	record_plugin_preflight(&mut report, &plugins);

	match list_all_mcp_servers_for_preflight(client) {
		Ok(mcp_servers) => record_mcp_preflight(&mut report, &mcp_servers),
		Err(error) if mcp_preflight_can_degrade(&error) => {
			record_mcp_preflight_degraded(&mut report, &error);
		},
		Err(error) => {
			return preflight_method_failure(
				recorder,
				&report,
				"mcpServerStatus/list",
				MCP_PREFLIGHT_REQUEST_TIMEOUT,
				1,
				error,
			);
		},
	}

	record_app_server_preflight_report(recorder, &report)?;

	if report.has_blockers() {
		return Err(Report::new(AppServerCapabilityPreflightFailure::blocked(report)));
	}

	Ok(report)
}

fn plugin_list_params_for_preflight(cwd: &str) -> PluginListParams {
	PluginListParams {
		cwds: Some(vec![cwd.to_owned()]),
		marketplace_kinds: Some(vec![PREFLIGHT_PLUGIN_MARKETPLACE_KIND.to_owned()]),
	}
}

fn preflight_method_failure<T>(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
	method: &'static str,
	request_timeout: Duration,
	attempt_count: u32,
	error: Report,
) -> crate::prelude::Result<T> {
	let error_message = error.to_string();
	let timed_out = preflight_error_timed_out(&error);
	let retry_count = attempt_count.saturating_sub(1);
	let mut failed_report = report.clone();
	let mut details = BTreeMap::new();

	details.insert(String::from("method"), method.to_owned());
	details.insert(String::from("error"), error_message.clone());
	details.insert(String::from("attempt_count"), attempt_count.to_string());

	if retry_count > 0 {
		details.insert(String::from("retry_count"), retry_count.to_string());
	}
	if timed_out {
		details.insert(String::from("failure_reason"), String::from("timeout"));
		details.insert(String::from("timeout_seconds"), request_timeout.as_secs().to_string());
	}

	failed_report.push_blocked(
		check_name_for_method(method),
		if timed_out {
			format!("`{method}` timed out before thread/start after {attempt_count} attempts.")
		} else {
			format!("`{method}` failed before thread/start.")
		},
		details,
	);

	record_app_server_preflight_report(recorder, &failed_report)?;

	let failure = if timed_out {
		AppServerCapabilityPreflightFailure::method_timed_out(method, error_message, failed_report)
	} else {
		AppServerCapabilityPreflightFailure::method_failed(method, error_message, failed_report)
	};

	Err(Report::new(failure))
}

fn preflight_request<T, F>(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
	method: &'static str,
	request: F,
) -> crate::prelude::Result<T>
where
	F: FnOnce() -> crate::prelude::Result<T>,
{
	match request() {
		Ok(response) => Ok(response),
		Err(error) => preflight_method_failure(recorder, report, method, REQUEST_TIMEOUT, 1, error),
	}
}

fn preflight_request_with_timeout_retry<T, F>(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
	method: &'static str,
	request_timeout: Duration,
	max_attempts: u32,
	mut request: F,
) -> crate::prelude::Result<T>
where
	F: FnMut() -> crate::prelude::Result<T>,
{
	let max_attempts = max_attempts.max(1);
	let mut attempt_count = 1;

	loop {
		match request() {
			Ok(response) => return Ok(response),
			Err(error) if preflight_error_timed_out(&error) && attempt_count < max_attempts => {
				tracing::warn!(
					method,
					attempt = attempt_count,
					max_attempts,
					"Retrying app-server preflight method after timeout."
				);

				attempt_count += 1;
			},
			Err(error) => {
				return preflight_method_failure(
					recorder,
					report,
					method,
					request_timeout,
					attempt_count,
					error,
				);
			},
		}
	}
}

fn list_all_models_for_preflight(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
) -> crate::prelude::Result<Vec<ModelSummary>> {
	let mut cursor = None;
	let mut models = Vec::new();

	loop {
		let response: ModelListResponse =
			preflight_request(recorder, report, "model/list", || {
				client.list_models(&ModelListParams {
					cursor: cursor.clone(),
					include_hidden: Some(true),
					limit: Some(PREFLIGHT_MODEL_PAGE_LIMIT),
				})
			})?;

		models.extend(response.data);

		let Some(next_cursor) = response.next_cursor else {
			return Ok(models);
		};

		cursor = Some(next_cursor);
	}
}

fn list_all_mcp_servers_for_preflight(
	client: &mut AppServerClient,
) -> crate::prelude::Result<Vec<McpServerStatusSummary>> {
	let mut cursor = None;
	let mut servers = Vec::new();

	loop {
		let response: ListMcpServerStatusResponse = client.list_mcp_server_status(
			&ListMcpServerStatusParams {
				cursor: cursor.clone(),
				detail: Some(PREFLIGHT_MCP_DETAIL.to_owned()),
				limit: Some(PREFLIGHT_MCP_PAGE_LIMIT),
			},
			MCP_PREFLIGHT_REQUEST_TIMEOUT,
		)?;

		servers.extend(response.data);

		let Some(next_cursor) = response.next_cursor else {
			return Ok(servers);
		};

		cursor = Some(next_cursor);
	}
}

fn record_config_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	config: &RuntimeConfigSummary,
) {
	let mut details = BTreeMap::new();

	insert_optional_detail(&mut details, "model", config.model.as_deref());
	insert_optional_detail(&mut details, "model_provider", config.model_provider.as_deref());

	if let Some(approval_policy) = config.approval_policy.as_ref().and_then(config_value_name) {
		details.insert(String::from("approval_policy"), approval_policy);
	}
	if let Some(sandbox_mode) = config.sandbox_mode.as_ref().and_then(config_value_name) {
		details.insert(String::from("sandbox_mode"), sandbox_mode);
	}

	report.push_ok(
		PREFLIGHT_CHECK_CONFIG,
		"config/read returned effective runtime configuration.",
		details,
	);
}

fn record_model_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	config: &RuntimeConfigSummary,
	models: &[ModelSummary],
) {
	let configured_model = config.model.as_deref().filter(|model| !model.trim().is_empty());
	let default_model = models.iter().find(|model| model.is_default);
	let matching_config_model = configured_model
		.and_then(|configured| models.iter().find(|model| model_matches_config(model, configured)));
	let mut details = BTreeMap::new();

	details.insert(String::from("model_count"), models.len().to_string());

	if let Some(configured_model) = configured_model {
		details.insert(String::from("configured_model"), configured_model.to_owned());
	}
	if let Some(model) = default_model {
		details.insert(String::from("default_model"), model.model.clone());
	}
	if let Some(model) = matching_config_model {
		details.insert(String::from("matched_model_id"), model.id.clone());
	}

	if models.is_empty() {
		report.push_blocked(
			PREFLIGHT_CHECK_MODEL,
			"model/list returned no available models.",
			details,
		);
	} else if configured_model.is_some() && matching_config_model.is_none() {
		report.push_blocked(
			PREFLIGHT_CHECK_MODEL,
			"configured model was not present in model/list.",
			details,
		);
	} else if configured_model.is_none() && default_model.is_none() {
		report.push_blocked(
			PREFLIGHT_CHECK_MODEL,
			"no configured model or default model was present.",
			details,
		);
	} else {
		report.push_ok(
			PREFLIGHT_CHECK_MODEL,
			"model/list returned an executable model selection.",
			details,
		);
	}
}

fn record_model_provider_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	capabilities: &ModelProviderCapabilitiesReadResponse,
) {
	let mut details = BTreeMap::new();

	details.insert(String::from("web_search"), capabilities.web_search.to_string());
	details.insert(String::from("image_generation"), capabilities.image_generation.to_string());
	details.insert(String::from("namespace_tools"), capabilities.namespace_tools.to_string());
	report.push_ok(
		PREFLIGHT_CHECK_MODEL_PROVIDER,
		"modelProvider/capabilities/read returned provider capabilities.",
		details,
	);
}

fn record_skills_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	cwd: &str,
	skills: &SkillsListResponse,
) {
	let cwd_entry = skills.data.iter().find(|entry| entry.cwd == cwd);
	let all_skill_count: usize = skills.data.iter().map(|entry| entry.skills.len()).sum();
	let enabled_skill_count: usize = skills
		.data
		.iter()
		.flat_map(|entry| entry.skills.iter())
		.filter(|skill| skill.enabled)
		.count();
	let errors = skills.data.iter().flat_map(|entry| entry.errors.iter()).collect::<Vec<_>>();
	let mut details = BTreeMap::new();

	details.insert(String::from("cwd"), cwd.to_owned());
	details.insert(String::from("entry_count"), skills.data.len().to_string());
	details.insert(String::from("skill_count"), all_skill_count.to_string());
	details.insert(String::from("enabled_skill_count"), enabled_skill_count.to_string());
	details.insert(String::from("error_count"), errors.len().to_string());

	if let Some(first_error) = errors.first() {
		details.insert(String::from("first_error_path"), first_error.path.clone());
		details.insert(String::from("first_error"), first_error.message.clone());
	}

	if cwd_entry.is_none() {
		report.push_blocked(
			PREFLIGHT_CHECK_SKILLS,
			"skills/list did not return an entry for the run cwd.",
			details,
		);
	} else if enabled_skill_count == 0 {
		report.push_blocked(
			PREFLIGHT_CHECK_SKILLS,
			"skills/list returned no enabled skills.",
			details,
		);
	} else if errors.is_empty() {
		report.push_ok(PREFLIGHT_CHECK_SKILLS, "skills/list returned enabled skills.", details);
	} else {
		report.push_ok(
			PREFLIGHT_CHECK_SKILLS,
			"skills/list returned enabled skills with scan diagnostics.",
			details,
		);
	}
}

fn record_plugin_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	plugins: &PluginListResponse,
) {
	let plugin_count: usize =
		plugins.marketplaces.iter().map(|marketplace| marketplace.plugins.len()).sum();
	let installed_count = plugins
		.marketplaces
		.iter()
		.flat_map(|marketplace| marketplace.plugins.iter())
		.filter(|plugin| plugin.installed)
		.count();
	let enabled_count = plugins
		.marketplaces
		.iter()
		.flat_map(|marketplace| marketplace.plugins.iter())
		.filter(|plugin| plugin.enabled)
		.count();
	let mut details = BTreeMap::new();

	details.insert(String::from("marketplace_count"), plugins.marketplaces.len().to_string());
	details.insert(String::from("plugin_count"), plugin_count.to_string());
	details.insert(String::from("installed_plugin_count"), installed_count.to_string());
	details.insert(String::from("enabled_plugin_count"), enabled_count.to_string());

	if let Some(first_error) = plugins.marketplace_load_errors.first() {
		details.insert(String::from("first_error_path"), first_error.marketplace_path.clone());
		details.insert(String::from("first_error"), first_error.message.clone());
	}

	if !plugins.marketplace_load_errors.is_empty() {
		report.push_blocked(
			PREFLIGHT_CHECK_PLUGINS,
			"plugin/list returned marketplace load errors.",
			details,
		);
	} else if plugins.marketplaces.is_empty() {
		report.push_blocked(
			PREFLIGHT_CHECK_PLUGINS,
			"plugin/list returned no marketplaces.",
			details,
		);
	} else {
		report.push_ok(PREFLIGHT_CHECK_PLUGINS, "plugin/list returned plugin inventory.", details);
	}
}

fn record_mcp_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	servers: &[McpServerStatusSummary],
) {
	let not_logged_in = servers
		.iter()
		.filter(|server| server.auth_status == "notLoggedIn")
		.map(|server| server.name.clone())
		.collect::<Vec<_>>();
	let tool_count: usize = servers.iter().map(|server| server.tools.len()).sum();
	let mut details = BTreeMap::new();

	details.insert(String::from("server_count"), servers.len().to_string());
	details.insert(String::from("tool_count"), tool_count.to_string());

	if !not_logged_in.is_empty() {
		details.insert(String::from("not_logged_in_servers"), not_logged_in.join(", "));
	}
	if !not_logged_in.is_empty() {
		report.push_blocked(
			PREFLIGHT_CHECK_MCP,
			"mcpServerStatus/list returned MCP servers that are not logged in.",
			details,
		);
	} else {
		report.push_ok(
			PREFLIGHT_CHECK_MCP,
			"mcpServerStatus/list returned MCP server state.",
			details,
		);
	}
}

fn mcp_preflight_can_degrade(error: &Report) -> bool {
	preflight_error_timed_out(error)
}

fn preflight_error_timed_out(error: &Report) -> bool {
	error.downcast_ref::<AppServerOutputTimeout>().is_some()
}

fn record_mcp_preflight_degraded(report: &mut AppServerCapabilityPreflightReport, error: &Report) {
	let mut details = BTreeMap::new();

	details.insert(String::from("method"), String::from("mcpServerStatus/list"));
	details.insert(String::from("degraded_reason"), String::from("timeout"));
	details.insert(String::from("error"), error.to_string());
	details.insert(
		String::from("timeout_seconds"),
		MCP_PREFLIGHT_REQUEST_TIMEOUT.as_secs().to_string(),
	);
	report.push_ok(
		PREFLIGHT_CHECK_MCP,
		"mcpServerStatus/list timed out during optional MCP inventory; continuing after core app-server capability checks passed.",
		details,
	);
}

fn record_app_server_preflight_report(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
) -> crate::prelude::Result<()> {
	recorder.record(PREFLIGHT_EVENT_TYPE, &serde_json::to_string(report)?)
}

fn model_matches_config(model: &ModelSummary, configured_model: &str) -> bool {
	model.model == configured_model || model.id == configured_model
}

fn insert_optional_detail(details: &mut BTreeMap<String, String>, name: &str, value: Option<&str>) {
	if let Some(value) = value.filter(|value| !value.is_empty()) {
		details.insert(name.to_owned(), value.to_owned());
	}
}

fn config_value_name(value: &Value) -> Option<String> {
	match value {
		Value::String(value) if !value.is_empty() => Some(value.clone()),
		Value::Object(object) => object
			.get("type")
			.and_then(Value::as_str)
			.map(str::to_owned)
			.or_else(|| (object.len() == 1).then(|| object.keys().next().cloned()).flatten()),
		_ => None,
	}
}

fn check_name_for_method(method: &str) -> &'static str {
	match method {
		"config/read" => PREFLIGHT_CHECK_CONFIG,
		"model/list" => PREFLIGHT_CHECK_MODEL,
		"modelProvider/capabilities/read" => PREFLIGHT_CHECK_MODEL_PROVIDER,
		"skills/list" => PREFLIGHT_CHECK_SKILLS,
		"plugin/list" => PREFLIGHT_CHECK_PLUGINS,
		"mcpServerStatus/list" => PREFLIGHT_CHECK_MCP,
		_ => "introspection",
	}
}

fn run_command_exec_health_check(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	health_check: &CommandExecHealthCheck,
) -> crate::prelude::Result<()> {
	let params = build_command_exec_health_check_params(health_check, &request.cwd);
	let response = client.command_exec(&params)?;

	flush_pending_messages(client, recorder, None)?;

	validate_command_exec_health_check_result(health_check, &response)
}

fn build_command_exec_health_check_params(
	health_check: &CommandExecHealthCheck,
	cwd: &str,
) -> CommandExecParams {
	CommandExecParams {
		command: health_check.command.clone(),
		cwd: Some(cwd.to_owned()),
		timeout_ms: Some(health_check.timeout_ms),
		output_bytes_cap: Some(health_check.output_bytes_cap),
	}
}

fn validate_command_exec_health_check_result(
	health_check: &CommandExecHealthCheck,
	response: &CommandExecResponse,
) -> crate::prelude::Result<()> {
	if response.exit_code != 0 {
		eyre::bail!(
			"`command/exec` health check failed with exit code {}. stdout: {:?}; stderr: {:?}",
			response.exit_code,
			response.stdout,
			response.stderr
		);
	}
	if response.stdout != health_check.expected_stdout {
		eyre::bail!(
			"`command/exec` health check returned stdout {:?}, expected {:?}. stderr: {:?}",
			response.stdout,
			health_check.expected_stdout,
			response.stderr
		);
	}
	if !response.stderr.is_empty() {
		eyre::bail!("`command/exec` health check wrote unexpected stderr: {:?}", response.stderr);
	}

	Ok(())
}

fn login_codex_account_for_run(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
) -> crate::prelude::Result<()> {
	let Some(account_provider) = request.codex_account_provider else {
		return Ok(());
	};
	let account = match account_provider.select_account() {
		Ok(account) => account,
		Err(error) => {
			record_codex_account_failure(recorder, "account/login/select/failed", &error);

			return Err(error);
		},
	};

	recorder.set_codex_account(account.summary(), account.account_summaries())?;

	record_codex_account_login(recorder, account.summary())?;

	let response = annotate_transport_failure_phase(
		client.login_account_with_handler(
			login_account_params(&account),
			|connection, wire_message, server_request| {
				handle_server_request_while_waiting(
					connection,
					recorder,
					wire_message,
					server_request,
					RequestDispatchContext::new(
						RequestWaitPhase::AccountLogin,
						request.dynamic_tool_handler,
						request.codex_account_provider,
						None,
						None,
					),
				)
			},
		),
		RequestWaitPhase::AccountLogin,
	)?;

	match response {
		LoginAccountResponse::ChatgptAuthTokens {} => {
			recorder.record(
				"account/login/start/response",
				&serde_json::json!({
					"type": "chatgptAuthTokens",
					"accountFingerprint": account.summary().account_fingerprint.as_str(),
					"planType": account.summary().plan_type.as_deref(),
				})
				.to_string(),
			)?;
		},
	}

	Ok(())
}

fn record_codex_account_failure(recorder: &mut RunRecorder<'_>, event_type: &str, error: &Report) {
	let auth_failure = error.downcast_ref::<CodexAccountAuthFailure>();
	let error_class =
		auth_failure.map(CodexAccountAuthFailure::error_class).unwrap_or("codex_account_failure");
	let account_fingerprint = auth_failure.and_then(CodexAccountAuthFailure::account_fingerprint);
	let email = auth_failure.and_then(CodexAccountAuthFailure::email);
	let reason =
		auth_failure.map_or_else(|| error.to_string(), |failure| failure.reason().to_owned());
	let payload = serde_json::json!({
		"errorClass": error_class,
		"accountFingerprint": account_fingerprint,
		"email": email,
		"reason": reason,
	});

	if let Err(record_error) = recorder.record(event_type, &payload.to_string()) {
		tracing::warn!(
			?record_error,
			event_type,
			error_class,
			"Failed to record Codex account failure event."
		);
	}
}

fn start_or_resume_thread_session(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
) -> crate::prelude::Result<ThreadSessionResponse> {
	if let Some(resume_thread_id) = request.resume_thread_id.as_deref() {
		return resume_existing_thread_session(client, recorder, request, resume_thread_id);
	}

	start_fresh_thread_session(client, recorder, request)
}

fn start_fresh_thread_session(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
) -> crate::prelude::Result<ThreadSessionResponse> {
	let thread_start_request = build_thread_start_request(request)?;

	annotate_transport_failure_phase(
		client.start_thread_with_handler(
			thread_start_request,
			|connection, wire_message, server_request| {
				handle_server_request_while_waiting(
					connection,
					recorder,
					wire_message,
					server_request,
					RequestDispatchContext::new(
						RequestWaitPhase::ThreadStart,
						request.dynamic_tool_handler,
						request.codex_account_provider,
						None,
						None,
					),
				)
			},
		),
		RequestWaitPhase::ThreadStart,
	)
}

fn resume_existing_thread_session(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	resume_thread_id: &str,
) -> crate::prelude::Result<ThreadSessionResponse> {
	match client.resume_thread_with_handler(
		build_thread_resume_request(resume_thread_id, request),
		|connection, wire_message, server_request| {
			handle_server_request_while_waiting(
				connection,
				recorder,
				wire_message,
				server_request,
				RequestDispatchContext::new(
					RequestWaitPhase::ThreadResume,
					request.dynamic_tool_handler,
					request.codex_account_provider,
					Some(resume_thread_id),
					None,
				),
			)
		},
	) {
		Ok(response) => Ok(response),
		Err(error) if thread_resume_error_allows_fallback(&error) => {
			recorder.record(
				"thread/resume/miss",
				&serde_json::json!({
					"requestedThreadId": resume_thread_id,
					"error": error.to_string(),
				})
				.to_string(),
			)?;

			start_fresh_thread_session(client, recorder, request)
		},
		Err(error) => Err(transport_failure_at_phase(error, RequestWaitPhase::ThreadResume)),
	}
}

fn record_thread_session_start(
	state_store: &StateStore,
	request: &AppServerRunRequest<'_>,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
	effective_thread_config: &EffectiveThreadConfig,
) -> crate::prelude::Result<()> {
	state_store.update_run_thread(&request.run_id, thread_id)?;
	recorder.set_thread_id(thread_id)?;
	recorder.set_effective_runtime(effective_thread_config)?;

	validate_effective_thread_config(&request.cwd, effective_thread_config)?;

	recorder.mark_activity()
}

fn execute_turn_loop(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	state_store: &StateStore,
	thread_id: &str,
) -> crate::prelude::Result<TurnLoopResult> {
	let mut next_input = request.user_input.clone();
	let mut turn_count = 0_u32;
	let mut phase_goal_runtime =
		initialize_phase_goal_runtime(client, recorder, request, thread_id)?;
	let mut phase_goal_status = phase_goal_runtime.as_ref().map(|runtime| PhaseGoalRunStatus {
		phase: runtime.active_goal.phase,
		status: ThreadGoalStatus::Active.as_str().to_owned(),
	});

	loop {
		let turn_id = start_turn_for_run(
			client,
			recorder,
			request.dynamic_tool_handler,
			request.codex_account_provider,
			thread_id,
			&next_input,
		)?;

		turn_count = turn_count.saturating_add(1);

		state_store.update_run_turn(&request.run_id, &turn_id)?;
		recorder.set_turn_id(&turn_id)?;

		flush_pending_messages(client, recorder, Some(thread_id))?;

		let outcome = wait_for_turn_completion(client, recorder, request, thread_id, &turn_id)?;
		let final_turn_id = outcome.turn_id;
		let final_output = outcome.final_output;

		if let Some((continuation_pending, observed_phase_goal_status)) = resolve_turn_completion(
			client,
			recorder,
			request,
			&mut phase_goal_runtime,
			thread_id,
			turn_count,
			&final_output,
		)? {
			if observed_phase_goal_status.is_some() {
				phase_goal_status = observed_phase_goal_status;
			}

			return Ok(TurnLoopResult {
				turn_id: final_turn_id,
				turn_count,
				final_output,
				continuation_pending,
				phase_goal_status,
			});
		}

		phase_goal_status = phase_goal_runtime.as_ref().map(|runtime| PhaseGoalRunStatus {
			phase: runtime.active_goal.phase,
			status: ThreadGoalStatus::Active.as_str().to_owned(),
		});
		next_input =
			request.continuation_user_input.clone().unwrap_or_else(|| request.user_input.clone());
	}
}

fn start_turn_for_run(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	codex_account_provider: Option<&dyn CodexAccountProvider>,
	thread_id: &str,
	next_input: &str,
) -> crate::prelude::Result<String> {
	let turn_response = annotate_transport_failure_phase(
		client.start_turn_with_handler(
			build_turn_start_request(thread_id, next_input),
			|connection, wire_message, server_request| {
				handle_server_request_while_waiting(
					connection,
					recorder,
					wire_message,
					server_request,
					RequestDispatchContext::new(
						RequestWaitPhase::TurnStart,
						dynamic_tool_handler,
						codex_account_provider,
						Some(thread_id),
						None,
					),
				)
			},
		),
		RequestWaitPhase::TurnStart,
	)?;

	Ok(turn_response.turn.id)
}

fn resolve_turn_completion(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	phase_goal_runtime: &mut Option<PhaseGoalRuntime<'_>>,
	thread_id: &str,
	turn_count: u32,
	final_output: &str,
) -> crate::prelude::Result<Option<(bool, Option<PhaseGoalRunStatus>)>> {
	let completion_status = classify_turn_completion(request.dynamic_tool_handler, final_output)?;
	let terminal_completion_signal = has_terminal_completion_signal(request.dynamic_tool_handler);

	if phase_goal_runtime.is_some() {
		let observed_goal_result = {
			let runtime = phase_goal_runtime
				.as_ref()
				.expect("phase goal runtime should be present after is_some check");

			get_thread_phase_goal(client, recorder, thread_id, runtime)
		};
		let observed_goal = match observed_goal_result {
			Ok(goal) => goal,
			Err(error) if app_server_method_not_found(&error) => {
				return Err(Report::new(AppServerPhaseGoalFailure::unsupported("thread/goal/get"))
					.wrap_err(error));
			},
			Err(error) => return Err(error),
		};
		let runtime = phase_goal_runtime
			.as_mut()
			.expect("phase goal runtime should still be present after goal status read");
		let observed_status = PhaseGoalRunStatus {
			phase: runtime.active_goal.phase,
			status: observed_goal.status.as_str().to_owned(),
		};

		if observed_goal.status == ThreadGoalStatus::Complete {
			let transition = runtime.controller.phase_goal_completed(runtime.active_goal.phase)?;

			record_phase_goal_completed(recorder, runtime.active_goal.phase, &observed_goal)?;

			match transition {
				PhaseGoalTransition::Continue(next_goal) => {
					if completion_status == TurnCompletionStatus::Complete
						&& terminal_completion_signal
					{
						return Ok(Some((false, Some(observed_status))));
					}

					set_thread_phase_goal(client, recorder, thread_id, &next_goal)?;

					runtime.active_goal = next_goal;

					if turn_count >= request.max_turns {
						return Ok(Some((true, Some(observed_status))));
					}
					if continuation_boundary_reached(request.continuation_guard, turn_count)? {
						return Ok(Some((true, Some(observed_status))));
					}

					return Ok(None);
				},
				PhaseGoalTransition::CompleteRun => {
					if completion_status == TurnCompletionStatus::Complete
						&& terminal_completion_signal
					{
						clear_thread_phase_goal_best_effort(client, recorder, thread_id);

						return Ok(Some((false, Some(observed_status))));
					}

					return Err(Report::new(AppServerPhaseGoalFailure::missing_terminal_path(
						runtime.active_goal.phase,
					)));
				},
			}
		}
		if completion_status == TurnCompletionStatus::Complete && terminal_completion_signal {
			clear_thread_phase_goal_best_effort(client, recorder, thread_id);

			return Ok(Some((false, Some(observed_status))));
		}
		if turn_count >= request.max_turns {
			return Ok(Some((true, Some(observed_status))));
		}
		if continuation_boundary_reached(request.continuation_guard, turn_count)? {
			return Ok(Some((true, Some(observed_status))));
		}

		return Ok(None);
	}

	resolve_turn_completion_without_phase_goal(request, turn_count, completion_status, final_output)
		.map(|result| result.map(|continuation_pending| (continuation_pending, None)))
}

fn resolve_turn_completion_without_phase_goal(
	request: &AppServerRunRequest<'_>,
	turn_count: u32,
	completion_status: TurnCompletionStatus,
	final_output: &str,
) -> crate::prelude::Result<Option<bool>> {
	match completion_status {
		TurnCompletionStatus::Complete => Ok(Some(false)),
		TurnCompletionStatus::Continue => {
			if request.max_turns <= 1 {
				reject_nonterminal_single_turn_completion(
					request.dynamic_tool_handler,
					final_output,
				)?;
			}
			if turn_count >= request.max_turns {
				return Ok(Some(true));
			}
			if continuation_boundary_reached(request.continuation_guard, turn_count)? {
				return Ok(Some(true));
			}

			Ok(None)
		},
	}
}

fn initialize_phase_goal_runtime<'a>(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &'a AppServerRunRequest<'_>,
	thread_id: &str,
) -> crate::prelude::Result<Option<PhaseGoalRuntime<'a>>> {
	let Some(controller) = request.phase_goal_controller else {
		return Ok(None);
	};
	let Some(active_goal) = controller.initial_phase_goal()? else {
		return Ok(None);
	};

	match set_thread_phase_goal(client, recorder, thread_id, &active_goal) {
		Ok(()) => Ok(Some(PhaseGoalRuntime { controller, active_goal })),
		Err(error) if app_server_method_not_found(&error) =>
			Err(Report::new(AppServerPhaseGoalFailure::unsupported("thread/goal/set"))
				.wrap_err(error)),
		Err(error) => Err(error),
	}
}

fn set_thread_phase_goal(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
	goal: &PhaseGoalSpec,
) -> crate::prelude::Result<()> {
	let response = client.set_thread_goal(ThreadGoalSetParams {
		thread_id: thread_id.to_owned(),
		objective: Some(goal.objective.clone()),
		status: Some(ThreadGoalStatus::Active),
		token_budget: goal.token_budget,
	})?;
	let payload = serde_json::json!({
		"phase": goal.phase.as_str(),
		"status": response.goal.status.as_str(),
		"threadId": response.goal.thread_id,
		"tokenBudget": response.goal.token_budget,
		"tokensUsed": response.goal.tokens_used,
		"timeUsedSeconds": response.goal.time_used_seconds,
	});

	recorder.record("thread/goal/set", &payload.to_string())?;

	record_phase_goal_private_event(recorder, "phase_goal_set", goal.phase, &payload)
}

fn get_thread_phase_goal(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
	runtime: &PhaseGoalRuntime<'_>,
) -> crate::prelude::Result<ThreadGoal> {
	let response =
		client.get_thread_goal(ThreadGoalGetParams { thread_id: thread_id.to_owned() })?;
	let goal = response.goal.ok_or_else(|| {
		Report::new(AppServerPhaseGoalFailure::missing_terminal_path(runtime.active_goal.phase))
			.wrap_err("Codex app-server returned no active phase goal for a goal-controlled lane.")
	})?;
	let payload = serde_json::json!({
		"phase": runtime.active_goal.phase.as_str(),
		"status": goal.status.as_str(),
		"threadId": goal.thread_id,
		"tokenBudget": goal.token_budget,
		"tokensUsed": goal.tokens_used,
		"timeUsedSeconds": goal.time_used_seconds,
	});

	recorder.record("thread/goal/get", &payload.to_string())?;

	record_phase_goal_private_event(
		recorder,
		"phase_goal_status",
		runtime.active_goal.phase,
		&payload,
	)?;

	Ok(goal)
}

fn clear_thread_phase_goal_best_effort(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
) {
	match client.clear_thread_goal(ThreadGoalClearParams { thread_id: thread_id.to_owned() }) {
		Ok(response) => {
			let payload = serde_json::json!({ "cleared": response.cleared, "threadId": thread_id });

			if let Err(error) = recorder.record("thread/goal/clear", &payload.to_string()) {
				tracing::warn!(?error, "Failed to record app-server goal clear response.");
			}
		},
		Err(error) => {
			tracing::warn!(?error, "Failed to clear app-server phase goal after terminal path.")
		},
	}
}

fn record_phase_goal_completed(
	recorder: &mut RunRecorder<'_>,
	phase: PhaseGoalKind,
	goal: &ThreadGoal,
) -> crate::prelude::Result<()> {
	let payload = serde_json::json!({
		"schema": "decodex.phase_goal_signal/1",
		"phase": phase.as_str(),
		"signal": "goal_complete",
		"threadId": goal.thread_id,
		"status": goal.status.as_str(),
		"tokenBudget": goal.token_budget,
		"tokensUsed": goal.tokens_used,
		"timeUsedSeconds": goal.time_used_seconds,
	});

	record_phase_goal_private_event(recorder, "phase_goal_completed", phase, &payload)
}

fn record_phase_goal_private_event(
	recorder: &mut RunRecorder<'_>,
	event_type: &str,
	phase: PhaseGoalKind,
	payload: &Value,
) -> crate::prelude::Result<()> {
	recorder.state_store.append_private_execution_event(
		recorder.project_id(),
		recorder.issue_id(),
		recorder.run_id,
		recorder.attempt_number,
		event_type,
		serde_json::json!({
			"schema": "decodex.phase_goal_signal/1",
			"phase": phase.as_str(),
			"payload": payload,
		}),
	)?;

	Ok(())
}

fn app_server_method_not_found(error: &Report) -> bool {
	let text = error.to_string().to_lowercase();

	text.contains("-32601") || text.contains("method not found")
}

fn build_thread_start_request(
	request: &AppServerRunRequest<'_>,
) -> crate::prelude::Result<ThreadStartRequest> {
	let dynamic_tools = request
		.dynamic_tool_handler
		.map(validated_dynamic_tool_specs)
		.transpose()?
		.map(|tool_specs| self::protocol::app_server_dynamic_tool_specs(&tool_specs));

	Ok(ThreadStartRequest {
		cwd: Some(request.cwd.clone()),
		dynamic_tools,
		developer_instructions: Some(request.developer_instructions.clone()),
		ephemeral: request.ephemeral_thread.then_some(true),
		..ThreadStartRequest::default()
	})
}

fn validated_dynamic_tool_specs(
	handler: &dyn DynamicToolHandler,
) -> crate::prelude::Result<Vec<DynamicToolSpec>> {
	let tool_specs = handler.tool_specs();

	for spec in &tool_specs {
		if !tracker_tool_bridge::dynamic_tool_identifier_is_valid(&spec.name) {
			return Err(Report::new(AppServerDynamicToolFailure::protocol(
				Some(spec.name.clone()),
				format!(
					"Dynamic tool name `{}` does not match the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`.",
					spec.name
				),
			)));
		}

		if let Some(namespace) = spec.namespace.as_deref()
			&& !tracker_tool_bridge::dynamic_tool_identifier_is_valid(namespace)
		{
			return Err(Report::new(AppServerDynamicToolFailure::protocol(
				Some(format!("{namespace}.{}", spec.name)),
				format!(
					"Dynamic tool namespace `{namespace}` does not match the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`."
				),
			)));
		}
	}

	Ok(tool_specs)
}

fn build_thread_resume_request(
	resume_thread_id: &str,
	request: &AppServerRunRequest<'_>,
) -> ThreadResumeRequest {
	ThreadResumeRequest {
		thread_id: resume_thread_id.to_owned(),
		cwd: Some(request.cwd.clone()),
		developer_instructions: Some(request.developer_instructions.clone()),
		..ThreadResumeRequest::default()
	}
}

fn classify_turn_completion(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	final_output: &str,
) -> crate::prelude::Result<TurnCompletionStatus> {
	if let Some(dynamic_tool_handler) = dynamic_tool_handler {
		return dynamic_tool_handler.classify_turn_completion(final_output);
	}

	Ok(TurnCompletionStatus::Complete)
}

fn has_terminal_completion_signal(dynamic_tool_handler: Option<&dyn DynamicToolHandler>) -> bool {
	dynamic_tool_handler.is_some_and(DynamicToolHandler::has_terminal_completion_signal)
}

fn reject_nonterminal_single_turn_completion(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	final_output: &str,
) -> crate::prelude::Result<()> {
	if let Some(dynamic_tool_handler) = dynamic_tool_handler {
		dynamic_tool_handler.validate_turn_completion(final_output)?;
	}

	eyre::bail!(
		"Turn completed without a terminal completion path while same-thread continuation is disabled."
	);
}

fn continuation_boundary_reached(
	continuation_guard: Option<&dyn TurnContinuationGuard>,
	turn_count: u32,
) -> crate::prelude::Result<bool> {
	let Some(continuation_guard) = continuation_guard else {
		return Ok(false);
	};

	if continuation_guard.should_continue_turn(turn_count)? {
		return Ok(false);
	}

	continuation_guard.validate_continuation_boundary(turn_count)?;

	Ok(true)
}

fn build_turn_start_request(thread_id: &str, user_input: &str) -> TurnStartRequest {
	TurnStartRequest {
		thread_id: thread_id.to_owned(),
		input: vec![UserInput::Text { text: user_input.to_owned() }],
		..TurnStartRequest::default()
	}
}

fn build_turn_steer_request(
	thread_id: &str,
	expected_turn_id: &str,
	message: &str,
) -> TurnSteerRequest {
	TurnSteerRequest {
		thread_id: thread_id.to_owned(),
		expected_turn_id: expected_turn_id.to_owned(),
		input: vec![UserInput::Text { text: message.to_owned() }],
	}
}

fn login_account_params(account: &CodexAccountLogin) -> LoginAccountParams {
	LoginAccountParams::ChatgptAuthTokens {
		access_token: account.access_token().to_owned(),
		chatgpt_account_id: account.account_id().to_owned(),
		chatgpt_plan_type: account.plan_type().map(str::to_owned),
	}
}

fn record_codex_account_login(
	recorder: &mut RunRecorder<'_>,
	summary: &CodexAccountActivitySummary,
) -> crate::prelude::Result<()> {
	recorder.record(
		"account/login/start",
		&serde_json::json!({
			"type": "chatgptAuthTokens",
			"accountFingerprint": summary.account_fingerprint.as_str(),
			"planType": summary.plan_type.as_deref(),
			"status": summary.status.as_str(),
			"refreshStatus": summary.refresh_status.as_str(),
			"primaryRemainingPercent": summary.primary_remaining_percent,
			"secondaryRemainingPercent": summary.secondary_remaining_percent,
			"rateLimitReachedType": summary.rate_limit_reached_type.as_deref(),
		})
		.to_string(),
	)
}

fn flush_pending_messages(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	target_thread_id: Option<&str>,
) -> crate::prelude::Result<()> {
	for message in client.drain_pending() {
		if targets_thread(&message, target_thread_id) {
			recorder.record(message_type(&message), &message.raw)?;

			apply_protocol_message_side_effects(recorder, &message)?;
		}
	}

	Ok(())
}

fn wait_for_turn_completion(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	target_thread_id: &str,
	target_turn_id: &str,
) -> crate::prelude::Result<RunOutcome> {
	let control_enabled = request.activity_marker_path.is_some();
	let mut last_activity_at = Instant::now();
	let mut target_turn_id = target_turn_id.to_owned();
	let mut final_output = String::new();
	let mut latest_turn_failure: Option<AppServerTurnFailure> = None;

	loop {
		if control_enabled
			&& let Some(response_turn_id) = handle_pending_turn_control_requests(
				client,
				recorder,
				request,
				target_thread_id,
				&target_turn_id,
			)? {
			recorder.state_store.update_run_turn(recorder.run_id, &response_turn_id)?;
			recorder.set_turn_id(&response_turn_id)?;

			target_turn_id = response_turn_id;
			last_activity_at = Instant::now();
		}

		let idle_timeout = protocol_activity_idle_timeout(
			Some(&recorder.protocol_activity.summary),
			request.timeout,
		);
		let Some(wire_message) = next_turn_wire_message(
			client,
			last_activity_at,
			idle_timeout,
			target_thread_id,
			&target_turn_id,
			latest_turn_failure.as_ref(),
			control_enabled,
		)?
		else {
			continue;
		};

		if !targets_thread(&wire_message, Some(target_thread_id)) {
			tracing::debug!(raw = %wire_message.raw, "Ignoring app-server message for another thread.");

			continue;
		}

		last_activity_at = Instant::now();

		recorder.record(message_type(&wire_message), &wire_message.raw)?;

		apply_protocol_message_side_effects(recorder, &wire_message)?;

		match &wire_message.message {
			JsonRpcMessage::Notification(notification) => {
				adopt_thread_bound_notification_turn_id(
					recorder,
					notification,
					target_thread_id,
					&mut target_turn_id,
				)?;

				if let Some(outcome) = handle_turn_execution_notification(
					notification,
					target_thread_id,
					&target_turn_id,
					&mut final_output,
					&mut latest_turn_failure,
				)? {
					return Ok(outcome);
				}
			},
			JsonRpcMessage::Request(server_request) => handle_turn_execution_request(
				client,
				recorder,
				server_request,
				target_thread_id,
				&target_turn_id,
				request.dynamic_tool_handler,
				request.codex_account_provider,
			)?,
			JsonRpcMessage::Response(_) => ignore_orphan_turn_json_rpc_response(),
			JsonRpcMessage::Error(error) => {
				latest_turn_failure = Some(turn_failure_from_json_rpc_error_response(
					target_thread_id,
					&target_turn_id,
					error,
				));
			},
		}
	}
}

fn handle_turn_execution_notification(
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &str,
	final_output: &mut String,
	latest_turn_failure: &mut Option<AppServerTurnFailure>,
) -> crate::prelude::Result<Option<RunOutcome>> {
	match notification.method.as_str() {
		"thread/status/changed" => {
			let payload: ThreadStatusChangedNotification =
				serde_json::from_value(notification.params.clone())?;

			if payload.status.kind == "systemError" && latest_turn_failure.is_none() {
				*latest_turn_failure =
					Some(AppServerTurnFailure::from_system_error(&payload.thread_id));
			}
		},
		"error" => {
			if let Some((failure, will_retry)) =
				failure_from_error_notification(notification, target_thread_id, target_turn_id)?
			{
				if (failure.requires_operator_attention() || failure.should_stop_current_turn())
					&& will_retry != Some(true)
				{
					return Err(Report::new(failure));
				}

				*latest_turn_failure = Some(failure);
			}
		},
		"item/agentMessage/delta" => {
			if !notification_targets_turn(notification, target_turn_id) {
				return Ok(None);
			}

			let payload: AgentMessageDeltaNotification =
				serde_json::from_value(notification.params.clone())?;

			final_output.push_str(&payload.delta);
		},
		"item/completed" => {
			if !notification_targets_turn(notification, target_turn_id) {
				return Ok(None);
			}

			let payload: ItemCompletedNotification =
				serde_json::from_value(notification.params.clone())?;

			if payload.item.kind == "agentMessage"
				&& let Some(text) = payload.item.text
			{
				*final_output = text;
			}
		},
		"turn/completed" => {
			let payload: TurnCompletedNotification =
				serde_json::from_value(notification.params.clone())?;

			if payload.turn.id != target_turn_id {
				return Ok(None);
			}
			if payload.turn.status == "completed" {
				return Ok(Some(RunOutcome {
					final_output: mem::take(final_output),
					turn_id: target_turn_id.to_owned(),
				}));
			}

			if let Some(error) = payload.turn.error.as_ref() {
				return Err(Report::new(turn_failure_from_turn_error(
					target_thread_id,
					Some(&payload.turn.id),
					&payload.turn.status,
					error,
				)));
			}
			if let Some(failure) = latest_turn_failure.take() {
				return Err(Report::new(failure));
			}

			eyre::bail!(
				"Turn `{}` ended with status `{}` without an explicit error payload.",
				payload.turn.id,
				payload.turn.status
			);
		},
		"thread/goal/updated" => {
			let payload: ThreadGoalUpdatedNotification =
				serde_json::from_value(notification.params.clone())?;

			if payload.thread_id != target_thread_id
				|| payload.turn_id.as_deref().is_some_and(|turn_id| turn_id != target_turn_id)
			{
				return Ok(None);
			}

			let _status = payload.goal.status;
		},
		"thread/goal/cleared" => {},
		_ => {},
	}

	Ok(None)
}

fn adopt_thread_bound_notification_turn_id(
	recorder: &mut RunRecorder<'_>,
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &mut String,
) -> crate::prelude::Result<()> {
	let Some(observed_turn_id) = turn_id_from_value(&notification.params) else {
		return Ok(());
	};

	if observed_turn_id == target_turn_id {
		return Ok(());
	}
	if thread_id_from_notification(notification)
		.is_none_or(|thread_id| thread_id != target_thread_id)
	{
		return Ok(());
	}

	tracing::warn!(
		target_thread_id,
		previous_turn_id = target_turn_id.as_str(),
		observed_turn_id,
		method = notification.method.as_str(),
		"App-server notification turn id differed from the turn/start response; adopting thread-bound notification turn id."
	);

	recorder.state_store.update_run_turn(recorder.run_id, observed_turn_id)?;
	recorder.set_turn_id(observed_turn_id)?;

	*target_turn_id = observed_turn_id.to_owned();

	Ok(())
}

fn notification_targets_turn(notification: &JsonRpcNotification, target_turn_id: &str) -> bool {
	turn_id_from_value(&notification.params).is_none_or(|turn_id| turn_id == target_turn_id)
}

fn next_turn_wire_message(
	client: &mut AppServerClient,
	last_activity_at: Instant,
	timeout: Duration,
	target_thread_id: &str,
	target_turn_id: &str,
	latest_turn_failure: Option<&AppServerTurnFailure>,
	control_enabled: bool,
) -> crate::prelude::Result<Option<WireMessage>> {
	let now = Instant::now();
	let wait_timeout = remaining_idle_budget(last_activity_at, now, timeout).ok_or_else(|| {
		turn_wait_timeout_error(target_thread_id, target_turn_id, latest_turn_failure.cloned())
	})?;
	let recv_timeout =
		if control_enabled { wait_timeout.min(RUN_CONTROL_POLL_INTERVAL) } else { wait_timeout };

	match recv_turn_wire_message(client, recv_timeout, latest_turn_failure) {
		Ok(wire_message) => Ok(Some(wire_message)),
		Err(error)
			if control_enabled
				&& recv_timeout < wait_timeout
				&& is_app_server_output_timeout(&error) =>
			Ok(None),
		Err(error) => Err(error),
	}
}

fn handle_pending_turn_control_requests(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	target_thread_id: &str,
	target_turn_id: &str,
) -> crate::prelude::Result<Option<String>> {
	let Some(worktree_path) = request.activity_marker_path.as_deref() else {
		return Ok(None);
	};

	for pending in run_control::pending_interrupt_requests(worktree_path, &request.run_id)? {
		handle_pending_turn_interrupt_request(
			client,
			recorder,
			request,
			worktree_path,
			pending,
			target_thread_id,
			target_turn_id,
		)?;
	}
	for pending in run_control::pending_steer_requests(worktree_path, &request.run_id)? {
		if let Some(response_turn_id) = handle_pending_turn_steer_request(
			client,
			recorder,
			request,
			worktree_path,
			pending,
			target_thread_id,
			target_turn_id,
		)? {
			return Ok(Some(response_turn_id));
		}
	}

	Ok(None)
}

fn handle_pending_turn_interrupt_request(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	run_request: &AppServerRunRequest<'_>,
	worktree_path: &Path,
	pending: PendingLaneControlRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> crate::prelude::Result<()> {
	record_lane_interrupt_request(recorder, &pending.request)?;

	if let Some((error_class, message)) = lane_interrupt_request_rejection(
		run_request,
		&pending.request,
		target_thread_id,
		target_turn_id,
	) {
		let response =
			LaneControlInterruptResponse::rejected(&pending.request, error_class, message);

		record_lane_interrupt_response(recorder, &response)?;

		run_control::write_interrupt_response(worktree_path, &response)?;
		run_control::remove_interrupt_request(&pending.path)?;

		return Ok(());
	}

	let interrupt = TurnInterruptRequest {
		thread_id: pending.request.thread_id.clone(),
		turn_id: pending.request.turn_id.clone(),
	};
	let result = client.interrupt_turn_with_handler(
		interrupt,
		|connection, wire_message, server_request| {
			handle_server_request_while_waiting(
				connection,
				recorder,
				wire_message,
				server_request,
				RequestDispatchContext::new(
					RequestWaitPhase::TurnExecution,
					run_request.dynamic_tool_handler,
					run_request.codex_account_provider,
					Some(target_thread_id),
					Some(target_turn_id),
				),
			)
		},
	);
	let response = match result {
		Ok(value) => LaneControlInterruptResponse::delivered(
			&pending.request,
			run_control::protocol_response_summary(&value),
		),
		Err(error) => LaneControlInterruptResponse::failed(
			&pending.request,
			soft_interrupt_error_class(&error),
			format!("turn/interrupt failed with {}.", soft_interrupt_error_class(&error)),
		),
	};

	record_lane_interrupt_response(recorder, &response)?;

	run_control::write_interrupt_response(worktree_path, &response)?;
	run_control::remove_interrupt_request(&pending.path)?;

	Ok(())
}

fn handle_pending_turn_steer_request(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	run_request: &AppServerRunRequest<'_>,
	worktree_path: &Path,
	pending: PendingLaneControlSteerRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> crate::prelude::Result<Option<String>> {
	record_lane_steer_request(recorder, &pending.request)?;

	if let Some((error_class, message)) = lane_steer_request_rejection(
		run_request,
		&pending.request,
		target_thread_id,
		target_turn_id,
	) {
		let response = LaneControlSteerResponse::rejected(
			&pending.request,
			target_turn_id,
			error_class,
			message,
		);

		record_lane_steer_response(recorder, &response, Some(pending.request.audit_record_id))?;

		run_control::write_steer_response(worktree_path, &response)?;
		run_control::remove_steer_request(&pending.path)?;

		return Ok(None);
	}

	let result = client.steer_turn_with_handler(
		build_turn_steer_request(
			&pending.request.thread_id,
			&pending.request.expected_turn_id,
			&pending.request.message,
		),
		|connection, wire_message, server_request| {
			handle_server_request_while_waiting(
				connection,
				recorder,
				wire_message,
				server_request,
				RequestDispatchContext::new(
					RequestWaitPhase::TurnExecution,
					run_request.dynamic_tool_handler,
					run_request.codex_account_provider,
					Some(target_thread_id),
					None,
				),
			)
		},
	);
	let response = match result {
		Ok(value) =>
			LaneControlSteerResponse::delivered(&pending.request, target_turn_id, &value.turn_id),
		Err(error) => {
			let error_class = steer_error_class(&error);

			LaneControlSteerResponse::failed(
				&pending.request,
				target_turn_id,
				error_class,
				format!("turn/steer failed with {error_class}."),
			)
		},
	};
	let response_turn_id = response.response_turn_id.clone();

	record_lane_steer_response(recorder, &response, Some(pending.request.audit_record_id))?;

	run_control::write_steer_response(worktree_path, &response)?;
	run_control::remove_steer_request(&pending.path)?;

	Ok(response_turn_id)
}

fn record_lane_interrupt_request(
	recorder: &mut RunRecorder<'_>,
	request: &LaneControlInterruptRequest,
) -> crate::prelude::Result<()> {
	recorder.record(
		"lane_control/interrupt/request",
		&serde_json::json!({
			"requestId": request.request_id,
			"projectId": request.project_id,
			"issueId": request.issue_id,
			"runId": request.run_id,
			"attemptNumber": request.attempt_number,
			"threadId": request.thread_id,
			"turnId": request.turn_id,
			"source": request.source,
			"reason": request.reason,
		})
		.to_string(),
	)
}

fn record_lane_interrupt_response(
	recorder: &mut RunRecorder<'_>,
	response: &LaneControlInterruptResponse,
) -> crate::prelude::Result<()> {
	recorder.record(
		"lane_control/interrupt/response",
		&serde_json::json!({
			"requestId": response.request_id,
			"projectId": response.project_id,
			"issueId": response.issue_id,
			"runId": response.run_id,
			"attemptNumber": response.attempt_number,
			"threadId": response.thread_id,
			"turnId": response.turn_id,
			"status": response.status,
			"classification": response.classification,
			"method": response.method,
			"errorClass": response.error_class,
			"protocolSummary": response.protocol_summary,
		})
		.to_string(),
	)?;
	recorder.state_store.append_private_execution_event(
		&response.project_id,
		&response.issue_id,
		&response.run_id,
		response.attempt_number,
		"lane_control/interrupt",
		serde_json::json!({
			"requestId": response.request_id,
			"status": response.status,
			"classification": response.classification,
			"method": response.method,
			"errorClass": response.error_class,
			"protocolSummary": response.protocol_summary,
			"message": response.message,
		}),
	)?;

	Ok(())
}

fn record_lane_steer_request(
	recorder: &mut RunRecorder<'_>,
	request: &LaneControlSteerRequest,
) -> crate::prelude::Result<()> {
	recorder.record(
		"lane_control/steer/request",
		&serde_json::json!({
			"requestId": request.request_id,
			"auditRecordId": request.audit_record_id,
			"projectId": request.project_id,
			"issueId": request.issue_id,
			"runId": request.run_id,
			"attemptNumber": request.attempt_number,
			"threadId": request.thread_id,
			"expectedTurnId": request.expected_turn_id,
			"source": request.source,
			"messageByteCount": request.message_byte_count,
			"messageLineCount": request.message_line_count,
		})
		.to_string(),
	)
}

fn record_lane_steer_response(
	recorder: &mut RunRecorder<'_>,
	response: &LaneControlSteerResponse,
	parent_record_id: Option<i64>,
) -> crate::prelude::Result<()> {
	let outcome = match &response.status {
		LaneControlSteerResponseStatus::Delivered => RUN_CONTROL_ACTION_COMPLETED,
		LaneControlSteerResponseStatus::Failed | LaneControlSteerResponseStatus::Rejected =>
			RUN_CONTROL_ACTION_FAILED,
	};
	let metadata = serde_json::json!({
		"requestId": response.request_id,
		"outcome": outcome,
		"reason": response.classification,
		"failureClass": response.error_class,
		"expectedTurnId": response.expected_turn_id,
		"currentTurnId": response.current_turn_id,
		"responseTurnId": response.response_turn_id,
	});

	recorder.record("turn/steer", &metadata.to_string())?;
	recorder.record(
		"lane_control/steer/response",
		&serde_json::json!({
			"requestId": response.request_id,
			"projectId": response.project_id,
			"issueId": response.issue_id,
			"runId": response.run_id,
			"attemptNumber": response.attempt_number,
			"threadId": response.thread_id,
			"expectedTurnId": response.expected_turn_id,
			"currentTurnId": response.current_turn_id,
			"responseTurnId": response.response_turn_id,
			"status": &response.status,
			"classification": response.classification,
			"method": response.method,
			"errorClass": response.error_class,
		})
		.to_string(),
	)?;
	recorder.state_store.record_run_control_action_delivery_outcome(
		RunControlActionOutcomeRequest {
			project_id: &response.project_id,
			issue_id: &response.issue_id,
			run_id: &response.run_id,
			attempt_number: response.attempt_number,
			thread_id: Some(&response.thread_id),
			turn_id: Some(&response.expected_turn_id),
			current_thread_id: Some(&response.thread_id),
			current_turn_id: response.current_turn_id.as_deref(),
			source: "app_server_child",
			action: "steer",
			outcome,
			reason: &response.classification,
			parent_record_id,
			timeout_ms: None,
			metadata: Some(&metadata),
			channel: None,
		},
	)?;
	recorder.state_store.append_private_execution_event(
		&response.project_id,
		&response.issue_id,
		&response.run_id,
		response.attempt_number,
		"lane_control/steer",
		serde_json::json!({
			"requestId": response.request_id,
			"status": &response.status,
			"classification": response.classification,
			"method": response.method,
			"errorClass": response.error_class,
			"expectedTurnId": response.expected_turn_id,
			"currentTurnId": response.current_turn_id,
			"responseTurnId": response.response_turn_id,
			"message": response.message,
		}),
	)?;

	Ok(())
}

fn lane_interrupt_request_rejection(
	run_request: &AppServerRunRequest<'_>,
	request: &LaneControlInterruptRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Option<(&'static str, String)> {
	if request.project_id != run_request.project_id {
		return Some((
			"project_mismatch",
			format!(
				"Control request targeted project `{}`, but this run belongs to `{}`.",
				request.project_id, run_request.project_id
			),
		));
	}
	if request.issue_id != run_request.issue_id {
		return Some((
			"issue_mismatch",
			format!(
				"Control request targeted issue `{}`, but this run belongs to `{}`.",
				request.issue_id, run_request.issue_id
			),
		));
	}
	if request.run_id != run_request.run_id {
		return Some((
			"run_mismatch",
			format!(
				"Control request targeted run `{}`, but this run is `{}`.",
				request.run_id, run_request.run_id
			),
		));
	}
	if request.attempt_number != run_request.attempt_number {
		return Some((
			"attempt_mismatch",
			format!(
				"Control request targeted attempt `{}`, but this run is attempt `{}`.",
				request.attempt_number, run_request.attempt_number
			),
		));
	}
	if request.thread_id != target_thread_id {
		return Some((
			"thread_mismatch",
			format!(
				"Control request targeted thread `{}`, but the active thread is `{target_thread_id}`.",
				request.thread_id
			),
		));
	}
	if request.turn_id != target_turn_id {
		return Some((
			"turn_mismatch",
			format!(
				"Control request targeted turn `{}`, but the active turn is `{target_turn_id}`.",
				request.turn_id
			),
		));
	}

	None
}

fn lane_steer_request_rejection(
	run_request: &AppServerRunRequest<'_>,
	request: &LaneControlSteerRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Option<(&'static str, String)> {
	if request.project_id != run_request.project_id {
		return Some((
			"project_mismatch",
			format!(
				"Control request targeted project `{}`, but this run belongs to `{}`.",
				request.project_id, run_request.project_id
			),
		));
	}
	if request.issue_id != run_request.issue_id {
		return Some((
			"issue_mismatch",
			format!(
				"Control request targeted issue `{}`, but this run belongs to `{}`.",
				request.issue_id, run_request.issue_id
			),
		));
	}
	if request.run_id != run_request.run_id {
		return Some((
			"run_mismatch",
			format!(
				"Control request targeted run `{}`, but this run is `{}`.",
				request.run_id, run_request.run_id
			),
		));
	}
	if request.attempt_number != run_request.attempt_number {
		return Some((
			"attempt_mismatch",
			format!(
				"Control request targeted attempt `{}`, but this run is attempt `{}`.",
				request.attempt_number, run_request.attempt_number
			),
		));
	}
	if request.thread_id != target_thread_id {
		return Some((
			"thread_mismatch",
			format!(
				"Control request targeted thread `{}`, but the active thread is `{target_thread_id}`.",
				request.thread_id
			),
		));
	}
	if request.expected_turn_id != target_turn_id {
		return Some((
			"stale_expected_turn_id",
			format!(
				"Control request expected turn `{}`, but the active turn is `{target_turn_id}`.",
				request.expected_turn_id
			),
		));
	}

	None
}

fn soft_interrupt_error_class(error: &Report) -> &'static str {
	if is_app_server_output_timeout(error) {
		return "soft_interrupt_timed_out";
	}

	let error_text = error.to_string().to_ascii_lowercase();

	if error_text.contains("-32601") || error_text.contains("method not found") {
		"soft_interrupt_unsupported"
	} else {
		"soft_interrupt_failed"
	}
}

fn steer_error_class(error: &Report) -> &'static str {
	if is_app_server_output_timeout(error) {
		return "app_server_turn_steer_timed_out";
	}

	let error_text = error.to_string().to_ascii_lowercase();

	if error_text.contains("activeturnnotsteerable")
		|| error_text.contains("active turn not steerable")
	{
		return "active_turn_not_steerable";
	}
	if error_text.contains("-32601") || error_text.contains("method not found") {
		return "app_server_turn_steer_unsupported";
	}

	"app_server_turn_steer_failed"
}

fn is_app_server_output_timeout(error: &Report) -> bool {
	error.downcast_ref::<AppServerOutputTimeout>().is_some()
}

fn handle_turn_execution_request(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: &str,
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	codex_account_provider: Option<&dyn CodexAccountProvider>,
) -> crate::prelude::Result<()> {
	handle_server_request_during_turn_execution(
		client,
		recorder,
		request,
		RequestDispatchContext::new(
			RequestWaitPhase::TurnExecution,
			dynamic_tool_handler,
			codex_account_provider,
			Some(target_thread_id),
			Some(target_turn_id),
		),
	)
}

fn ignore_orphan_turn_json_rpc_response() {
	tracing::debug!(
		"Recorded and ignored orphan app-server JSON-RPC response while waiting for turn completion."
	);
}

fn turn_wait_timeout_error(
	target_thread_id: &str,
	target_turn_id: &str,
	latest_turn_failure: Option<AppServerTurnFailure>,
) -> Report {
	let message = format!(
		"Timed out while waiting for turn `{target_turn_id}` on thread `{target_thread_id}`."
	);

	if let Some(failure) = latest_turn_failure {
		return Report::new(failure).wrap_err(message);
	}

	eyre::eyre!(message)
}

fn recv_turn_wire_message(
	client: &mut AppServerClient,
	wait_timeout: Duration,
	latest_turn_failure: Option<&AppServerTurnFailure>,
) -> crate::prelude::Result<WireMessage> {
	match annotate_transport_failure_phase(
		client.recv(Some(wait_timeout)),
		RequestWaitPhase::TurnExecution,
	) {
		Ok(wire_message) => Ok(wire_message),
		Err(error) => {
			if error.downcast_ref::<AppServerOutputTimeout>().is_some()
				&& let Some(failure) = latest_turn_failure
			{
				return Err(Report::new(failure.clone())
					.wrap_err("Timed out while waiting for additional app-server output."));
			}

			Err(error)
		},
	}
}

fn failure_from_error_notification(
	notification: &JsonRpcNotification,
	target_thread_id: &str,
	target_turn_id: &str,
) -> crate::prelude::Result<Option<(AppServerTurnFailure, Option<bool>)>> {
	let payload: ErrorNotification = serde_json::from_value(notification.params.clone())?;
	let payload_turn_matches =
		payload.turn_id.as_deref().is_none_or(|turn_id| turn_id == target_turn_id);
	let payload_thread_matches =
		payload.thread_id.as_deref().is_none_or(|thread_id| thread_id == target_thread_id);

	if !payload_thread_matches || !payload_turn_matches {
		return Ok(None);
	}

	let failure = turn_failure_from_turn_error(
		target_thread_id,
		payload.turn_id.as_deref(),
		"failed",
		&payload.error,
	);

	Ok(Some((failure, payload.will_retry)))
}

fn turn_failure_from_turn_error(
	thread_id: &str,
	turn_id: Option<&str>,
	status: &str,
	error: &TurnError,
) -> AppServerTurnFailure {
	AppServerTurnFailure::new(
		thread_id,
		turn_id.map(str::to_owned),
		status,
		error.message.clone(),
		error.codex_error_info.clone(),
	)
}

fn turn_failure_from_json_rpc_error_response(
	thread_id: &str,
	turn_id: &str,
	error: &JsonRpcError,
) -> AppServerTurnFailure {
	tracing::warn!(
		id = %error.id,
		code = error.error.code,
		message = %error.error.message,
		"Received JSON-RPC error response while waiting for turn completion."
	);

	AppServerTurnFailure::new(
		thread_id,
		Some(turn_id.to_owned()),
		"failed",
		format!(
			"app-server JSON-RPC error response while waiting for turn completion: code {}: {}",
			error.error.code, error.error.message
		),
		None,
	)
}

fn remaining_idle_budget(
	last_activity_at: Instant,
	now: Instant,
	timeout: Duration,
) -> Option<Duration> {
	timeout.checked_sub(now.saturating_duration_since(last_activity_at))
}

fn handle_server_request_while_waiting(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	wire_message: &WireMessage,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	if targets_thread(wire_message, context.target_thread_id) {
		record_wire_message_safely(recorder, wire_message)?;
		record_interactive_request_state(recorder, request)?;
	} else if request.method == "account/chatgptAuthTokens/refresh" {
		record_codex_account_refresh_request(recorder, request)?;
	}

	dispatch_server_request(connection, recorder, request, context)
}

fn handle_server_request_during_turn_execution(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	record_server_request_safely(recorder, request)?;
	record_interactive_request_state(recorder, request)?;

	dispatch_server_request(&mut client.connection, recorder, request, context)
}

fn dispatch_server_request(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	match request.method.as_str() {
		"item/tool/call" if context.phase == RequestWaitPhase::TurnExecution =>
			dispatch_dynamic_tool_call(connection, recorder, request, context),
		"account/chatgptAuthTokens/refresh" =>
			dispatch_codex_account_refresh(connection, recorder, request, context),
		"item/tool/call" => respond_to_dynamic_tool_call_dispatch(
			connection,
			recorder,
			request,
			dynamic_tool_call_unavailable_for_phase(context.phase),
		),
		"item/commandExecution/requestApproval" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/commandExecution/requestApproval/response",
			&CommandExecutionRequestApprovalResponse {
				decision: CommandExecutionApprovalDecision::Decline,
			},
		),
		"item/fileChange/requestApproval" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/fileChange/requestApproval/response",
			&FileChangeRequestApprovalResponse { decision: FileChangeApprovalDecision::Decline },
		),
		"item/tool/requestUserInput" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/tool/requestUserInput/response",
			&ToolRequestUserInputResponse::default(),
		),
		"item/permissions/requestApproval" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/permissions/requestApproval/response",
			&PermissionsRequestApprovalResponse {
				permissions: Default::default(),
				scope: PermissionGrantScope::Turn,
			},
		),
		"mcpServer/elicitation/request" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"mcpServer/elicitation/request/response",
			&McpServerElicitationRequestResponse {
				action: McpServerElicitationAction::Decline,
				content: None,
				meta: None,
			},
		),
		other =>
			reject_unsupported_server_request(connection, recorder, request, context.phase, other),
	}
}

fn record_server_request(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> crate::prelude::Result<()> {
	recorder.record(
		request.method.as_str(),
		&serde_json::json!({
			"id": request.id.clone(),
			"method": request.method.clone(),
			"params": request.params.clone(),
		})
		.to_string(),
	)
}

fn record_server_request_safely(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> crate::prelude::Result<()> {
	if request.method == "account/chatgptAuthTokens/refresh" {
		return record_codex_account_refresh_request(recorder, request);
	}

	record_server_request(recorder, request)
}

fn record_wire_message_safely(
	recorder: &mut RunRecorder<'_>,
	wire_message: &WireMessage,
) -> crate::prelude::Result<()> {
	match &wire_message.message {
		JsonRpcMessage::Request(request)
			if request.method == "account/chatgptAuthTokens/refresh" =>
			record_codex_account_refresh_request(recorder, request),
		_ => recorder.record(message_type(wire_message), &wire_message.raw),
	}
}

fn record_codex_account_refresh_request(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> crate::prelude::Result<()> {
	let params = serde_json::from_value::<ChatgptAuthTokensRefreshParams>(request.params.clone())
		.unwrap_or(ChatgptAuthTokensRefreshParams { reason: None, previous_account_id: None });

	recorder.record(
		"account/chatgptAuthTokens/refresh",
		&serde_json::json!({
			"id": request.id.clone(),
			"method": request.method.as_str(),
			"reason": params.reason.as_deref(),
			"previousAccountFingerprint": params.previous_account_id.as_deref().map(redact_identifier),
		})
		.to_string(),
	)
}

fn dispatch_codex_account_refresh(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	let account_provider = context.codex_account_provider.ok_or_else(|| {
		eyre::eyre!(
			"app_server_protocol_failure: received `account/chatgptAuthTokens/refresh` without a configured Codex account provider."
		)
	})?;
	let params = serde_json::from_value::<ChatgptAuthTokensRefreshParams>(request.params.clone())?;
	let account = match account_provider.refresh_account(params.previous_account_id.as_deref()) {
		Ok(account) => account,
		Err(error) => {
			record_codex_account_failure(
				recorder,
				"account/chatgptAuthTokens/refresh/failed",
				&error,
			);

			return Err(error);
		},
	};
	let response = ChatgptAuthTokensRefreshResponse {
		access_token: account.access_token().to_owned(),
		chatgpt_account_id: account.account_id().to_owned(),
		chatgpt_plan_type: account.plan_type().map(str::to_owned),
	};

	recorder.set_codex_account(account.summary(), account.account_summaries())?;
	connection.respond(&request.id, &response)?;

	recorder.record(
		"account/chatgptAuthTokens/refresh/response",
		&serde_json::json!({
			"type": "chatgptAuthTokens",
			"accountFingerprint": account.summary().account_fingerprint.as_str(),
			"planType": account.summary().plan_type.as_deref(),
			"refreshStatus": account.summary().refresh_status.as_str(),
			"primaryRemainingPercent": account.summary().primary_remaining_percent,
			"secondaryRemainingPercent": account.summary().secondary_remaining_percent,
		})
		.to_string(),
	)
}

fn dispatch_dynamic_tool_call(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	let target_thread_id = context.target_thread_id.ok_or_else(|| {
		eyre::eyre!("app_server_protocol_failure: turn execution request missing thread context")
	})?;
	let dispatch = handle_dynamic_tool_call(
		context.dynamic_tool_handler,
		request,
		target_thread_id,
		context.target_turn_id,
	);

	respond_to_dynamic_tool_call_dispatch(connection, recorder, request, dispatch)
}

fn dynamic_tool_call_unavailable_for_phase(phase: RequestWaitPhase) -> DynamicToolCallDispatch {
	DynamicToolCallDispatch::protocol_failure(
		None,
		None,
		format!("Dynamic tool calls are unavailable while waiting for {}.", phase.label()),
	)
}

fn respond_to_dynamic_tool_call_dispatch(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	dispatch: DynamicToolCallDispatch,
) -> crate::prelude::Result<()> {
	record_server_request_response(
		connection,
		recorder,
		request,
		"item/tool/call/response",
		&dispatch.response,
	)?;

	if let Some(diagnostic) = dispatch.diagnostic.as_ref() {
		tracing::warn!(
			failure_class = diagnostic.failure_class,
			tool = diagnostic.tool.as_deref().unwrap_or("unknown"),
			next_action = diagnostic.next_action,
			message = diagnostic.message,
			"Dynamic tool call failed."
		);

		recorder.record("item/tool/call/failure", &serde_json::to_string(diagnostic)?)?;
	}
	if let Some(terminal_failure) = dispatch.terminal_failure {
		return Err(Report::new(terminal_failure));
	}

	Ok(())
}

fn reject_unsupported_server_request(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	phase: RequestWaitPhase,
	method: &str,
) -> crate::prelude::Result<()> {
	let message = format!("unsupported non-interactive server request `{method}`");

	connection.respond_error(&request.id, JSONRPC_METHOD_NOT_FOUND, &message)?;
	recorder.record(
		"json-rpc/error/response",
		&serde_json::json!({
			"code": JSONRPC_METHOD_NOT_FOUND,
			"message": message,
		})
		.to_string(),
	)?;

	eyre::bail!(
		"app_server_protocol_failure: unsupported server request `{method}` while waiting for {}.",
		phase.label()
	);
}

fn record_server_request_response<T>(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	event_type: &str,
	response: &T,
) -> crate::prelude::Result<()>
where
	T: Serialize,
{
	connection.respond(&request.id, response)?;

	recorder.record(event_type, &serde_json::to_string(response)?)
}

fn reject_interactive_server_request<T>(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	phase: RequestWaitPhase,
	event_type: &str,
	response: &T,
) -> crate::prelude::Result<()>
where
	T: Serialize,
{
	record_server_request_response(connection, recorder, request, event_type, response)?;

	Err(noninteractive_interaction_required(request.method.as_str(), phase))
}

fn noninteractive_interaction_required(method: &str, phase: RequestWaitPhase) -> Report {
	eyre::eyre!(
		"noninteractive_interaction_required: server request `{method}` requires interactive handling during {}.",
		phase.label()
	)
}

fn record_interactive_request_state(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> crate::prelude::Result<()> {
	let Some(flag) = interactive_flag_for_request(request.method.as_str()) else {
		return Ok(());
	};

	if let Some(thread_id) = thread_id_from_value(&request.params) {
		recorder.set_thread_id(thread_id)?;
	}
	if let Some(turn_id) = turn_id_from_value(&request.params) {
		recorder.set_turn_id(turn_id)?;
	}

	recorder.set_thread_status("active", &[flag.to_owned()])
}

fn interactive_flag_for_request(method: &str) -> Option<&'static str> {
	match method {
		"item/tool/requestUserInput" => Some("waitingOnUserInput"),
		"item/commandExecution/requestApproval"
		| "item/fileChange/requestApproval"
		| "item/permissions/requestApproval"
		| "mcpServer/elicitation/request" => Some("waitingOnApproval"),
		_ => None,
	}
}

fn apply_protocol_message_side_effects(
	recorder: &mut RunRecorder<'_>,
	message: &WireMessage,
) -> crate::prelude::Result<()> {
	match &message.message {
		JsonRpcMessage::Notification(notification)
			if notification.method == "thread/status/changed" =>
		{
			let payload: ThreadStatusChangedNotification =
				serde_json::from_value(notification.params.clone())?;

			if recorder.thread_id.is_none() {
				recorder.set_thread_id(&payload.thread_id)?;
			}

			recorder.set_thread_status(&payload.status.kind, &payload.status.active_flags)?;
		},
		_ => {},
	}

	Ok(())
}

fn validate_effective_thread_config(
	cwd: &str,
	runtime: &EffectiveThreadConfig,
) -> crate::prelude::Result<()> {
	if runtime.cwd != cwd {
		eyre::bail!(
			"app_server_protocol_failure: effective cwd `{}` did not match requested worktree `{cwd}`.",
			runtime.cwd
		);
	}
	if runtime.approval_policy != "never" {
		eyre::bail!(
			"app_server_protocol_failure: effective approval policy `{}` is interactive; Decodex requires `never`.",
			runtime.approval_policy
		);
	}
	if runtime.sandbox_mode == "readOnly" {
		eyre::bail!(
			"app_server_protocol_failure: effective sandbox mode `readOnly` does not allow Decodex execution."
		);
	}

	Ok(())
}

fn validate_initialize_codex_home(
	expected: &ResolvedAppServerCodexHomeEnv,
	response: &InitializeResponse,
) -> crate::prelude::Result<()> {
	let expected_home = normalized_home_path(expected.codex_home());
	let resolved_home = normalized_home_path(Path::new(&response.codex_home));

	if resolved_home != expected_home {
		tracing::warn!(
			expected_codex_home = %expected.codex_home().display(),
			resolved_codex_home = %response.codex_home,
			"Codex app-server resolved an unexpected Codex home."
		);

		return Err(Report::new(AppServerHomePreflightFailure::initialize_mismatch(
			response.codex_home.clone(),
			expected.codex_home().display().to_string(),
		)));
	}

	Ok(())
}

fn normalized_home_path(path: &Path) -> PathBuf {
	path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn thread_resume_error_allows_fallback(error: &Report) -> bool {
	let message = error.to_string().to_lowercase();

	thread_missing_error_message_allows_discard(&message)
}

fn thread_missing_error_message_allows_discard(message: &str) -> bool {
	message.contains("no rollout found for thread id") || message.contains("thread not found")
}

fn handle_dynamic_tool_call(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: Option<&str>,
) -> DynamicToolCallDispatch {
	let payload =
		match validated_dynamic_tool_call_payload(request, target_thread_id, target_turn_id) {
			Ok(payload) => payload,
			Err(dispatch) => return *dispatch,
		};
	let Some(dynamic_tool_handler) = dynamic_tool_handler else {
		return DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from("Dynamic tool bridge is unavailable for this run attempt."),
		);
	};
	let tool_specs = dynamic_tool_handler.tool_specs();
	let spec_matches_namespace = tool_specs.iter().any(|spec| {
		spec.name == payload.tool && spec.namespace.as_deref() == payload.namespace.as_deref()
	});

	if !spec_matches_namespace {
		let message = match payload.namespace.as_deref() {
			Some(namespace) => format!(
				"Dynamic tool `{}` was called under namespace `{namespace}`, but this run did not declare that tool namespace.",
				payload.tool
			),
			None => {
				format!("Dynamic tool `{}` is not declared for this run attempt.", payload.tool)
			},
		};

		return DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			message,
		);
	}

	let response = dynamic_tool_handler.handle_call_with_namespace(
		payload.namespace.as_deref(),
		&payload.tool,
		payload.arguments,
	);

	if let Err(message) = validate_dynamic_tool_call_response(&response) {
		return DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			message,
		);
	}

	if !response.success {
		return DynamicToolCallDispatch::tool_failure(
			response,
			Some(payload.tool),
			payload.namespace,
		);
	}

	DynamicToolCallDispatch::success(response)
}

fn validated_dynamic_tool_call_payload(
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: Option<&str>,
) -> std::result::Result<DynamicToolCallParams, Box<DynamicToolCallDispatch>> {
	let payload = serde_json::from_value::<DynamicToolCallParams>(request.params.clone()).map_err(
		|error| {
			Box::new(DynamicToolCallDispatch::protocol_failure(
				None,
				None,
				format!("Invalid `item/tool/call` payload: {error}"),
			))
		},
	)?;

	if payload.call_id.trim().is_empty() {
		return Err(Box::new(DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from("Dynamic tool call payload included an empty `callId`."),
		)));
	}
	if !tracker_tool_bridge::dynamic_tool_identifier_is_valid(&payload.tool) {
		return Err(Box::new(DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from(
				"Dynamic tool call payload included a tool name outside the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`.",
			),
		)));
	}

	if let Some(namespace) = payload.namespace.as_deref()
		&& !tracker_tool_bridge::dynamic_tool_identifier_is_valid(namespace)
	{
		return Err(Box::new(DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from(
				"Dynamic tool call payload included a namespace outside the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`.",
			),
		)));
	}

	if payload.thread_id != target_thread_id {
		return Err(Box::new(DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			format!(
				"Dynamic tool call targeted thread `{}`, but the active thread is `{target_thread_id}`.",
				payload.thread_id
			),
		)));
	}

	if let Some(target_turn_id) = target_turn_id
		&& payload.turn_id != target_turn_id
	{
		tracing::warn!(
			target_thread_id,
			target_turn_id,
			payload_thread_id = payload.thread_id.as_str(),
			payload_turn_id = payload.turn_id.as_str(),
			tool = payload.tool.as_str(),
			namespace = payload.namespace.as_deref().unwrap_or(""),
			"Dynamic tool call turn id differed from the active turn; accepting thread-bound request."
		);
	}

	Ok(payload)
}

fn validate_dynamic_tool_call_response(response: &DynamicToolCallResponse) -> Result<(), String> {
	if response.content_items.is_empty() {
		return Err(String::from(
			"Dynamic tool handler returned an invalid response with no `contentItems`.",
		));
	}

	Ok(())
}

fn dynamic_tool_response_text(response: &DynamicToolCallResponse) -> String {
	let text_items = response
		.content_items
		.iter()
		.map(|item| match item {
			DynamicToolContentItem::InputText { text } => text.trim(),
		})
		.filter(|text| !text.is_empty())
		.collect::<Vec<_>>();

	if text_items.is_empty() {
		String::from("Dynamic tool call failed without a text response.")
	} else {
		text_items.join("\n")
	}
}

fn message_type(message: &WireMessage) -> &str {
	match &message.message {
		JsonRpcMessage::Notification(notification) => notification.method.as_str(),
		JsonRpcMessage::Request(request) => request.method.as_str(),
		JsonRpcMessage::Response(_) => "json-rpc/response",
		JsonRpcMessage::Error(_) => "json-rpc/error",
	}
}

fn targets_thread(message: &WireMessage, target_thread_id: Option<&str>) -> bool {
	let Some(target_thread_id) = target_thread_id else {
		return true;
	};

	match &message.message {
		JsonRpcMessage::Notification(notification) => thread_id_from_notification(notification)
			.is_none_or(|thread_id| thread_id == target_thread_id),
		JsonRpcMessage::Request(request) => thread_id_from_value(&request.params)
			.is_none_or(|thread_id| thread_id == target_thread_id),
		JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => true,
	}
}

fn thread_id_from_notification(notification: &JsonRpcNotification) -> Option<&str> {
	thread_id_from_value(&notification.params)
}

fn thread_id_from_value(value: &Value) -> Option<&str> {
	value
		.get("threadId")
		.and_then(Value::as_str)
		.or_else(|| value.get("thread").and_then(|thread| thread.get("id")).and_then(Value::as_str))
}

fn turn_id_from_value(value: &Value) -> Option<&str> {
	value
		.get("turnId")
		.and_then(Value::as_str)
		.or_else(|| value.get("turn").and_then(|turn| turn.get("id")).and_then(Value::as_str))
}

#[cfg(test)]
mod tests;
