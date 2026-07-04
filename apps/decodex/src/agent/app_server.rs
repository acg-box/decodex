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
mod run;
mod runtime_types;
mod schema_probe;
mod server_requests;
mod session;
mod transport;
mod turn_failure;
mod turn_loop;

pub(crate) use self::{
	activity::protocol_activity_idle_timeout,
	constants::{MODEL_EXECUTION_IDLE_TIMEOUT, RUN_LEASE_IDLE_TIMEOUT},
	dynamic_tools::failure::AppServerDynamicToolFailure,
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
pub(crate) use archive::archive_app_server_thread_after_success;
pub(crate) use probe::probe_app_server;
pub(crate) use run::execute_app_server_run;
pub(crate) use turn_failure::AppServerTurnFailure;

use std::{
	collections::BTreeMap,
	error::Error,
	fmt::{self, Display, Formatter},
	path::Path,
	time::Duration,
};

use serde::Serialize;
use serde_json::{self, Value};

#[cfg(test)] use self::dynamic_tools::dispatch::handle_dynamic_tool_call;
#[cfg(test)]
use self::dynamic_tools::{classify_turn_completion, reject_nonterminal_single_turn_completion};
#[cfg(test)] use self::lane_control::steer_error_class;
#[cfg(test)]
use self::preflight::{
	AppServerCapabilityPreflightStatus, build_command_exec_health_check_params,
	mcp_preflight_can_degrade, plugin_list_params_for_preflight, preflight_request,
	preflight_request_with_timeout_retry, record_config_preflight, record_mcp_preflight,
	record_mcp_preflight_degraded, record_model_preflight, record_model_provider_preflight,
	record_plugin_preflight, record_skills_preflight, validate_command_exec_health_check_result,
};
#[cfg(test)] use self::protocol::{InitializeResponse, ProbeDynamicToolHandler, UserInput};
#[cfg(test)]
use self::schema_probe::{
	APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS, APP_SERVER_REQUIRED_CLIENT_REQUESTS,
	APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS, APP_SERVER_REQUIRED_SERVER_REQUESTS,
	APP_SERVER_SCHEMA_REQUIRED_MARKERS, validate_generated_app_server_schema,
};
#[cfg(test)]
use self::server_requests::{record_interactive_request_state, record_server_request};
use self::{
	activity::{ChildActivityAccumulator, ProtocolActivityAccumulator, redact_identifier},
	constants::{JSONRPC_METHOD_NOT_FOUND, REQUEST_TIMEOUT, THREAD_SESSION_REQUEST_TIMEOUT},
	dynamic_tools::{
		dispatch_dynamic_tool_call, dynamic_tool_call_unavailable_for_phase,
		respond_to_dynamic_tool_call_dispatch,
	},
	protocol::{
		AppServerClient, ChatgptAuthTokensRefreshParams, ChatgptAuthTokensRefreshResponse,
		CommandExecParams, CommandExecResponse, CommandExecutionApprovalDecision,
		CommandExecutionRequestApprovalResponse, ConfigReadParams, DynamicToolCallParams,
		EffectiveThreadConfig, FileChangeApprovalDecision, FileChangeRequestApprovalResponse,
		ListMcpServerStatusParams, ListMcpServerStatusResponse, McpServerElicitationAction,
		McpServerElicitationRequestResponse, McpServerStatusSummary, ModelListParams,
		ModelListResponse, ModelProviderCapabilitiesReadResponse, ModelSummary,
		PermissionGrantScope, PermissionsRequestApprovalResponse, PluginListParams,
		PluginListResponse, RuntimeConfigSummary, SkillsListParams, SkillsListResponse, ThreadGoal,
		ThreadGoalClearParams, ThreadGoalGetParams, ThreadGoalSetParams, ThreadGoalStatus,
		ThreadStatusChangedNotification, ToolRequestUserInputResponse, TurnInterruptRequest,
	},
	runtime_types::{RequestDispatchContext, RequestWaitPhase, RunRecorder},
	server_requests::{handle_server_request_while_waiting, interactive_flag_for_request},
	session::record_codex_account_failure,
	turn_loop::{
		build_turn_steer_request, is_app_server_output_timeout, message_type, targets_thread,
		thread_id_from_value, turn_id_from_value,
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
use crate::{
	agent::{
		codex_accounts::CodexAccountProvider,
		json_rpc::{
			AppServerOutputTimeout, JsonRpcConnection, JsonRpcMessage, JsonRpcRequest, WireMessage,
		},
		tracker_tool_bridge::{
			self, DynamicToolCallResponse, DynamicToolContentItem, DynamicToolHandler,
			DynamicToolSpec, TurnCompletionStatus,
		},
	},
	prelude::eyre,
	run_control::{
		LaneControlInterruptRequest, LaneControlInterruptResponse, LaneControlSteerRequest,
		LaneControlSteerResponse, LaneControlSteerResponseStatus, PendingLaneControlRequest,
		PendingLaneControlSteerRequest,
	},
	state::{
		CodexAccountActivitySummary, CodexAccountMarker, EffectiveRuntimeMarker,
		RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED, RUN_CONTROL_CHANNEL_DIR,
		RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE, RUN_OPERATION_AGENT_RUN,
		RUN_OPERATION_APP_SERVER_PREFLIGHT, RunControlActionOutcomeRequest, RunControlChannel,
		StateStore,
	},
};

#[cfg(test)] mod tests;
