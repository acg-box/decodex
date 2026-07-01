mod execution_architecture_recovery;
mod execution_closeout;
mod execution_failure;
mod execution_phase_goal;
mod execution_thread_archive;
mod harness_improvement;
mod lane_control;
mod lane_decision;
mod status_autonomy;
mod status_execution_programs;
mod status_ghost_lane_cleanup;
mod status_ghost_lane_evidence;
mod status_github_cli_authority;
mod status_history_ledger;
mod status_history_projection;
mod status_issue_metadata;
mod status_models;
mod status_process_liveness;
mod status_project_display;
mod status_queued_attention;
mod status_run_projection;
mod status_summary;
mod status_worktrees;

pub(crate) use lane_control::{
	DEFAULT_STEER_RESULT_WAIT_TIMEOUT, LaneInspectRequest, LaneInterruptRequest, interrupt_lane,
	print_lane_inspect, steer_lane,
};

#[cfg(unix)] use std::os::fd::AsRawFd;
use std::{
	cmp::Ordering,
	collections::{BTreeSet, HashMap, HashSet},
	env,
	error::Error,
	fmt::{self, Display, Formatter},
	fs::{self, File},
	io::{ErrorKind, Write},
	net::{SocketAddr, TcpListener},
	path::{Path, PathBuf},
	process::{Child, Command, Stdio},
	slice,
	sync::{
		Arc, Mutex,
		mpsc::{self, Sender},
	},
	thread::{self, JoinHandle},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use color_eyre::Report;
use records::LinearExecutionEventRecord;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use state::{ProjectLoopEvidenceSnapshot, ProtocolActivityEventSummary};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	agent,
	agent::REVIEW_POLICY_CONVERGENCE_BUDGET,
	default_branch_sync, git_credentials, state, tracker,
	tracker::{privacy_classifier::PublicProjectionPrivacyClassifier, public_text},
};
use state::PrivateExecutionEvent;
#[rustfmt::skip]
use crate::{agent::{RUN_LEASE_IDLE_TIMEOUT, AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure, AppServerHomePreflightFailure, AppServerPhaseGoalFailure, AppServerProcessEnv, AppServerRunRequest, AppServerRunResult, AppServerTransportFailure, AppServerTurnFailure, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, DecodexRunContext, DecodexToolBridge, PhaseGoalController, PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition, ReviewExecutionMode, ReviewHandoffContext, ReviewHandoffWritebackFailed, ReviewPolicyStopReason, ReviewPolicyStopRequested, RunCompletionDisposition, TrackerToolBridge, TurnContinuationGuard}, config::{ReviewLevel, ServiceConfig}, execution_program::{ExecutionNodeEvaluation, ExecutionProgramEvaluation, ExecutionProgramOperatorSummary, ExecutionProgramReadinessContext, ExecutionWorkflowPolicy}, git_credentials::GitCredentialSource, github, prelude::{Result, eyre}, state::{ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary, ExecutionProgramRecord, LoopGuardrailCheckpoint, LoopGuardrailCheckpointInput, ProjectRegistration, ProjectRunStatus, ProtocolActivitySummary, RUN_OPERATION_AGENT_RUN, RUN_OPERATION_GIT_CREDENTIALS, RUN_OPERATION_IDLE, RUN_OPERATION_RECONCILIATION, RUN_OPERATION_REPO_GATE, RUN_OPERATION_REVIEW_WRITEBACK, RUN_OPERATION_WAITING_EXTERNAL, ReviewHandoffMarker, ReviewOrchestrationMarker, RunActivityMarker, RunAttempt, StateStore, WorktreeMapping}, tracker::{IssueTracker, TrackerIssue, linear::LinearClient, records}, workflow::WorkflowDocument, worktree::{WorktreeManager, WorktreeSpec}};
use execution_architecture_recovery::{
	architecture_recovery_retry_next_action, loop_guardrail_architecture_recovery_decision,
};
#[cfg(test)] use execution_closeout::ensure_closeout_issue_completed_state;
use execution_closeout::execute_deterministic_closeout;
use execution_failure::{
	ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_RETRY_KIND,
	AppServerZeroEvidenceStartFailure, ArchitectureRecoveryStart,
	LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailRecoveryDecision,
	RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE, RetainedReviewRepairPushFailed,
	RetainedReviewRepairPushFailureKind, TerminalFailureWritebackRuntime,
	apply_terminal_failure_writeback, ensure_automation_activity_label, git_guardrail_output,
	handle_failure, loop_guardrail_effective_status, loop_guardrail_text_hash,
	loop_guardrail_worktree_fingerprint, preserve_and_promote_app_server_run_failure,
	retained_progress_source_error_class, retryable_failure_loop_guardrail_stop,
	run_failure_requires_terminal_attention, run_failure_writeback_disposition,
	truncate_private_diagnostic_text,
};
#[cfg(test)]
use execution_failure::{
	RunFailureWritebackDisposition, preserve_manual_attention_request,
	promote_zero_evidence_app_server_start_failure, retry_budget_attempts_for_current_failure,
	write_retry_schedule_marker_for_runtime_retry,
};
#[cfg(test)] use execution_phase_goal::RepoGatePhaseGoalController;
use execution_phase_goal::{
	PhaseAcceptanceCheckFailure, PhaseGoalRecoveryContinuation, build_phase_goal_controller,
	issue_has_blocking_lane_decision_evidence, latest_open_issue_phase_goal_before_attempt,
	maybe_continue_after_phase_goal_recovery, recover_phase_goal_continuation,
};
use execution_thread_archive::{
	archive_completed_issue_threads_best_effort,
	reconcile_terminal_thread_archive_backlog_best_effort,
};
#[cfg(test)]
use execution_thread_archive::{
	completed_issue_thread_archive_candidates, terminal_thread_archive_backlog_candidates,
};
use harness_improvement::{HarnessOutcomeKind, record_harness_outcome_best_effort};
#[cfg(test)]
use harness_improvement::{HarnessOutcomeRecordInput, record_harness_outcome_for_issue_run};
use lane_decision::{
	LaneDecisionSnapshot, LaneNextAction, decide_lane_next_action,
	lane_decision_blocks_automatic_execution,
};
use status_autonomy::{
	operator_autonomy_lineage_statuses, operator_autonomy_objective_status,
	operator_autonomy_proposal_statuses, operator_autonomy_report_status,
	operator_autonomy_signal_statuses,
};
use status_execution_programs::operator_execution_program_statuses;
pub(crate) use status_ghost_lane_cleanup::ghost_lane_cleanup_status_blockers;
use status_ghost_lane_cleanup::{
	apply_missing_issue_ghost_lane_projection, mark_operator_run_tracker_issue_missing,
};
use status_github_cli_authority::{
	operator_github_cli_authority, operator_github_cli_authority_from_registration,
};
use status_history_ledger::{
	collect_history_ledger_records, compare_history_ledger_record_position,
	hydrate_history_lanes_from_linear_ledger, local_history_ledger_records,
	not_loaded_history_ledger_outcome, operator_history_ledger_outcome, parse_rfc3339_unix_epoch,
};
use status_history_projection::{
	apply_operator_lane_terminal_projection, apply_terminal_history_ledger_outcome_to_run,
	apply_terminal_history_ledger_outcomes, current_lane_has_authoritative_live_owner,
	current_lane_terminal_projection_from_local_ledger, history_lane_group_key,
	history_ledger_outcome_is_terminal, history_ledger_outcome_requires_attention,
	hydrate_history_lanes_from_local_ledger, suppress_terminal_attention_queue_echoes,
};
use status_issue_metadata::{
	fill_missing_history_lane_issue_metadata, fill_missing_run_issue_metadata,
	hydrate_operator_run_rows_from_tracker, operator_run_is_stale_terminal_local_residue,
	operator_run_tracker_issue_identifier_selector,
};
use status_models::{
	AccountActivityMode, ExternalReviewRequestCiGate, LiveOperatorStatusObserverContext,
	LiveOperatorStatusSnapshotOptions, MarkerProcessLiveness, OperatorExecutionProgramReadback,
	OperatorHistoryLedgerRecord, OperatorIssueDisplayMetadata, OperatorLaneControlProjection,
	OperatorLaneTerminalProjection, OperatorLifecycleMetricPhase,
	OperatorReviewCheckpointSummaryFields, OperatorRunAppServerState,
	OperatorRunLifecycleProjection, OperatorRunProtocolSummary, OperatorRunTiming,
	OperatorTerminalFinalizeProjection, PostReviewLaneBuildContext, PostReviewOrchestrationStatus,
	PostReviewReadbackDegradation, PostReviewRuntimeState, RetainedCloseoutPrMergeGate,
	RunIssueMetadataHydration, TrackerObserverOutcome, WorktreeOwnership,
};
pub(crate) use status_process_liveness::process_is_alive;
use status_process_liveness::{
	marker_process_is_alive, marker_process_liveness_for_marker, run_activity_idle_timeout,
	worktree_activity_marker_is_fresh,
};
use status_project_display::operator_project_display_name;
use status_queued_attention::{
	operator_authority_decision_request_status_from_event, operator_queued_issue_attention_status,
};
use status_run_projection::{
	format_optional_i64, format_optional_unix_timestamp, hydrate_current_lane_lifecycle_metrics,
	operator_boundary_policy_blocks_landing, operator_boundary_policy_requires_enhanced_evidence,
	operator_history_lanes, operator_loop_status_for_run,
	operator_protocol_activity_detail_is_public, operator_run_group_key,
	operator_run_issue_identifier_from_fields, operator_run_status,
};
use status_summary::{
	hydrate_post_review_lane_current_lane_shadowing, operator_issue_attention_key,
	operator_run_counts_as_attention, operator_run_counts_as_current_lane,
	operator_run_counts_as_running, operator_run_counts_as_waiting,
	operator_run_has_fresh_execution, operator_run_has_live_execution,
	operator_run_has_recent_app_server_execution, operator_run_needs_attention,
	project_attention_count, project_history_only_attention_count,
	queued_candidate_counts_as_waiting_intake, refresh_operator_project_summary,
};
pub(crate) use status_worktrees::ensure_project_has_no_merged_worktree_cleanup_debt;
use status_worktrees::{
	active_shared_issue_ids, operator_status_worktrees, refresh_worktree_ownership,
	stale_terminal_local_issue_ids,
};

