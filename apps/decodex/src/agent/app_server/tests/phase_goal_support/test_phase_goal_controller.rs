use crate::{
	agent::app_server::{PhaseGoalController, PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition},
	prelude::Result,
};

pub(in crate::agent::app_server::tests) struct TestPhaseGoalController {
	initial_phase: PhaseGoalKind,
}
impl TestPhaseGoalController {
	pub(in crate::agent::app_server::tests) fn new(initial_phase: PhaseGoalKind) -> Self {
		Self { initial_phase }
	}
}

impl PhaseGoalController for TestPhaseGoalController {
	fn initial_phase_goal(&self) -> Result<Option<PhaseGoalSpec>> {
		Ok(Some(PhaseGoalSpec::new(self.initial_phase, "test phase goal", None)))
	}

	fn phase_goal_completed(&self, phase: PhaseGoalKind) -> Result<PhaseGoalTransition> {
		Ok(match phase {
			PhaseGoalKind::RepairAcceptedReviewFindings =>
				PhaseGoalTransition::Continue(PhaseGoalSpec::new(
					PhaseGoalKind::ReviewRepairEvidence,
					"prepare review repair evidence",
					None,
				)),
			PhaseGoalKind::ImplementToValidationReady | PhaseGoalKind::RepairValidationFailures =>
				PhaseGoalTransition::Continue(PhaseGoalSpec::new(
					PhaseGoalKind::HandoffEvidence,
					"prepare handoff evidence",
					None,
				)),
			PhaseGoalKind::ReviewRepairEvidence | PhaseGoalKind::HandoffEvidence =>
				PhaseGoalTransition::CompleteRun,
		})
	}
}
