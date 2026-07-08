use serde_json::Value;

use crate::orchestrator::{
	self, IssueDispatchMode, IssueRunPlan, PhaseGoalController, PhaseGoalKind, PhaseGoalSpec,
	PhaseGoalTransition, Result, ServiceConfig, StateStore, WorkflowDocument,
	execution_phase_goal::{
		acceptance,
		recovery::{self, phase_goal_kind_from_str},
	},
};

pub(crate) struct RepoGatePhaseGoalController<'a> {
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) state_store: &'a StateStore,
	pub(crate) issue_run: &'a IssueRunPlan,
}
impl RepoGatePhaseGoalController<'_> {
	fn initial_phase_goal_kind(&self) -> PhaseGoalKind {
		match self.issue_run.dispatch_mode {
			IssueDispatchMode::Normal | IssueDispatchMode::Program | IssueDispatchMode::Retry =>
				PhaseGoalKind::ImplementToValidationReady,
			IssueDispatchMode::ReviewRepair => PhaseGoalKind::RepairAcceptedReviewFindings,
			IssueDispatchMode::Closeout => PhaseGoalKind::HandoffEvidence,
		}
	}

	fn latest_persisted_phase_goal(&self) -> Result<Option<PhaseGoalKind>> {
		let events = self.state_store.list_private_execution_events(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
		)?;

		Ok(events
			.iter()
			.rev()
			.filter(|event| event.event_type() == "phase_goal_next")
			.find_map(|event| event.payload().get("phase").and_then(Value::as_str))
			.and_then(phase_goal_kind_from_str))
	}

	fn latest_cross_attempt_phase_goal(&self) -> Result<Option<PhaseGoalKind>> {
		if !matches!(
			self.issue_run.dispatch_mode,
			IssueDispatchMode::Normal | IssueDispatchMode::Program | IssueDispatchMode::Retry
		) {
			return Ok(None);
		}

		recovery::latest_open_issue_phase_goal_before_attempt(
			self.project,
			self.state_store,
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
		)
	}
}

impl PhaseGoalController for RepoGatePhaseGoalController<'_> {
	fn initial_phase_goal(&self) -> Result<Option<PhaseGoalSpec>> {
		if let Some(phase) = self.latest_persisted_phase_goal()? {
			return Ok(Some(self.phase_goal_spec(phase, None)));
		}
		if let Some(phase) = self.latest_cross_attempt_phase_goal()? {
			return Ok(Some(self.phase_goal_spec(phase, None)));
		}

		Ok(Some(self.phase_goal_spec(self.initial_phase_goal_kind(), None)))
	}

	fn phase_goal_completed(&self, phase: PhaseGoalKind) -> Result<PhaseGoalTransition> {
		match phase {
			PhaseGoalKind::ReviewRepairEvidence | PhaseGoalKind::HandoffEvidence => {
				self.record_phase_goal_transition(
					phase,
					acceptance::phase_terminal_goal_complete_signal(phase),
					orchestrator::json!({ "terminalPathRequired": true }),
				)?;

				Ok(PhaseGoalTransition::CompleteRun)
			},
			PhaseGoalKind::ImplementToValidationReady
			| PhaseGoalKind::RepairValidationFailures
			| PhaseGoalKind::RepairAcceptedReviewFindings => self.validate_phase_goal_output(phase),
		}
	}
}

pub(crate) fn build_phase_goal_controller<'a>(
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
) -> RepoGatePhaseGoalController<'a> {
	RepoGatePhaseGoalController { project, workflow, state_store, issue_run }
}