mod types;
#[allow(unused_imports)]
pub(crate) use types::{
	ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_PACKET_SCHEMA,
	ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE, ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE,
	AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
	ActiveWorkflowOverride, AgentGitCredentialsUnavailable, AuthorityBoundaryChangedSurface,
	AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition, AuthorityBoundaryImprovementSignal,
	AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface, AuthorityDecisionOption,
	AuthorityDecisionRequestInput, CachedWorkflowDocument, ChildExitRetryContext, ChildRunRef,
	CurrentChildRunContext, DaemonRunChild, DaemonTickContext, DiagnoseRequest, EvidenceRequest,
	GhPullRequestReviewStateInspector, IssueDispatchMode, IssueRunPlan, IssueTurnContinuationGuard,
	LaneSteerReport, LaneSteerRequest, LoopGuardrailReason, LoopGuardrailStopRequested,
	ManualAttentionRequested, MaterializedDaemonSpawnState, OperatorArchitectureRecoveryStatus,
	OperatorAuthorityDecisionRequestStatus, OperatorAutonomyDecisionContractStatus,
	OperatorAutonomyExecutionEvidenceStatus, OperatorAutonomyLineageStatus,
	OperatorAutonomyObjectiveStatus, OperatorAutonomyProgramIntakeStatus,
	OperatorAutonomyProposalRefusalStatus, OperatorAutonomyProposalStatus,
	OperatorAutonomyReportReadbackStatus, OperatorAutonomySignalStatus, OperatorBoundaryStatus,
	OperatorCodexAccountControlStatus, OperatorConnectorBackoffStatus,
	OperatorContinuationRecoveryStatus, OperatorControlRequests,
	OperatorExecutionProgramNodeStatus, OperatorExecutionProgramStatus, OperatorGitHubCliAuthority,
	OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome, OperatorLaneLifecycleAttemptEvidence,
	OperatorLaneLifecycleMetrics, OperatorLaneLifecyclePhaseMetrics, OperatorLinearScanRequest,
	OperatorLoopStatus, OperatorPhaseAcceptanceStatus, OperatorPostReviewLaneStatus,
	OperatorProjectStatus, OperatorQueuedIssueAttentionStatus, OperatorQueuedIssueStatus,
	OperatorRecoveryBudgetStatus, OperatorReviewCheckpointStatus, OperatorReviewLoopStatus,
	OperatorReviewRouteCount, OperatorRunControlCapability, OperatorRunStatus,
	OperatorSnapshotWarningDetail, OperatorStateEndpoint, OperatorStatusSnapshot,
	OperatorWorktreeHygieneStatus, OperatorWorktreeProvenanceStatus, OperatorWorktreeStatus,
	PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
	PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE,
	PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
	PostReviewLaneStateLoad, PreferredRunIdentity, PrepareIssueRunContext, ProjectDaemonRuntime,
	PublishedOperatorSnapshot, PullRequestActor, PullRequestCommitConnection,
	PullRequestCommitNode, PullRequestCommitPayload, PullRequestIssueCommentConnection,
	PullRequestIssueCommentNode, PullRequestIssueCommentState, PullRequestIssueCommentsData,
	PullRequestIssueCommentsNode, PullRequestIssueCommentsRepository,
	PullRequestIssueCommentsResponse, PullRequestMergeCommitNode, PullRequestPageInfo,
	PullRequestReactionGroup, PullRequestReactionUsersConnection, PullRequestReadbackFailure,
	PullRequestReadbackRootCause, PullRequestRepository, PullRequestRepositoryOwner,
	PullRequestReviewConnection, PullRequestReviewNode, PullRequestReviewRequestConnection,
	PullRequestReviewState, PullRequestReviewStateData, PullRequestReviewStateInspector,
	PullRequestReviewStateNode, PullRequestReviewStateRepository, PullRequestReviewStateResponse,
	PullRequestReviewSummaryState, PullRequestReviewThreadConnection, PullRequestReviewThreadNode,
	PullRequestStatusCheckRollup, RecoverableWorktreeSkipCache, RecoveredRuntimeState,
	RetainedPartialProgress, RetainedReviewLaneBlocked, RetainedReviewLaneLoad,
	RetainedReviewNeedsAttention, RetainedReviewRunIdentity, RetryDispatchDecision, RetryEntry,
	RetryIssueStateHint, RetryKind, RetryQueue, ReviewHandoffNeedsAttention,
	ReviewOrchestrationPhase, RunCycleRequest, RunLeaseDisposition, RunLeaseReconciliation,
	RunOnceRequest, RunSummary, SelectedIssueRunCandidate, ServeRequest, SpawnRunOnceChildRequest,
	StalledRunNeedsAttention, TargetIssueRunContext, TerminalFailureOutcome,
	TrackerConnectorBackoff, classify_pull_request_readback_report,
	record_authority_boundary_check_private_event, record_authority_decision_request_private_event,
};

