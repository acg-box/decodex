use crate::orchestrator::execution::{
	self, HarnessOutcomeKind, IssueRunPlan, IssueTracker, ManualAttentionRequested, PhaseGoalKind,
	RUN_OPERATION_REVIEW_WRITEBACK, Report, Result, ReviewHandoffNeedsAttention,
	ReviewHandoffWritebackFailed, RunCompletionDisposition, ServiceConfig, StateStore,
	TrackerToolBridge, WorkflowDocument, completion,
};

pub(crate) fn apply_run_completion_disposition<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	tracker_tool_bridge: &TrackerToolBridge<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let recorded_pr_url =
		tracker_tool_bridge.review_context().and_then(|context| context.recorded_pr_url.as_deref());

	match tracker_tool_bridge.completion_disposition()? {
		RunCompletionDisposition::ReviewHandoff => {
			execution::validate_review_handoff_runtime(project, false)?;
			completion::run_completion_repo_gate(
				project,
				workflow,
				state_store,
				issue_run,
				PhaseGoalKind::HandoffEvidence,
			)?;

			tracker_tool_bridge.apply_review_handoff().map_err(|error| {
				if let Some(writeback_error) = error.downcast_ref::<ReviewHandoffWritebackFailed>()
				{
					Report::new(ReviewHandoffNeedsAttention {
						issue_identifier: writeback_error.issue_identifier.clone(),
						pr_url: writeback_error.pr_url.clone(),
						run_id: writeback_error.run_id.clone(),
					})
					.wrap_err(error)
				} else {
					error
				}
			})?;

			self::record_completion_outcome(
				state_store,
				project,
				issue_run,
				HarnessOutcomeKind::ReviewHandoff,
				recorded_pr_url,
			);
		},
		RunCompletionDisposition::ManualAttention => {
			return Err(Report::new(ManualAttentionRequested {
				issue_identifier: issue_run.issue.identifier.clone(),
				label: workflow.frontmatter().tracker().needs_attention_label().to_owned(),
				run_id: issue_run.run_id.clone(),
				error_class: tracker_tool_bridge.manual_attention_error_class(),
			}));
		},
		RunCompletionDisposition::ReviewRepair => {
			execution::validate_review_repair_runtime(project, false)?;
			completion::run_completion_repo_gate(
				project,
				workflow,
				state_store,
				issue_run,
				PhaseGoalKind::ReviewRepairEvidence,
			)?;
			completion::push_retained_review_repair_head(project, issue_run, recorded_pr_url)?;

			tracker_tool_bridge.apply_review_repair()?;

			self::record_completion_outcome(
				state_store,
				project,
				issue_run,
				HarnessOutcomeKind::ReviewRepair,
				recorded_pr_url,
			);
		},
		RunCompletionDisposition::Closeout => {
			execution::write_run_operation_marker_best_effort(
				&issue_run.worktree.path,
				&issue_run.run_id,
				issue_run.attempt_number,
				RUN_OPERATION_REVIEW_WRITEBACK,
			);

			let cleanup_commit_sha = execution::worktree_head_oid(&issue_run.worktree.path)?;

			tracker_tool_bridge.apply_closeout()?;

			execution::cleanup_completed_post_review_lane(
				project,
				workflow,
				state_store,
				issue_run,
			)?;
			execution::write_cleanup_complete_lifecycle_event(
				tracker,
				project,
				state_store,
				issue_run,
				recorded_pr_url,
				cleanup_commit_sha.as_deref(),
			)?;

			tracker_tool_bridge.clear_closeout_issue_scope()?;

			self::record_completion_outcome(
				state_store,
				project,
				issue_run,
				HarnessOutcomeKind::Closeout,
				recorded_pr_url,
			);
		},
	}

	Ok(())
}

fn record_completion_outcome(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	kind: HarnessOutcomeKind,
	recorded_pr_url: Option<&str>,
) {
	execution::record_harness_outcome_best_effort(
		state_store,
		project.service_id(),
		issue_run,
		kind,
		None,
		Some("passed"),
		recorded_pr_url,
	);
}
