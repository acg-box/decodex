pub(crate) mod autonomy;
pub(crate) mod execution_programs;
pub(crate) mod ghost_lane_cleanup;
pub(crate) mod ghost_lane_evidence;
pub(crate) mod github_cli_authority;
pub(crate) mod history_ledger;
pub(crate) mod history_projection;
pub(crate) mod issue_metadata;
pub(crate) mod models;
pub(crate) mod operator_worktrees;
pub(crate) mod process_liveness;
pub(crate) mod project_display;
pub(crate) mod queued_attention;
pub(crate) mod render;
pub(crate) mod run_projection;
pub(crate) mod summary;

mod post_review;
mod queue;
mod review_orchestration;
mod review_state;
mod runtime_recovery;
mod snapshot;
mod worktrees;

#[cfg(test)] pub(crate) use self::post_review::classify_post_review_lane;
pub(crate) use self::{
	post_review::{
		authority_boundary_landing_requirement, build_degraded_post_review_lane_statuses,
		build_post_review_lane_statuses, build_post_review_lane_statuses_and_hydrate_worktrees,
		load_post_review_worktree_issues,
	},
	queue::{
		apply_queued_candidate_guardrail_commands, build_queued_candidate_status_plan,
		build_queued_candidate_statuses, codex_account_activity_summaries,
	},
	review_orchestration::{
		apply_non_github_review_post_review_classification,
		apply_pre_orchestration_post_review_classification,
		apply_review_lifecycle_action_classification, external_review_has_actionable_feedback,
		external_review_has_strict_pass_signals, external_review_result_arrived,
		load_post_review_lane_review_state, load_post_review_lifecycle_record,
		request_ack_timed_out, request_comment_has_eyes, validate_post_review_lane_review_state,
		validate_post_review_lifecycle_record,
	},
	review_state::{
		blocked_post_review_lane, blocked_post_review_lane_from_lifecycle,
		blocked_post_review_lane_from_state, blocked_post_review_lane_status,
		external_review_request_ci_gate, failed_checks_require_repair,
		initial_post_review_lane_classification, merge_state_requires_review_repair,
		readback_degraded_post_review_lane_from_lifecycle, resolve_configured_env_var,
		retained_closeout_pr_merge_gate_with_inspector, review_state_checks_require_repair,
		review_state_clean_path_landing_gates_satisfied, review_state_landing_gates_satisfied,
		review_state_landing_requires_agent_fallback, validate_post_review_lane_worktree,
		worktree_head_descends_from_lifecycle_record,
	},
	runtime_recovery::{
		append_primary_account_if_missing, hydrate_status_snapshot_state,
		recover_runtime_state_from_tracker_and_worktrees,
		recover_runtime_state_from_tracker_and_worktrees_with_skip_cache,
		recoverable_worktree_identifiers, refresh_recoverable_runtime_issues,
	},
	snapshot::{
		add_operator_snapshot_warning, build_control_plane_operator_status_snapshot,
		build_lane_inspect_operator_runs, build_live_operator_status_snapshot,
		build_operator_status_snapshot, build_operator_status_snapshot_with_account_mode,
		build_status_command_operator_status_snapshot, global_codex_account_control_status,
		hydrate_live_operator_external_observers, operator_current_lane_statuses,
	},
	worktrees::{
		WorktreeTrackedChangeState, worktree_has_tracked_changes, worktree_tracked_change_state,
	},
};
pub(crate) use review_state::{worktree_checkout_branch_name, worktree_head_oid};