include!("orchestrator/operator_presentation.rs");

mod entrypoints_control_plane;
#[cfg(test)]
pub(crate) use entrypoints_control_plane::{
	ControlPlaneProjectTick, collect_control_plane_snapshot, complete_project_status,
	linear_scan_due, remember_next_linear_scan, run_control_plane_dev_tick, run_control_plane_tick,
};
pub(crate) use entrypoints_control_plane::{
	build_diagnose_live_snapshot, build_operator_state_snapshot_without_live_observers,
	empty_control_plane_snapshot, run_control_plane, runtime_recovery_error_class,
	runtime_recovery_warning,
};

mod entrypoints_status_cache;
pub(crate) use entrypoints_status_cache::status_should_attempt_operator_snapshot_cache;
#[cfg(test)]
pub(crate) use entrypoints_status_cache::{
	StatusSnapshotHttpResponse, status_snapshot_from_operator_cache_response,
};
use entrypoints_status_cache::{
	add_status_snapshot_cache_miss_warning, status_snapshot_from_local_operator_cache,
};
mod entrypoints_tracker_backoff;
pub(crate) use entrypoints_tracker_backoff::{
	active_connector_backoff_statuses, active_stored_tracker_backoff_status,
	active_stored_tracker_backoff_status_best_effort,
	build_operator_status_snapshot_for_tracker_backoff, clear_tracker_backoff_state_best_effort,
	persist_tracker_backoff_state, push_connector_backoff_warning,
	render_tracker_backoff_cli_message, snapshot_warnings_include_tracker_backoff,
	tracker_connector_backoff, warnings_include_tracker_backoff,
};

