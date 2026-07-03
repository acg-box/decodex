pub(in crate::orchestrator) mod post_review;

mod queue;
mod review_orchestration;
mod review_state;
mod runtime_recovery;
mod snapshot;
mod worktrees;

pub(in crate::orchestrator) use self::{
	post_review::{
		authority_boundary_landing_requirement, build_degraded_post_review_lane_statuses,
		build_post_review_lane_statuses, build_post_review_lane_statuses_and_hydrate_worktrees,
	},
	queue::{
		apply_queued_candidate_guardrail_commands, build_queued_candidate_status_plan,
		build_queued_candidate_statuses, codex_account_activity_summaries,
	},
	review_orchestration::{
		apply_non_github_review_post_review_classification,
		apply_pre_orchestration_post_review_classification,
		apply_review_orchestration_phase_classification, external_review_has_actionable_feedback,
		external_review_has_strict_pass_signals, external_review_result_arrived,
		load_post_review_lane_review_state, load_post_review_orchestration_marker,
		request_ack_timed_out, request_comment_has_eyes, validate_post_review_lane_review_state,
		validate_review_orchestration_marker,
	},
	review_state::{
		blocked_post_review_lane, blocked_post_review_lane_from_handoff,
		blocked_post_review_lane_from_state, blocked_post_review_lane_status,
		external_review_request_ci_gate, failed_checks_require_repair,
		initial_post_review_lane_classification, merge_state_requires_review_repair,
		readback_degraded_post_review_lane_from_handoff, resolve_configured_env_var,
		retained_closeout_pr_merge_gate_with_inspector,
		review_state_clean_path_landing_gates_satisfied, review_state_landing_gates_satisfied,
		review_state_landing_requires_agent_fallback, validate_post_review_lane_worktree,
		worktree_checkout_branch_name, worktree_head_descends_from_review_handoff,
		worktree_head_oid,
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

use state::{ProjectLoopEvidenceSnapshot, ReviewCheckpointArtifactLookup};

use crate::github::PullRequestMergeViewResponse;
#[cfg(test)]
use crate::orchestrator::ReviewLevel;
use crate::orchestrator::TrackerConnectorBackoff;
use crate::orchestrator::entrypoints_tracker_backoff::{
	active_stored_tracker_backoff_status, persist_tracker_backoff_state, tracker_connector_backoff,
};
use crate::orchestrator::kernel::state::OwnershipState;
use crate::orchestrator::kernel::state::PolicyState;
use crate::orchestrator::status_execution_programs::operator_execution_program_statuses;
use crate::orchestrator::status_ghost_lane_cleanup::apply_missing_issue_ghost_lane_projection;
use crate::orchestrator::status_github_cli_authority::operator_github_cli_authority;
use crate::orchestrator::status_history_ledger::{
	hydrate_history_lanes_from_linear_ledger, local_history_ledger_records,
	operator_history_ledger_outcome,
};
use crate::orchestrator::status_history_projection::{
	apply_operator_lane_terminal_projection, apply_terminal_history_ledger_outcome_to_run,
	current_lane_has_authoritative_live_owner, history_ledger_outcome_is_terminal,
	hydrate_history_lanes_from_local_ledger, suppress_terminal_attention_queue_echoes,
};
use crate::orchestrator::status_issue_metadata::hydrate_operator_run_rows_from_tracker;
use crate::orchestrator::status_models::{
	LiveOperatorStatusObserverContext, LiveOperatorStatusSnapshotOptions,
	RunIssueMetadataHydration, TrackerObserverOutcome,
};
use crate::orchestrator::status_run_projection::{
	hydrate_current_lane_lifecycle_metrics, operator_run_group_key, operator_run_status,
};
use crate::orchestrator::status_summary::{
	hydrate_post_review_lane_current_lane_shadowing, operator_run_has_live_execution,
};
use crate::orchestrator::status_worktrees::{
	operator_status_worktrees, stale_terminal_local_issue_ids,
};
use crate::orchestrator::{
	AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
	AccountActivityMode, BTreeSet, CodexAccountActivitySummary, CodexAccountPool, Command,
	EXTERNAL_REVIEW_ACK_TIMEOUT_SECS, EXTERNAL_REVIEW_ACTOR_LOGIN,
	EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS, EXTERNAL_REVIEW_PASS_PHRASE,
	ExternalReviewRequestCiGate, GhPullRequestReviewStateInspector, HashSet, Instant, IssueTracker,
	LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailCheckpoint, LoopGuardrailCheckpointInput,
	LoopGuardrailReason, ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON, OffsetDateTime,
	OperatorCodexAccountControlStatus, OperatorConnectorBackoffStatus, OperatorLoopStatus,
	OperatorPostReviewLaneStatus, OperatorProjectStatus, OperatorQueuedIssueStatus,
	OperatorRunStatus, OperatorStatusSnapshot, Path, PostReviewLaneBuildContext,
	PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
	PostReviewLaneStateLoad, PostReviewOrchestrationStatus, PostReviewReadbackDegradation,
	PostReviewRuntimeState, PrivateExecutionEvent, ProjectRunStatus, PullRequestReadbackRootCause,
	PullRequestReviewState, PullRequestReviewStateInspector, RecoverableWorktreeSkipCache,
	RecoveredRuntimeState, RetainedCloseoutPrMergeGate, RetryIssueStateHint, ReviewHandoffMarker,
	ReviewOrchestrationMarker, ReviewOrchestrationPhase, RunActivityMarker, ServiceConfig,
	StateStore, TrackerIssue, Value, WorkflowDocument, WorktreeManager, WorktreeMapping,
	WorktreeSpec, active_shared_issue_ids, classify_pull_request_readback_report,
	clear_recovered_issue_lease, compare_issue_candidates,
	current_lane_terminal_projection_from_local_ledger, env, eyre, fs, github, is_terminal_issue,
	issue_has_generic_dispatch_briefing, issue_passes_closeout_dispatch_policy,
	issue_passes_dispatch_policy, issue_passes_retry_dispatch_policy,
	issue_retry_budget_exhausted_for_worktree, json, loop_guardrail_text_hash,
	operator_boundary_policy_blocks_landing, operator_boundary_policy_requires_enhanced_evidence,
	operator_history_lanes, operator_loop_status_for_run, operator_project_display_name,
	operator_queued_issue_attention_status, operator_run_counts_as_current_lane,
	operator_run_issue_identifier_from_fields,
	ordinary_dispatch_blocked_by_retained_review_handoff, refresh_operator_project_summary,
	refresh_worktree_ownership, relative_worktree_path_for_path, slice, state,
	state_name_is_terminal, todo_blocker_rule_passes, tracker, worktree_activity_marker_is_fresh,
	worktree_mapping_is_stale_terminal_local_residue,
};
use crate::pull_request::{self, PullRequestLandingGateView};

pub(in crate::orchestrator) const QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT: &str =
	"linear_active_label_present";
pub(in crate::orchestrator) const ATTENTION_ERROR_EVIDENCE_MISSING: &str = "evidence_missing";
pub(in crate::orchestrator) const EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH: &str =
	"process_identity_mismatch";
pub(in crate::orchestrator) const GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING: &str =
	"tracker_issue_missing";
pub(in crate::orchestrator) const GHOST_LANE_OWNERSHIP_STATE: &str =
	OwnershipState::GhostLane.as_str();
pub(in crate::orchestrator) const GHOST_LANE_POLICY_STATE: &str =
	PolicyState::RuntimeRecoveryRequired.as_str();
pub(in crate::orchestrator) const GHOST_LANE_NEXT_ACTION: &str = "run_ghost_lane_recovery";
pub(in crate::orchestrator) const GHOST_LANE_TERMINAL_STATUS: &str = "terminal_guarded";
