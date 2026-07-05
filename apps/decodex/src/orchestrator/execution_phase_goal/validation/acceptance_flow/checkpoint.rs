use crate::{
	orchestrator::{Result, execution_phase_goal::controller::RepoGatePhaseGoalController},
	state::PrivateExecutionEvent,
};

impl RepoGatePhaseGoalController<'_> {
	pub(in crate::orchestrator::execution_phase_goal) fn latest_progress_checkpoint(
		&self,
	) -> Result<Option<PrivateExecutionEvent>> {
		let events = self.state_store.list_private_execution_events(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
		)?;

		Ok(events.into_iter().rev().find(|event| event.event_type() == "progress_checkpoint"))
	}
}