include!("orchestrator/entrypoints.rs");

mod operator_http;
#[cfg(test)]
pub(crate) use operator_http::{
	DASHBOARD_MAX_WEBSOCKET_CLIENTS, DashboardClientSubscription,
	build_operator_lane_inspect_http_response, build_operator_lane_interrupt_http_response,
	build_operator_lane_steer_http_response, build_operator_run_activity_event,
	build_operator_state_http_response, build_operator_state_http_response_with_control_requests,
	dashboard_websocket_message, handle_operator_state_endpoint_connection,
	strip_dashboard_run_activity_volatile_fields,
};
pub(crate) use operator_http::{
	DashboardEventHub, operator_snapshot_json_value,
	run_operator_run_activity_websocket_broadcasts, run_operator_state_endpoint,
};

include!("orchestrator/pull_request_review.rs");

include!("orchestrator/program_reconciler.rs");

mod run_cycle_reconciliation;
pub(crate) use run_cycle_reconciliation::{
	local_run_attempt_status_is_terminal, looks_like_tracker_issue_identifier_key,
	reconcile_project_state, retained_closeout_lease_has_fresh_activity,
	terminal_issue_keeps_retained_closeout,
};

mod daemon_retry;
#[cfg(test)] pub(crate) use daemon_retry::schedule_retry_after_child_exit;
pub(crate) use daemon_retry::{retry_delay, write_retry_schedule_for_run};

