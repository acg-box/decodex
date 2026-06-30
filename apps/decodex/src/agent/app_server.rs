mod activity;
mod dynamic_tools;
mod lane_control;
mod phase_goal;
mod preflight;
mod protocol;
mod schema_probe;
mod server_requests;
mod turn_failure;

pub(crate) use turn_failure::AppServerTurnFailure;

#[cfg(test)] use self::dynamic_tools::handle_dynamic_tool_call;
#[cfg(test)] use self::lane_control::steer_error_class;
#[cfg(test)]
use self::preflight::{
	AppServerCapabilityPreflightStatus, build_command_exec_health_check_params,
	mcp_preflight_can_degrade, plugin_list_params_for_preflight, preflight_request,
	preflight_request_with_timeout_retry, record_config_preflight, record_mcp_preflight,
	record_mcp_preflight_degraded, record_model_preflight, record_model_provider_preflight,
	record_plugin_preflight, record_skills_preflight, validate_command_exec_health_check_result,
};
#[cfg(test)]
use self::schema_probe::{
	APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS, APP_SERVER_REQUIRED_CLIENT_REQUESTS,
	APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS, APP_SERVER_REQUIRED_SERVER_REQUESTS,
	APP_SERVER_SCHEMA_REQUIRED_MARKERS, validate_generated_app_server_schema,
};
#[cfg(test)]
use self::server_requests::{record_interactive_request_state, record_server_request};
pub(crate) use self::{
	activity::protocol_activity_idle_timeout,
	dynamic_tools::AppServerDynamicToolFailure,
	phase_goal::{
		AppServerPhaseGoalFailure, PhaseGoalController, PhaseGoalKind, PhaseGoalRunStatus,
		PhaseGoalSpec, PhaseGoalTransition,
	},
	preflight::{
		AppServerCapabilityPreflightFailure, AppServerCapabilityPreflightReport,
		CommandExecHealthCheck,
	},
};
use self::{
	activity::{ChildActivityAccumulator, ProtocolActivityAccumulator, redact_identifier},
	dynamic_tools::{
		classify_turn_completion, dispatch_dynamic_tool_call,
		dynamic_tool_call_unavailable_for_phase, has_terminal_completion_signal,
		reject_nonterminal_single_turn_completion, respond_to_dynamic_tool_call_dispatch,
		validated_dynamic_tool_specs,
	},
	lane_control::handle_pending_turn_control_requests,
	phase_goal::{
		PhaseGoalRuntime, app_server_method_not_found, clear_thread_phase_goal_best_effort,
		get_thread_phase_goal, initialize_phase_goal_runtime, record_phase_goal_completed,
		set_thread_phase_goal,
	},
	preflight::{run_app_server_capability_preflight, run_command_exec_health_check},
	schema_probe::probe_app_server_schema,
	server_requests::{
		apply_protocol_message_side_effects, handle_server_request_during_turn_execution,
		handle_server_request_while_waiting, interactive_flag_for_request,
		record_server_request_response,
	},
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
use serde::Serialize;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppServerThreadArchiveOutcome {
	Archived,
	DiscardedMissingThread,
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

#[cfg(test)] mod tests;
