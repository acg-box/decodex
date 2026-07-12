#[allow(dead_code)] pub(crate) mod kernel;

mod agent_evidence;
mod baseline_guard;
mod constants;
mod daemon;
mod daemon_retry;
mod dispatch_policy;
mod entrypoints;
mod entrypoints_control_plane;
mod entrypoints_status_cache;
mod entrypoints_tracker_backoff;
mod execution;
mod execution_architecture_recovery;
mod execution_closeout;
mod execution_failure;
mod execution_lifecycle;
mod execution_phase_goal;
mod execution_thread_archive;
mod git_ops;
mod harness_improvement;
mod lane_control;
mod lane_decision;
mod operator_http;
mod operator_presentation;
mod post_review_facts;
mod program_reconciler;
mod prompting;
mod pull_request_review;
mod reconciliation;
mod retained_review_orchestration;
mod run_cycle;
mod run_cycle_post_review;
mod run_cycle_reconciliation;
mod runtime_standard_review;
mod runtime_validation;
mod selection;
mod status;
mod types;

pub(crate) use self::{
	baseline_guard::{
		BaselineGuardDispatchOutcome, ensure_clean_baseline_before_dispatch,
		push_baseline_status_projection,
	},
	constants::{
		CONTINUATION_PENDING_RUN_STATUS, CONTINUATION_RETRY_DELAY_MS,
		DASHBOARD_WS_MESSAGE_MAX_BYTES, DEFAULT_CONTROL_PLANE_POLL_INTERVAL,
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT, DEFAULT_OPERATOR_LISTEN_ADDRESS,
		DEFAULT_STATUS_RUN_LIMIT, EXTERNAL_REVIEW_ACK_TIMEOUT_SECS, EXTERNAL_REVIEW_ACTOR_LOGIN,
		EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS, EXTERNAL_REVIEW_PASS_PHRASE,
		EXTERNAL_REVIEW_REQUEST_BODY, FAILURE_RETRY_BASE_DELAY_MS,
		LINEAR_CONTROL_PLANE_POLL_INTERVAL, OPERATOR_ACCOUNTS_ENDPOINT_PATH,
		OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH, OPERATOR_DASHBOARD_WS_ENDPOINT_PATH,
		OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL, OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL,
		OPERATOR_LANE_INSPECT_ENDPOINT_PATH, OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
		OPERATOR_LANE_STEER_ALIAS_ENDPOINT_PATH, OPERATOR_LANE_STEER_ENDPOINT_PATH,
		OPERATOR_LINEAR_SCAN_ENDPOINT_PATH, OPERATOR_LIVE_ENDPOINT_PATH,
		OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL, OPERATOR_STATE_HEADER_TERMINATOR,
		OPERATOR_STATE_MAX_REQUEST_BYTES, PULL_REQUEST_ISSUE_COMMENTS_QUERY,
		PULL_REQUEST_REVIEW_STATE_QUERY, RECOVERABLE_WORKTREE_SKIP_TTL,
		STATUS_OPERATOR_SNAPSHOT_CONNECT_TIMEOUT, STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT,
		STATUS_OPERATOR_SNAPSHOT_MAX_AGE, STATUS_OPERATOR_SNAPSHOT_WARNING,
		TERMINAL_GUARD_MARKER_FILE, TERMINAL_GUARDED_RUN_STATUS, TRACKER_RATE_LIMIT_BACKOFF_SECS,
		TRACKER_RATE_LIMIT_WARNING, TRACKER_TRANSIENT_TIMEOUT_BACKOFF_SECS,
		TRACKER_TRANSIENT_TIMEOUT_WARNING,
	},
	dispatch_policy::{
		CloseoutDispatchEligibility, REVIEW_HANDOFF_BLOCK_REASON, attest_issue_project_binding,
		cleanup_completed_post_review_lane, cleanup_terminal_worktree, cleanup_worktree_mapping,
		clear_recovered_issue_lease, clear_terminal_guard_marker, clear_worktree_retry_schedule,
		closeout_dispatch_block_reason, evaluate_closeout_dispatch_policy_with_inspector,
		is_issue_eligible, is_issue_in_progress_for_run,
		is_issue_not_dispatchable_for_current_dispatch, is_terminal_issue,
		issue_has_generic_dispatch_briefing, issue_has_service_ownership,
		issue_passes_closeout_dispatch_policy, issue_passes_current_dispatch_policy,
		issue_passes_dispatch_policy, issue_passes_retry_dispatch_policy,
		issue_passes_retry_retention_policy, issue_passes_review_repair_dispatch_policy,
		issue_retry_budget_exhausted, issue_retry_budget_exhausted_for_worktree,
		mark_run_attempt_if_active, ordinary_dispatch_blocked_by_retained_review_handoff,
		refresh_issue, retry_budget_base_for_dispatch_mode, retry_budget_base_for_issue_worktree,
		state_name_is_terminal, todo_blocker_rule_passes, write_retry_budget_marker,
		write_terminal_guard_marker,
	},
	entrypoints::{
		McpLaneSteerRequest, build_mcp_lane_control_resource, build_mcp_status_resource,
		print_private_evidence, print_status, run_diagnose, run_mcp_lane_interrupt,
		run_mcp_lane_steer, run_once,
	},
	entrypoints_control_plane::{
		build_diagnose_live_snapshot, build_operator_state_snapshot_without_live_observers,
		empty_control_plane_snapshot, run_control_plane, runtime_recovery_error_class,
		runtime_recovery_warning,
	},
	entrypoints_status_cache::status_should_attempt_operator_snapshot_cache,
	entrypoints_tracker_backoff::{
		active_connector_backoff_statuses, active_stored_tracker_backoff_status,
		active_stored_tracker_backoff_status_best_effort,
		build_operator_status_snapshot_for_tracker_backoff,
		clear_tracker_backoff_state_best_effort, persist_tracker_backoff_state,
		push_connector_backoff_warning, render_tracker_backoff_cli_message,
		snapshot_warnings_include_tracker_backoff, tracker_connector_backoff,
		warnings_include_tracker_backoff,
	},
	execution::{planned_issue_state_for_dispatch, run_summary_from_issue_run},
	lane_control::{
		DEFAULT_STEER_RESULT_WAIT_TIMEOUT, LaneInspectRequest, LaneInterruptRequest,
		interrupt_lane, print_lane_inspect, steer_lane,
	},
	operator_http::{
		DashboardEventHub, operator_snapshot_json_value,
		run_operator_run_activity_websocket_broadcasts, run_operator_state_endpoint,
	},
	reconciliation::{
		apply_run_lease_reconciliation, inspect_exited_daemon_child_reconciliation,
		observed_idle_duration, retained_review_handoff_matches_run,
		run_lease_reconciliation_workflow, stalled_idle_duration,
		stalled_run_has_retained_partial_progress, superseded_run_disposition,
	},
	retained_review_orchestration::{
		RetainedReviewLane, reconcile_post_review_orchestration,
		reconcile_post_review_orchestration_with_inspector,
		worktree_mapping_is_stale_terminal_local_residue,
	},
	run_cycle_post_review::{
		post_review_lane_is_closeout_candidate, post_review_lane_is_repair_dispatch_candidate,
		retained_closeout_preferred_run_identity, select_post_review_issue_candidate,
		select_target_closeout_candidate_with_inspector,
		select_target_review_repair_candidate_with_inspector,
	},
	run_cycle_reconciliation::{
		local_run_attempt_status_is_terminal, looks_like_tracker_issue_identifier_key,
		reconcile_project_state, retained_closeout_lease_has_fresh_activity,
		terminal_issue_keeps_retained_closeout,
	},
	status::{
		autonomy as status_autonomy, execution_programs as status_execution_programs,
		ghost_lane_cleanup as status_ghost_lane_cleanup,
		ghost_lane_evidence as status_ghost_lane_evidence,
		github_cli_authority as status_github_cli_authority,
		history_ledger as status_history_ledger, history_projection as status_history_projection,
		issue_metadata as status_issue_metadata, models as status_models,
		operator_worktrees as status_worktrees, process_liveness as status_process_liveness,
		project_display as status_project_display, queued_attention as status_queued_attention,
		render as status_render, run_projection as status_run_projection,
		summary as status_summary, worktree_checkout_branch_name, worktree_head_oid,
	},
	status_ghost_lane_cleanup::ghost_lane_cleanup_status_blockers,
	status_process_liveness::process_is_alive,
	status_render::{render_operator_status, render_queue_explain, rendered_recovery_worktrees},
	status_worktrees::ensure_project_has_no_merged_worktree_cleanup_debt,
};
#[cfg(test)]
pub(crate) use self::{
	dispatch_policy::{
		closeout_dispatch_block_reason_with_inspector,
		issue_passes_closeout_dispatch_policy_with_inspector,
	},
	entrypoints_control_plane::{
		ControlPlaneProjectTick, collect_control_plane_snapshot, complete_project_status,
		linear_scan_due, remember_next_linear_scan, run_control_plane_dev_tick,
		run_control_plane_tick,
	},
	entrypoints_status_cache::{
		client::StatusSnapshotHttpResponse, project::status_snapshot_from_operator_cache_response,
	},
};
#[allow(unused_imports)]
pub(crate) use self::{
	entrypoints::output::{publish_operator_snapshot, write_cli_output},
	operator_presentation::{
		OPERATOR_PRESENTATION_SCHEMA, OperatorCurrentLaneCard, OperatorSnapshotPresentation,
		insert_non_empty_operator_presentation_text, operator_current_lane_card,
		operator_current_lane_card_detail, operator_current_lane_card_title,
		operator_current_lane_card_tone, operator_run_assigned_account_emails,
		operator_run_assigned_account_fingerprints, operator_snapshot_presentation,
		operator_snapshot_presentation_value, trimmed_operator_presentation_text,
	},
	post_review_facts::{
		PostReviewLifecycleFacts, PostReviewLifecycleFactsInput, RuntimeReviewCheckpointStatus,
		RuntimeReviewGateState, build_post_review_lifecycle_facts,
		latest_runtime_review_checkpoint_status, runtime_review_checkpoint_status_for_head,
		runtime_review_checkpoint_status_for_head_phase, worktree_has_review_blocking_changes,
	},
	program_reconciler::{
		PROGRAM_DISPATCH_SELECTED_EVENT_TYPE, PROGRAM_DISPATCH_SELECTED_SCHEMA,
		ProgramIssueSnapshot, ProgramIssueSnapshotInput, ProgramSchedulerSelection,
		ProgramSchedulerSummary, RefreshedExecutionProgram, execution_program_dependency_snapshots,
		execution_program_occupied_conflict_domains, execution_program_readiness_context,
		insert_dependency_snapshot, program_issue_occupies_conflict_domain, program_issue_snapshot,
		record_program_dispatch_selected, record_program_dispatch_selected_for_summary,
		refresh_execution_program_issues, refresh_execution_program_local_lifecycle_facts,
		refresh_execution_program_tracker_facts, select_execution_program_run_candidate,
		select_execution_program_run_candidate_with_summary,
	},
	pull_request_review::{
		PullRequestIssueCommentsPageQuery, PullRequestReviewStatePageQuery,
		count_unresolved_review_threads, format_run_once_summary, issue_comment_state_from_node,
		merge_pull_request_issue_comment_page, merge_pull_request_review_state_page,
		next_pull_request_issue_comments_cursor, next_pull_request_review_threads_cursor,
		pull_request_review_state_from_page, pull_request_status_check_rollup_state,
		query_pull_request_issue_comments_page, query_pull_request_review_state_page,
		reaction_group_actor_count, review_summary_state_from_node,
	},
};
#[allow(unused_imports)]
pub(crate) use self::{
	execution::{
		build_closeout_review_state_inspector, configured_public_projection_privacy_classifier,
		execute_issue_run, execute_issue_run_inner, persist_issue_run_outcome,
		resolve_resume_thread_id, write_run_operation_marker_best_effort,
	},
	execution_lifecycle::{
		RunStartedLifecycleFields, TerminalFailureLifecycle, lifecycle_event_identity,
		terminal_failure_lifecycle_event, write_cleanup_complete_lifecycle_event,
		write_lifecycle_event, write_prepare_lifecycle_events, write_run_started_lifecycle_event,
	},
	runtime_validation::{
		validate_closeout_runtime, validate_command_available, validate_daemon_runtime,
		validate_review_handoff_runtime, validate_review_repair_runtime,
	},
};
#[allow(unused_imports)]
pub(crate) use self::{
	git_ops::{
		RepoGateCommandOutcome, RepoGateFailure, RepoGateFailureDiagnostic,
		RepoGateFailureDisposition, RepoGateTrackedRewriteDecision, delete_local_branch_if_present,
		detach_worktree_head_from_branch_if_checked_out, relative_worktree_path,
		relative_worktree_path_for_path, repo_gate_changed_tracked_files, repo_gate_output_text,
		run_repo_gate_commands, run_repo_gate_commands_with_owned_rewrites,
		select_repo_gate_for_worktree,
	},
	prompting::TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION,
	run_cycle::{
		closeout_lane_active_claim_blocks_dispatch, load_configured_cycle_workflow,
		plan_project_issue_run_with_exclusions, run_configured_cycle, run_target_issue_once,
		run_target_status_visible_program_once,
	},
};
#[cfg(test)]
pub(crate) use self::{
	git_ops::{
		RepoGateFailureKind, repo_gate_shell_from_env, run_repo_gate_cleanliness_check_with_git,
	},
	operator_http::{
		DASHBOARD_MAX_WEBSOCKET_CLIENTS, DashboardClientSubscription,
		build_operator_lane_inspect_http_response, build_operator_lane_interrupt_http_response,
		build_operator_lane_steer_http_response, build_operator_run_activity_event,
		build_operator_state_http_response, build_operator_state_http_response_with_controls,
		dashboard_websocket_message, handle_operator_state_endpoint_connection,
		strip_dashboard_run_activity_volatile_fields,
	},
};
#[allow(unused_imports)]
pub(crate) use self::{
	prompting::{
		build_continuation_user_input, build_developer_instructions, build_review_run_context,
		build_user_input, review_pull_request_title, validate_workflow_read_first_files,
	},
	selection::{
		RetryComment, build_run_id, compare_issue_candidates, current_timestamp,
		format_no_eligible_issue_hint, format_no_eligible_issue_message,
		format_no_eligible_queue_label_hint, format_retry_comment,
		format_status_no_eligible_issue_hint, format_terminal_failure_comment, resolve_config_path,
		retained_review_needs_attention_error_class, retry_comment_details,
		review_policy_stop_terminal_next_action, select_issue_candidate,
		select_issue_candidate_with_exclusions, sleep_until_next_tick,
		terminal_failure_comment_details, terminal_failure_pr_url, terminal_failure_recovery_gate,
	},
};
#[cfg(test)]
pub(crate) use self::{
	reconciliation::{
		inspect_exited_daemon_child_reconciliation_at, inspect_run_lease_reconciliation_at,
		stalled_protocol_idle_duration,
	},
	retained_review_orchestration::{
		PassiveRetainedAttentionRuntime,
		attention::apply_passive_retained_manual_attention_with_run_identity,
		lifecycle_authority::ensure_review_lifecycle_authority,
	},
};
#[cfg(test)]
pub(crate) use self::{
	run_cycle::{
		drain_non_github_review_retained_tail_with_inspector, prepare_issue_run, run_project_once,
		run_retained_closeout_for_handoff_summary, run_target_issue_once_with_inferred_dispatch,
		select_target_status_visible_program_candidate, target_issue_active_claim_blocks_dispatch,
	},
	run_cycle_post_review::{
		retained_closeout_run_identity_is_reusable,
		select_post_review_closeout_issue_candidate_with_inspector,
		select_post_review_issue_candidate_with_inspector,
		select_post_review_repair_issue_candidate_with_inspector,
	},
};
#[cfg(test)] pub(crate) use crate::agent::ISSUE_REVIEW_CHECKPOINT_TOOL_NAME;
#[cfg(test)]
pub(crate) use crate::state::{ReviewLifecycleHandoffFixture, ReviewLifecycleTransitionFixture};
pub(crate) use crate::workflow::ResolvedRepoGate;
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
#[cfg(test)] pub(crate) use daemon_retry::schedule_retry_after_child_exit;
pub(crate) use daemon_retry::{retry_delay, write_retry_schedule_for_run};
#[allow(unused_imports)] pub(crate) use execution::prepare_agent_git_credentials;
#[cfg(test)]
pub(crate) use execution::{
	AgentGitCredentialEnvironment, push_retained_review_repair_head, run_completion_repo_gate,
};
#[allow(unused_imports)]
pub(crate) use types::{
	ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_PACKET_SCHEMA,
	ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE, ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE,
	AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_BOUNDARY_CHECK_SCHEMA,
	AUTHORITY_DECISION_REQUEST_EVENT_TYPE, ActiveWorkflowOverride, AgentGitCredentialsUnavailable,
	AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition,
	AuthorityBoundaryImprovementSignal, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	AuthorityDecisionOption, AuthorityDecisionRequestInput, CachedWorkflowDocument,
	ChildExitRetryContext, ChildRunRef, CurrentChildRunContext, DaemonRunChild, DaemonTickContext,
	DiagnoseRequest, EvidenceRequest, GhPullRequestReviewStateInspector, IssueDispatchMode,
	IssueRunPlan, IssueTurnContinuationGuard, LaneSteerReport, LaneSteerRequest,
	LoopGuardrailReason, LoopGuardrailStopRequested, ManualAttentionRequested,
	MaterializedDaemonSpawnState, OperatorArchitectureRecoveryStatus,
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
	OperatorLoopStatus, OperatorPostReviewLaneStatus, OperatorProjectStatus,
	OperatorQueuedIssueAttentionStatus, OperatorQueuedIssueStatus, OperatorRecoveryBudgetStatus,
	OperatorReviewCheckpointStatus, OperatorReviewLoopStatus, OperatorReviewRouteCount,
	OperatorRunControlCapability, OperatorRunStatus, OperatorSnapshotWarningDetail,
	OperatorStateEndpoint, OperatorStatusSnapshot, OperatorValidationEvidenceStatus,
	OperatorWorktreeHygieneStatus, OperatorWorktreeProvenanceStatus, OperatorWorktreeStatus,
	PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT, PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE,
	PHASE_GOAL_RECOVERY_EVENT_TYPE, PostReviewLaneClassification, PostReviewLaneDecision,
	PostReviewLaneSnapshot, PostReviewLaneStateLoad, PreferredRunIdentity, PrepareIssueRunContext,
	ProgramDispatchSelection, ProjectDaemonRuntime, PublishedOperatorSnapshot, PullRequestActor,
	PullRequestCommitConnection, PullRequestCommitNode, PullRequestCommitPayload,
	PullRequestIssueCommentConnection, PullRequestIssueCommentNode, PullRequestIssueCommentState,
	PullRequestIssueCommentsData, PullRequestIssueCommentsNode, PullRequestIssueCommentsRepository,
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
	RetryEntryLifecycle, RetryIssueStateHint, RetryKind, RetryQueue, ReviewHandoffNeedsAttention,
	RunCycleRequest, RunLeaseDisposition, RunLeaseReconciliation, RunOnceRequest, RunSummary,
	SelectedIssueRunCandidate, ServeRequest, SpawnRunOnceChildRequest, StalledRunNeedsAttention,
	TargetIssueRunContext, TerminalFailureOutcome, TrackerConnectorBackoff,
	VALIDATION_EVIDENCE_EVENT_TYPE, VALIDATION_EVIDENCE_SCHEMA,
	classify_pull_request_readback_report, record_authority_boundary_check_private_event,
	record_authority_decision_request_private_event,
};