mod daemon;
#[cfg(test)]
pub(crate) use daemon::{
	DaemonTickRuntimeContext, inspect_current_daemon_child_reconciliation,
	inspect_current_daemon_child_reconciliation_at, inspect_or_clear_active_children,
	load_daemon_tick_workflow, materialize_daemon_spawn_state, materialize_run_summary_worktree,
	plan_due_retry_run, plan_next_daemon_run, recover_and_reconcile_idle_daemon_state,
	run_daemon_tick_with_review_state_inspector,
};
pub(crate) use daemon::{
	build_operator_state_snapshot_for_publish, clear_orphaned_daemon_child_state,
	load_daemon_tick_context, resolve_child_exit_run_attempt, run_daemon_tick,
};

mod reconciliation;
pub(in crate::orchestrator) use reconciliation::{
	apply_run_lease_reconciliation, inspect_exited_daemon_child_reconciliation,
	observed_idle_duration, retained_review_handoff_matches_run, run_lease_reconciliation_workflow,
	stalled_idle_duration, stalled_run_has_retained_partial_progress, superseded_run_disposition,
};
#[cfg(test)]
pub(crate) use reconciliation::{
	inspect_exited_daemon_child_reconciliation_at, inspect_run_lease_reconciliation_at,
	stalled_protocol_idle_duration,
};

mod retained_review_orchestration;
#[cfg(test)]
pub(crate) use retained_review_orchestration::{
	PassiveRetainedAttentionRuntime, apply_passive_retained_manual_attention_with_run_identity,
	ensure_review_orchestration_marker,
};
pub(crate) use retained_review_orchestration::{
	RetainedReviewLane, reconcile_post_review_orchestration,
	reconcile_post_review_orchestration_with_inspector,
	worktree_mapping_is_stale_terminal_local_residue,
};

mod run_cycle_post_review;
pub(crate) use run_cycle_post_review::{
	post_review_lane_is_closeout_candidate, post_review_lane_is_repair_candidate,
	retained_closeout_preferred_run_identity, select_post_review_issue_candidate,
	select_target_post_review_closeout_issue_candidate_with_inspector,
	select_target_post_review_repair_issue_candidate_with_inspector,
};
#[cfg(test)]
pub(crate) use run_cycle_post_review::{
	retained_closeout_run_identity_is_reusable,
	select_post_review_closeout_issue_candidate_with_inspector,
	select_post_review_issue_candidate_with_inspector,
	select_post_review_repair_issue_candidate_with_inspector,
};

mod run_cycle;
pub(crate) use run_cycle::{
	closeout_lane_active_claim_blocks_dispatch, load_configured_cycle_workflow,
	plan_project_issue_run_with_exclusions, run_configured_cycle, run_target_issue_once,
};
#[cfg(test)]
pub(crate) use run_cycle::{
	drain_non_github_review_retained_tail_with_inspector, prepare_issue_run, run_project_once,
	run_retained_closeout_for_handoff_summary, run_target_issue_once_with_inferred_dispatch,
	select_target_status_visible_program_candidate, target_issue_active_claim_blocks_dispatch,
};