#[allow(unused_imports)] use crate::github::PullRequestMergeViewResponse;
use crate::orchestrator::kernel::state::{OwnershipState, PolicyState};
#[allow(unused_imports)]
use crate::orchestrator::{
	AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
	AccountActivityMode, BTreeSet, CodexAccountActivitySummary, CodexAccountPool, Command,
	EXTERNAL_REVIEW_ACK_TIMEOUT_SECS, EXTERNAL_REVIEW_ACTOR_LOGIN,
	EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS, EXTERNAL_REVIEW_PASS_PHRASE,
	ExternalReviewRequestCiGate, GhPullRequestReviewStateInspector, HashMap, HashSet, Instant,
	IssueTracker, LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LiveOperatorStatusObserverContext,
	LiveOperatorStatusSnapshotOptions, LoopGuardrailCheckpoint, LoopGuardrailCheckpointInput,
	LoopGuardrailReason, ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON, OffsetDateTime,
	OperatorCodexAccountControlStatus, OperatorConnectorBackoffStatus, OperatorLoopStatus,
	OperatorPostReviewLaneStatus, OperatorProjectStatus, OperatorQueuedIssueStatus,
	OperatorRunStatus, OperatorStatusSnapshot, Path, PostReviewLaneBuildContext,
	PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
	PostReviewLaneStateLoad, PostReviewLifecycleAction, PostReviewOrchestrationStatus,
	PostReviewReadbackDegradation, PostReviewRuntimeState, PrivateExecutionEvent, ProjectRunStatus,
	PullRequestReadbackRootCause, PullRequestReviewState, PullRequestReviewStateInspector,
	RecoverableWorktreeSkipCache, RecoveredRuntimeState, RetainedCloseoutPrMergeGate,
	RetryIssueStateHint, RunActivityMarker, RunIssueMetadataHydration, ServiceConfig, StateStore,
	TrackerConnectorBackoff, TrackerIssue, TrackerObserverOutcome, Value, WorkflowDocument,
	WorktreeManager, WorktreeMapping, WorktreeSpec, active_shared_issue_ids,
	active_stored_tracker_backoff_status, apply_missing_issue_ghost_lane_projection,
	apply_operator_lane_terminal_projection, apply_terminal_history_ledger_outcome_to_run,
	classify_pull_request_readback_report, clear_recovered_issue_lease, compare_issue_candidates,
	current_lane_has_authoritative_live_owner, current_lane_terminal_projection_from_local_ledger,
	env, eyre, fs, github, history_ledger_outcome_is_terminal,
	hydrate_current_lane_lifecycle_metrics, hydrate_history_lanes_from_linear_ledger,
	hydrate_history_lanes_from_local_ledger, hydrate_operator_run_rows_from_tracker,
	hydrate_post_review_lane_current_lane_shadowing, is_terminal_issue,
	issue_has_generic_dispatch_briefing, issue_passes_closeout_dispatch_policy,
	issue_passes_dispatch_policy, issue_passes_retry_dispatch_policy,
	issue_retry_budget_exhausted_for_worktree, json, local_history_ledger_records,
	loop_guardrail_text_hash, operator_boundary_policy_blocks_landing,
	operator_boundary_policy_requires_enhanced_evidence, operator_execution_program_statuses,
	operator_github_cli_authority, operator_history_lanes, operator_history_ledger_outcome,
	operator_loop_status_for_run, operator_project_display_name,
	operator_queued_issue_attention_status, operator_run_counts_as_current_lane,
	operator_run_group_key, operator_run_has_live_execution,
	operator_run_issue_identifier_from_fields, operator_run_status, operator_status_worktrees,
	ordinary_dispatch_blocked_by_retained_review_handoff, persist_tracker_backoff_state,
	refresh_operator_project_summary, refresh_worktree_ownership, relative_worktree_path_for_path,
	slice, stale_terminal_local_issue_ids, state_name_is_terminal,
	suppress_terminal_attention_queue_echoes, todo_blocker_rule_passes, tracker_connector_backoff,
	worktree_activity_marker_is_fresh, worktree_mapping_is_stale_terminal_local_residue,
};
#[allow(unused_imports)]
use crate::state::{
	ProjectLoopEvidenceSnapshot, ProtocolActivityEventSummary, ReviewCheckpointArtifactLookup,
};
#[allow(unused_imports)] use crate::tracker::records::LinearExecutionEventRecord;
#[allow(unused_imports)]
use crate::{
	agent::REVIEW_POLICY_CONVERGENCE_BUDGET, pull_request::PullRequestLandingGateView,
	tracker::public_text,
};

pub(crate) const QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT: &str = "linear_active_label_present";
pub(crate) const ATTENTION_ERROR_EVIDENCE_MISSING: &str = "evidence_missing";
pub(crate) const EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH: &str = "process_identity_mismatch";
pub(crate) const GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING: &str = "tracker_issue_missing";
pub(crate) const GHOST_LANE_OWNERSHIP_STATE: &str = OwnershipState::GhostLane.as_str();
pub(crate) const GHOST_LANE_POLICY_STATE: &str = PolicyState::RuntimeRecoveryRequired.as_str();
pub(crate) const GHOST_LANE_NEXT_ACTION: &str = "run_ghost_lane_recovery";
pub(crate) const GHOST_LANE_TERMINAL_STATUS: &str = "terminal_guarded";
