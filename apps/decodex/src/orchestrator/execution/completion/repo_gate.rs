use crate::orchestrator::execution::{
	self, IssueRunPlan, LaneDecisionSnapshot, PhaseGoalKind, RUN_OPERATION_REPO_GATE,
	RUN_OPERATION_REVIEW_WRITEBACK, RepoGateFailure, RepoGateTrackedRewriteDecision, Result,
	ServiceConfig, StateStore, WorkflowDocument,
};

pub(crate) fn run_completion_repo_gate(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	phase: PhaseGoalKind,
) -> Result<()> {
	let selected_repo_gate = execution::select_repo_gate_for_worktree(
		workflow.frontmatter().execution(),
		&issue_run.worktree.path,
	);

	execution::write_run_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_REPO_GATE,
	);

	if let Err(error) = execution::run_repo_gate_commands(
		selected_repo_gate.canonicalize_commands(),
		selected_repo_gate.verify_commands(),
		&issue_run.worktree.path,
	) {
		if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
			let scope_envelope_violation = repo_gate_failure
				.tracked_rewrite_decision()
				.is_some_and(RepoGateTrackedRewriteDecision::is_scope_envelope_violation);
			let lane_snapshot = LaneDecisionSnapshot::repo_gate_failure(
				issue_run.issue.identifier.clone(),
				issue_run.run_id.clone(),
				issue_run.attempt_number,
				issue_run.dispatch_mode,
				phase,
				repo_gate_failure.disposition(),
				scope_envelope_violation,
			);
			let lane_decision = execution::decide_lane_next_action(&lane_snapshot);

			state_store.append_private_execution_event(
				project.service_id(),
				&issue_run.issue.id,
				&issue_run.run_id,
				issue_run.attempt_number,
				"lane_decision",
				lane_snapshot.to_json(lane_decision.next_action, lane_decision.reason),
			)?;
		}

		return Err(error);
	}

	execution::write_run_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_REVIEW_WRITEBACK,
	);

	Ok(())
}