include!("orchestrator/runtime_validation.rs");

include!("orchestrator/execution_lifecycle.rs");

include!("orchestrator/execution.rs");

mod dispatch_policy;
pub(crate) use dispatch_policy::issue_has_generic_dispatch_briefing;
pub(in crate::orchestrator) use dispatch_policy::{
	CloseoutDispatchEligibility, ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON,
	cleanup_completed_post_review_lane, cleanup_terminal_worktree, cleanup_worktree_mapping,
	clear_recovered_issue_lease, clear_terminal_guard_marker, clear_worktree_retry_schedule,
	closeout_dispatch_block_reason, evaluate_closeout_dispatch_policy_with_inspector,
	is_issue_eligible, is_issue_in_progress_for_run,
	is_issue_not_dispatchable_for_current_dispatch, is_terminal_issue, issue_has_service_ownership,
	issue_passes_closeout_dispatch_policy, issue_passes_dispatch_policy,
	issue_passes_retry_dispatch_policy, issue_passes_retry_retention_policy,
	issue_passes_review_repair_dispatch_policy, issue_retry_budget_exhausted,
	issue_retry_budget_exhausted_for_worktree, mark_run_attempt_if_active,
	ordinary_dispatch_blocked_by_retained_review_handoff, refresh_issue,
	render_issue_description_for_prompt, retry_budget_base_for_dispatch_mode,
	retry_budget_base_for_issue_worktree, state_name_is_terminal, todo_blocker_rule_passes,
	write_retry_budget_marker, write_terminal_guard_marker,
};
#[cfg(test)]
pub(crate) use dispatch_policy::{
	closeout_dispatch_block_reason_with_inspector,
	issue_passes_closeout_dispatch_policy_with_inspector,
};

include!("orchestrator/prompting.rs");

mod git_ops;
pub(crate) use crate::workflow::ResolvedRepoGate;
pub(crate) use git_ops::{
	RepoGateCommandOutcome, RepoGateFailure, RepoGateFailureDiagnostic, RepoGateFailureDisposition,
	RepoGateTrackedRewriteDecision, delete_local_branch_if_present,
	detach_worktree_head_from_branch_if_checked_out, relative_worktree_path,
	relative_worktree_path_for_path, repo_gate_changed_tracked_files, repo_gate_output_text,
	run_repo_gate_commands, run_repo_gate_commands_allow_owned_tracked_rewrites,
	select_repo_gate_for_worktree,
};
#[cfg(test)]
pub(crate) use git_ops::{
	RepoGateFailureKind, repo_gate_shell_from_env, run_repo_gate_cleanliness_check_with_git,
};

mod status;
#[allow(clippy::wildcard_imports)]
#[allow(unused_imports)]
use status::*;
pub(crate) use status::{worktree_checkout_branch_name, worktree_head_oid};

mod status_render;
pub(in crate::orchestrator) use status_render::rendered_recovery_worktrees;
pub(crate) use status_render::{render_operator_status, render_queue_explain};

include!("orchestrator/selection.rs");

mod agent_evidence;
#[cfg(test)] use agent_evidence::PrivateEvidenceReadback;
use agent_evidence::{
	AgentEvidenceSource, AgentPrivateEvidenceRef, build_private_evidence_readback,
	private_evidence_ref_for_run_fields, render_agent_evidence_write_result,
	render_private_evidence_readback, render_private_evidence_reference,
	write_agent_evidence_best_effort, write_agent_evidence_snapshot,
};

pub(crate) const DEFAULT_STATUS_RUN_LIMIT: usize = 10;
pub(crate) const DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT: usize = 25;
pub(crate) const DEFAULT_OPERATOR_LISTEN_ADDRESS: &str = "127.0.0.1:8192";
pub(crate) const EXTERNAL_REVIEW_ACTOR_LOGIN: &str = "codex";
pub(crate) const EXTERNAL_REVIEW_REQUEST_BODY: &str = "@codex review";
pub(crate) const EXTERNAL_REVIEW_PASS_PHRASE: &str = "Didn't find any major issues.";
pub(crate) const EXTERNAL_REVIEW_ACK_TIMEOUT_SECS: i64 = 60;
pub(crate) const EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS: i64 = 15 * 60;

