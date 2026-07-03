mod activity;
mod archive;
mod constants;
mod dynamic_tools;
mod lane_control;
mod markers;
mod phase_goal;
mod preflight;
mod probe;
mod protocol;
mod runtime_types;
mod schema_probe;
mod server_requests;
mod session;
mod transport;
mod turn_failure;
mod turn_loop;

pub(crate) use archive::archive_app_server_thread_after_success;
pub(crate) use probe::probe_app_server;
pub(crate) use turn_failure::AppServerTurnFailure;

#[cfg(test)]
use self::dynamic_tools::{
	classify_turn_completion, handle_dynamic_tool_call, reject_nonterminal_single_turn_completion,
};
#[cfg(test)]
use self::lane_control::steer_error_class;
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
	constants::{MODEL_EXECUTION_IDLE_TIMEOUT, RUN_LEASE_IDLE_TIMEOUT},
	dynamic_tools::AppServerDynamicToolFailure,
	phase_goal::{
		AppServerPhaseGoalFailure, PhaseGoalController, PhaseGoalKind, PhaseGoalRunStatus,
		PhaseGoalSpec, PhaseGoalTransition,
	},
	preflight::{
		AppServerCapabilityPreflightFailure, AppServerCapabilityPreflightReport,
		CommandExecHealthCheck,
	},
	runtime_types::{
		AppServerRunRequest, AppServerRunResult, AppServerThreadArchiveOutcome,
		AppServerThreadArchiveRequest, TurnContinuationGuard,
	},
};
use self::{
	activity::{ChildActivityAccumulator, ProtocolActivityAccumulator, redact_identifier},
	constants::{JSONRPC_METHOD_NOT_FOUND, REQUEST_TIMEOUT},
	dynamic_tools::{
		dispatch_dynamic_tool_call, dynamic_tool_call_unavailable_for_phase,
		respond_to_dynamic_tool_call_dispatch,
	},
	markers::{
		publish_run_control_channel_for_request, write_activity_marker_best_effort,
		write_activity_marker_best_effort_for_request,
		write_capability_preflight_marker_best_effort,
	},
	preflight::{run_app_server_capability_preflight, run_command_exec_health_check},
	runtime_types::{RequestDispatchContext, RequestWaitPhase, RunRecorder},
	server_requests::{
		handle_server_request_while_waiting, interactive_flag_for_request,
		record_server_request_response,
	},
	session::{
		initialize_client_for_run, login_codex_account_for_run, record_codex_account_failure,
		record_thread_session_start, start_or_resume_thread_session,
	},
	turn_loop::{
		build_turn_steer_request, execute_turn_loop, flush_pending_messages,
		is_app_server_output_timeout, message_type, targets_thread, thread_id_from_value,
		turn_id_from_value,
	},
};
#[cfg(test)]
use self::{
	archive::{record_thread_archive_result_best_effort, thread_archive_error_allows_discard},
	session::{
		build_thread_resume_request, build_thread_start_request,
		thread_resume_error_allows_fallback, validate_effective_thread_config,
		validate_initialize_codex_home,
	},
	turn_loop::{
		build_turn_start_request, continuation_boundary_reached, failure_from_error_notification,
		handle_turn_execution_notification, remaining_idle_budget,
		turn_failure_from_json_rpc_error_response,
	},
};

use std::{
	error::Error,
	fmt::{self, Display, Formatter},
	path::Path,
	time::Duration,
};

use serde::Serialize;

use self::protocol::{
	AppServerClient, ChatgptAuthTokensRefreshParams, ChatgptAuthTokensRefreshResponse,
	CommandExecutionApprovalDecision, CommandExecutionRequestApprovalResponse, ConfigReadParams,
	DynamicToolCallParams, EffectiveThreadConfig, FileChangeApprovalDecision,
	FileChangeRequestApprovalResponse, McpServerElicitationAction,
	McpServerElicitationRequestResponse, PermissionGrantScope, PermissionsRequestApprovalResponse,
	SkillsListParams, ThreadGoal, ThreadGoalClearParams, ThreadGoalGetParams, ThreadGoalSetParams,
	ThreadGoalStatus, ThreadStatusChangedNotification, ToolRequestUserInputResponse,
	TurnInterruptRequest,
};
#[cfg(test)]
use self::protocol::{InitializeResponse, ProbeDynamicToolHandler, UserInput};
use crate::{
	agent::{
		codex_accounts::CodexAccountProvider,
		json_rpc::{JsonRpcConnection, JsonRpcMessage, JsonRpcRequest, WireMessage},
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
		Ok(_result) => {
			if control_channel.is_some() {
				state_store.retire_run_control_channel_for_attempt(
					&request.run_id,
					request.attempt_number,
					RUN_CONTROL_CHANNEL_STATUS_COMPLETED,
				)?;
			}
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

#[cfg(test)]
mod tests;