use std::{
	collections::{BTreeSet, HashMap, HashSet},
	env,
	error::Error,
	fmt::{self, Display, Formatter},
	fs::{self, File},
	io::ErrorKind,
	net::{SocketAddr, TcpListener},
	path::{Path, PathBuf},
	process::{Child, Command},
	slice,
	sync::{
		Arc, Mutex,
		mpsc::{self, Sender},
	},
	thread::{self, JoinHandle},
	time::{Duration, Instant},
};

use color_eyre::Report;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	agent::{CodexAccountAuthFailure, CodexAccountPool, REVIEW_POLICY_CONVERGENCE_BUDGET},
	default_branch_sync, runtime,
	state::{PrivateExecutionEvent, ProjectLoopEvidenceSnapshot, ProtocolActivityEventSummary},
	tracker::privacy_classifier::PublicProjectionPrivacyClassifier,
};
#[allow(unused_imports)]
#[rustfmt::skip]
use crate::{agent::{RUN_LEASE_IDLE_TIMEOUT, AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure, AppServerHomePreflightFailure, AppServerPhaseGoalFailure, AppServerProcessEnv, AppServerRunRequest, AppServerRunResult, AppServerTransportFailure, AppServerTurnFailure, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, DecodexRunContext, DecodexToolBridge, PhaseGoalController, PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition, ReviewHandoffContext, ReviewHandoffWritebackFailed, ReviewPolicyStopReason, ReviewPolicyStopRequested, RunCompletionDisposition, TrackerToolBridge, TurnContinuationGuard}, config::{ReviewLevel, ServiceConfig}, execution_program::{ExecutionNodeEvaluation, ExecutionProgramEvaluation, ExecutionProgramOperatorSummary, ExecutionProgramReadinessContext, ExecutionWorkflowPolicy}, git_credentials::GitCredentialSource, github, prelude::{Result, eyre}, state::{ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary, ExecutionProgramRecord, LoopGuardrailCheckpoint, LoopGuardrailCheckpointInput, ProjectRegistration, ProjectRunStatus, ProtocolActivitySummary, RUN_OPERATION_AGENT_RUN, RUN_OPERATION_GIT_CREDENTIALS, RUN_OPERATION_IDLE, RUN_OPERATION_RECONCILIATION, RUN_OPERATION_REPO_GATE, RUN_OPERATION_REVIEW_WRITEBACK, RUN_OPERATION_WAITING_EXTERNAL, ReviewLifecycleReadback, RunActivityMarker, RunAttempt, StateStore, WorktreeMapping}, tracker::{IssueTracker, TrackerIssue, linear::LinearClient, records}, workflow::WorkflowDocument, worktree::{WorktreeManager, WorktreeSpec}};
#[cfg(test)] use self::status::classify_post_review_lane;
#[allow(unused_imports)]
use self::status::{
	ATTENTION_ERROR_EVIDENCE_MISSING, EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH,
	GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING, GHOST_LANE_NEXT_ACTION, GHOST_LANE_OWNERSHIP_STATE,
	GHOST_LANE_POLICY_STATE, GHOST_LANE_TERMINAL_STATUS, QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT,
	WorktreeTrackedChangeState, add_operator_snapshot_warning,
	apply_queued_candidate_guardrail_commands, authority_boundary_landing_requirement,
	build_control_plane_operator_status_snapshot, build_degraded_post_review_lane_statuses,
	build_lane_inspect_operator_runs, build_live_operator_status_snapshot,
	build_operator_status_snapshot, build_operator_status_snapshot_with_account_mode,
	build_post_review_lane_statuses, build_queued_candidate_status_plan,
	build_queued_candidate_statuses, build_status_command_operator_status_snapshot,
	external_review_has_actionable_feedback, external_review_has_strict_pass_signals,
	external_review_request_ci_gate, external_review_result_arrived, failed_checks_require_repair,
	global_codex_account_control_status, hydrate_status_snapshot_state,
	load_post_review_lane_review_state, load_post_review_worktree_issues,
	merge_state_requires_review_repair, recover_runtime_state_from_tracker_and_worktrees,
	recover_runtime_state_with_skip_cache, recoverable_worktree_identifiers,
	request_comment_has_eyes, resolve_configured_env_var,
	retained_closeout_pr_merge_gate_with_inspector, review_state_checks_require_repair,
	review_state_clean_path_landing_gates_satisfied, review_state_landing_gates_satisfied,
	review_state_landing_requires_agent_fallback, validate_post_review_lifecycle_record,
	worktree_has_tracked_changes, worktree_tracked_change_state,
};
use self::{
	lane_decision::{
		LaneDecisionSnapshot, LaneNextAction, RepoGateFailureSignal, decide_lane_next_action,
	},
	status_autonomy::{
		operator_autonomy_lineage_statuses, operator_autonomy_objective_status,
		operator_autonomy_proposal_statuses, operator_autonomy_report_status,
		operator_autonomy_signal_statuses,
	},
	status_execution_programs::operator_execution_program_statuses,
	status_ghost_lane_cleanup::{
		apply_missing_issue_ghost_lane_projection, mark_operator_run_tracker_issue_missing,
	},
	status_github_cli_authority::{
		operator_github_cli_authority, operator_github_cli_authority_from_registration,
	},
	status_history_ledger::{
		collect_history_ledger_records, compare_history_ledger_record_position,
		hydrate_history_lanes_from_linear_ledger, local_history_ledger_records,
		not_loaded_history_ledger_outcome, operator_history_ledger_outcome,
		parse_rfc3339_unix_epoch,
	},
	status_history_projection::{
		apply_operator_lane_terminal_projection, apply_terminal_history_ledger_outcome_to_run,
		apply_terminal_history_ledger_outcomes, current_lane_has_authoritative_live_owner,
		current_lane_terminal_projection_from_local_ledger, history_lane_group_key,
		history_ledger_outcome_is_terminal, history_ledger_outcome_requires_attention,
		hydrate_history_lanes_from_local_ledger, suppress_terminal_attention_queue_echoes,
	},
	status_issue_metadata::{
		fill_missing_history_lane_issue_metadata, fill_missing_run_issue_metadata,
		hydrate_operator_run_rows_from_tracker, operator_run_is_stale_terminal_local_residue,
		operator_run_tracker_issue_identifier_selector,
	},
	status_models::{
		AccountActivityMode, ExternalReviewRequestCiGate, LiveOperatorStatusObserverContext,
		LiveOperatorStatusSnapshotOptions, MarkerProcessLiveness, OperatorExecutionProgramReadback,
		OperatorHistoryLedgerRecord, OperatorIssueDisplayMetadata, OperatorLaneControlProjection,
		OperatorLaneTerminalProjection, OperatorLifecycleMetricPhase,
		OperatorReviewCheckpointSummaryFields, OperatorRunAppServerState,
		OperatorRunLifecycleProjection, OperatorRunProtocolSummary, OperatorRunTiming,
		OperatorTerminalFinalizeProjection, PostReviewLaneBuildContext, PostReviewLifecycleAction,
		PostReviewOrchestrationStatus, PostReviewReadbackDegradation, PostReviewRuntimeState,
		RetainedCloseoutPrMergeGate, RunIssueMetadataHydration, TrackerObserverOutcome,
		WorktreeOwnership,
	},
	status_process_liveness::{
		marker_process_is_alive, marker_process_liveness_for_marker,
		worktree_activity_marker_is_fresh,
	},
	status_project_display::operator_project_display_name,
	status_queued_attention::{
		operator_authority_decision_request_status_from_event,
		operator_queued_issue_attention_status,
	},
	status_run_projection::{
		format_optional_i64, format_optional_unix_timestamp,
		hydrate_current_lane_lifecycle_metrics, operator_boundary_policy_blocks_landing,
		operator_boundary_policy_requires_enhanced_evidence, operator_history_lanes,
		operator_loop_status_for_run, operator_protocol_activity_detail_is_public,
		operator_run_group_key, operator_run_issue_identifier_from_fields, operator_run_status,
	},
	status_summary::{
		hydrate_post_review_lane_current_lane_shadowing, operator_issue_attention_key,
		operator_run_counts_as_attention, operator_run_counts_as_current_lane,
		operator_run_counts_as_running, operator_run_counts_as_waiting,
		operator_run_has_live_execution, operator_run_has_recent_app_server_execution,
		project_attention_count, project_history_only_attention_count,
		queued_candidate_counts_as_waiting_intake, refresh_operator_project_summary,
	},
	status_worktrees::{
		active_shared_issue_ids, operator_status_worktrees, refresh_worktree_ownership,
		stale_terminal_local_issue_ids,
	},
};
use crate::tracker::records::LinearExecutionEventRecord;
#[cfg(test)] use agent_evidence::PrivateEvidenceReadback;
use agent_evidence::{
	AgentEvidenceSource, AgentPrivateEvidenceRef, build_private_evidence_readback,
	render_agent_evidence_write_result, render_private_evidence_readback,
	render_private_evidence_reference, write_agent_evidence_best_effort,
	write_agent_evidence_snapshot,
};
use entrypoints_status_cache::{
	add_status_snapshot_cache_miss_warning, status_snapshot_from_local_operator_cache,
};
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
	PhaseGoalRecoveryContinuation, ValidationEvidenceFailure, build_phase_goal_controller,
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

#[cfg(test)] mod tests;