const CONTINUATION_RETRY_DELAY_MS: u64 = 1_000;
const FAILURE_RETRY_BASE_DELAY_MS: u64 = 10_000;
const RECOVERABLE_WORKTREE_SKIP_TTL: Duration = Duration::from_secs(10 * 60);
const CONTINUATION_PENDING_RUN_STATUS: &str = "continuation_pending";
const TERMINAL_GUARDED_RUN_STATUS: &str = "terminal_guarded";
const TERMINAL_GUARD_MARKER_FILE: &str = ".decodex-terminal-guarded";
const TRACKER_RATE_LIMIT_BACKOFF_SECS: u64 = 15 * 60;
const TRACKER_RATE_LIMIT_WARNING: &str = "tracker_rate_limited";
const TRACKER_TRANSIENT_TIMEOUT_BACKOFF_SECS: u64 = 60;
const TRACKER_TRANSIENT_TIMEOUT_WARNING: &str = "tracker_transient_timeout";
const OPERATOR_DASHBOARD_ENDPOINT_PATH: &str = "/";
const OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH: &str = "/dashboard";
const OPERATOR_DASHBOARD_WS_ENDPOINT_PATH: &str = "/dashboard/control";
const OPERATOR_LIVE_ENDPOINT_PATH: &str = "/livez";
const OPERATOR_ACCOUNTS_ENDPOINT_PATH: &str = "/api/accounts";
const OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH: &str = "/api/operator-snapshot";
const OPERATOR_LINEAR_SCAN_ENDPOINT_PATH: &str = "/api/linear-scan";
const OPERATOR_LANE_INSPECT_ENDPOINT_PATH: &str = "/api/lane/inspect";
const OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH: &str = "/api/lane/interrupt";
const OPERATOR_LANE_STEER_ENDPOINT_PATH: &str = "/api/lane/steer";
const OPERATOR_LANE_STEER_ALIAS_ENDPOINT_PATH: &str = "/api/lane-steer";
const OPERATOR_STATE_MAX_REQUEST_BYTES: usize = 256 * 1_024;
const OPERATOR_DASHBOARD_WS_CLIENT_MESSAGE_MAX_BYTES: usize = 64 * 1_024;
const OPERATOR_STATE_HEADER_TERMINATOR: &[u8] = b"\r\n\r\n";
const STATUS_OPERATOR_SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(60);
const STATUS_OPERATOR_SNAPSHOT_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT: Duration = Duration::from_millis(500);
const STATUS_OPERATOR_SNAPSHOT_WARNING: &str = "status_cached_snapshot_unavailable";
const OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL: Duration = Duration::from_secs(1);
const OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_CONTROL_PLANE_POLL_INTERVAL: Duration = Duration::from_secs(15);
const LINEAR_CONTROL_PLANE_POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
const PULL_REQUEST_REVIEW_STATE_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $reviewThreadsAfter: String) {
  repository(owner: $owner, name: $name) {
    mergeCommitAllowed
    pullRequest(number: $number) {
      url
      state
      isDraft
      reviewDecision
      mergeable
      mergeStateStatus
      headRefName
      headRefOid
      mergeCommit {
        oid
      }
      headRepository {
        name
      }
      headRepositoryOwner {
        login
      }
      reactionGroups {
        content
        users(first: 100) {
          totalCount
          nodes {
            login
          }
        }
      }
      comments(first: 100) {
        nodes {
          databaseId
          body
          createdAt
          author {
            login
          }
          reactionGroups {
            content
            users(first: 100) {
              totalCount
              nodes {
                login
              }
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
      reviews(last: 100) {
        nodes {
          body
          state
          submittedAt
          author {
            login
          }
        }
      }
      reviewRequests(first: 1) {
        totalCount
      }
      reviewThreads(first: 100, after: $reviewThreadsAfter) {
        nodes {
          isResolved
          isOutdated
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              state
            }
          }
        }
      }
    }
  }
}
"#;
const PULL_REQUEST_ISSUE_COMMENTS_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $commentsAfter: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      url
      comments(first: 100, after: $commentsAfter) {
        nodes {
          databaseId
          body
          createdAt
          author {
            login
          }
          reactionGroups {
            content
            users(first: 100) {
              totalCount
              nodes {
                login
              }
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
"#;

#[cfg(test)] mod tests;
