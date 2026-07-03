mod complete;
mod prepare;
mod project;
mod target_issue;

#[cfg(test)]
pub(crate) use self::{
	complete::{
		drain_non_github_review_retained_tail_with_inspector,
		run_retained_closeout_for_handoff_summary,
	},
	target_issue::{
		select_target_status_visible_program_candidate, target_issue_active_claim_blocks_dispatch,
	},
};
pub(crate) use self::{
	prepare::prepare_issue_run,
	project::{plan_project_issue_run_with_exclusions, run_project_once},
	target_issue::{
		closeout_lane_active_claim_blocks_dispatch, run_target_issue_once,
		run_target_issue_once_with_inferred_dispatch,
	},
};

use crate::orchestrator::{
	GhPullRequestReviewStateInspector, IssueDispatchMode, IssueRunPlan, IssueTracker, LinearClient,
	OffsetDateTime, Path, PreferredRunIdentity, PrepareIssueRunContext,
	PullRequestReviewStateInspector, RecoveredRuntimeState, Result, RetainedReviewRunIdentity,
	RetryIssueStateHint, RunCycleRequest, RunSummary, SelectedIssueRunCandidate, ServiceConfig,
	StateStore, TargetIssueRunContext, TrackerIssue, WorkflowDocument, WorktreeManager,
	WorktreeSpec, apply_queued_candidate_guardrail_commands, build_post_review_lane_statuses,
	build_queued_candidate_status_plan, build_run_id, cleanup_terminal_worktree,
	clear_terminal_guard_marker, closeout_dispatch_block_reason,
	ensure_project_has_no_merged_worktree_cleanup_debt, execute_issue_run, eyre, is_terminal_issue,
	issue_passes_current_dispatch_policy, planned_issue_state_for_dispatch,
	post_review_lane_is_closeout_candidate, reconcile_post_review_orchestration,
	reconcile_post_review_orchestration_with_inspector, reconcile_project_state,
	reconcile_terminal_thread_archive_backlog_best_effort, record_program_dispatch_selected,
	recover_runtime_state_from_tracker_and_worktrees, refresh_issue,
	retained_closeout_lease_has_fresh_activity, retained_closeout_preferred_run_identity,
	retry_budget_base_for_dispatch_mode, run_summary_from_issue_run,
	select_execution_program_run_candidate, select_issue_candidate_with_exclusions,
	select_post_review_issue_candidate, slice, validate_workflow_read_first_files,
	write_prepare_lifecycle_events,
};
use complete::complete_issue_run;

const INTERNAL_RETAINED_DRAIN_MAX_PASSES: usize = 2;

pub(crate) fn run_configured_cycle(request: RunCycleRequest<'_>) -> Result<Option<RunSummary>> {
	let config = ServiceConfig::from_path(request.config_path)?;
	let workflow = load_configured_cycle_workflow(&config, request.preferred_workflow_snapshot)?;
	let api_key = config.tracker().resolve_api_key()?;
	let tracker = LinearClient::new(api_key)?;

	if let Some(issue_id) = request.preferred_issue_id {
		let target_context = TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: request.state_store,
			issue_id,
			preferred_issue_state: request.preferred_issue_state,
			preferred_initial_issue_state: request.preferred_initial_issue_state,
			dry_run: request.dry_run,
			lease_preacquired: request.preferred_lease_acquired,
			preferred_issue_claim_fd: request.preferred_issue_claim_fd,
			preferred_dispatch_slot_fd: request.preferred_dispatch_slot_fd,
			preferred_dispatch_slot_index: request.preferred_dispatch_slot_index,
			dispatch_mode: request.preferred_dispatch_mode.unwrap_or(IssueDispatchMode::Normal),
			preferred_run_identity: request.preferred_run_identity,
			preferred_retry_budget_base: request.preferred_retry_budget_base,
		};

		return match request.preferred_dispatch_mode {
			Some(_) => run_target_issue_once(target_context),
			None => run_target_issue_once_with_inferred_dispatch(target_context),
		};
	}

	run_project_once(&tracker, &config, &workflow, request.state_store, request.dry_run)
}

pub(crate) fn load_configured_cycle_workflow(
	config: &ServiceConfig,
	preferred_workflow_snapshot: Option<&str>,
) -> Result<WorkflowDocument> {
	let workflow_path = config.workflow_path().to_path_buf();

	match preferred_workflow_snapshot {
		Some(snapshot) => WorkflowDocument::parse_markdown(snapshot),
		None => WorkflowDocument::from_path(&workflow_path),
	}
}
