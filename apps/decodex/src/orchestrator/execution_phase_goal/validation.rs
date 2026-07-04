mod acceptance_flow;
mod repo_gate;

use crate::orchestrator::{
	self, PhaseGoalKind, PhaseGoalTransition, RUN_OPERATION_REPO_GATE, Result,
	execution_phase_goal::controller::RepoGatePhaseGoalController,
};

impl RepoGatePhaseGoalController<'_> {
	pub(super) fn validate_phase_goal_output(
		&self,
		phase: PhaseGoalKind,
	) -> Result<PhaseGoalTransition> {
		let selected_repo_gate = orchestrator::select_repo_gate_for_worktree(
			self.workflow.frontmatter().execution(),
			&self.issue_run.worktree.path,
		);

		orchestrator::write_run_operation_marker_best_effort(
			&self.issue_run.worktree.path,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			RUN_OPERATION_REPO_GATE,
		);

		match orchestrator::run_repo_gate_commands_allow_owned_tracked_rewrites(
			selected_repo_gate.canonicalize_commands(),
			selected_repo_gate.verify_commands(),
			&self.issue_run.worktree.path,
		) {
			Ok(repo_gate_outcome) =>
				self.continue_after_repo_gate_pass(phase, &selected_repo_gate, &repo_gate_outcome),
			Err(error) => self.continue_after_repo_gate_error(phase, error),
		}
	}
}
