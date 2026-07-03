use crate::orchestrator::run_cycle::{
	self, GhPullRequestReviewStateInspector, INTERNAL_RETAINED_DRAIN_MAX_PASSES, IssueDispatchMode,
	IssueRunPlan, IssueTracker, Path, PullRequestReviewStateInspector, Result, RunSummary,
	ServiceConfig, StateStore, TargetIssueRunContext, WorkflowDocument,
};

use crate::{
	config::ServiceConfig,
	orchestrator::{
		GhPullRequestReviewStateInspector, IssueDispatchMode, IssueRunPlan, IssueTracker,
		PullRequestReviewStateInspector, Result, RunSummary, StateStore, TargetIssueRunContext,
		build_post_review_lane_statuses, execute_issue_run, post_review_lane_is_closeout_candidate,
		reconcile_post_review_orchestration_with_inspector,
		reconcile_terminal_thread_archive_backlog_best_effort, run_summary_from_issue_run,
	},
	workflow::WorkflowDocument,
};

pub(in crate::orchestrator::run_cycle) fn complete_issue_run<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: IssueRunPlan,
	dry_run: bool,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	if dry_run {
		return Ok(Some(run_cycle::run_summary_from_issue_run(project.service_id(), &issue_run)));
	}

	let summary = run_cycle::execute_issue_run(tracker, project, workflow, state_store, issue_run)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};
	let summary = if let Some(retained_summary) =
		drain_non_github_review_retained_tail_with_inspector(
			tracker,
			project,
			workflow,
			state_store,
			&summary,
			&review_state_inspector,
			|source_summary| {
				run_retained_closeout_for_handoff_summary(
					tracker,
					project,
					workflow,
					state_store,
					source_summary,
				)
			},
		)? {
		retained_summary
	} else {
		summary
	};

	run_cycle::reconcile_terminal_thread_archive_backlog_best_effort(
		project,
		workflow,
		state_store,
	);

	Ok(Some(summary))
}

pub(crate) fn run_retained_closeout_for_handoff_summary<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	source_summary: &RunSummary,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	run_cycle::run_target_issue_once(TargetIssueRunContext {
		tracker,
		project,
		workflow,
		state_store,
		issue_id: source_summary.issue_id.as_str(),
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: false,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
}

pub(crate) fn drain_non_github_review_retained_tail_with_inspector<T, I, F>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	summary: &RunSummary,
	review_state_inspector: &I,
	mut run_closeout: F,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
	F: FnMut(&RunSummary) -> Result<Option<RunSummary>>,
{
	if project.codex().review_level().uses_github_review()
		|| summary.continuation_pending
		|| !matches!(
			summary.dispatch_mode,
			IssueDispatchMode::Normal
				| IssueDispatchMode::Program
				| IssueDispatchMode::ReviewRepair
		) {
		return Ok(None);
	}

	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();

	for pass in 0..INTERNAL_RETAINED_DRAIN_MAX_PASSES {
		run_cycle::reconcile_post_review_orchestration_with_inspector(
			tracker,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)?;

		let Some(lane) = run_cycle::build_post_review_lane_statuses(
			tracker,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)?
		.into_iter()
		.find(|lane| lane.issue_id == summary.issue_id) else {
			return Ok(None);
		};

		if run_cycle::post_review_lane_is_closeout_candidate(&lane, completed_state) {
			if let Some(retained_summary) = run_closeout(summary)? {
				return Ok(Some(retained_summary));
			}

			return Ok(None);
		}
		if lane.reason != "non_github_review_waiting_for_merge"
			|| pass + 1 == crate::orchestrator::run_cycle::INTERNAL_RETAINED_DRAIN_MAX_PASSES
		{
			return Ok(None);
		}
	}

	Ok(None)
}
