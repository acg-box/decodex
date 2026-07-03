use crate::autonomy_objective::{
	AutonomyObjectiveAcceptance, AutonomyObjectiveContract, AutonomyObjectiveRejection,
	AutonomyObjectiveState, AutonomyObjectiveSupersession,
};

#[allow(dead_code)]
impl AutonomyObjectiveContract {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn id(&self) -> &str {
		&self.id
	}

	pub(crate) fn version(&self) -> u64 {
		self.version
	}

	pub(crate) fn state(&self) -> AutonomyObjectiveState {
		self.state
	}

	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn goals(&self) -> &[String] {
		&self.goals
	}

	pub(crate) fn non_goals(&self) -> &[String] {
		&self.non_goals
	}

	pub(crate) fn metrics(&self) -> &[String] {
		&self.metrics
	}

	pub(crate) fn allowed_surfaces(&self) -> &[String] {
		&self.allowed_surfaces
	}

	pub(crate) fn allowed_signal_kinds(&self) -> &[String] {
		&self.allowed_signal_kinds
	}

	pub(crate) fn validation_gates(&self) -> &[String] {
		&self.validation_gates
	}

	pub(crate) fn review_policy(&self) -> &str {
		&self.review_policy
	}

	pub(crate) fn acceptance(&self) -> Option<&AutonomyObjectiveAcceptance> {
		self.acceptance.as_ref()
	}

	pub(crate) fn rejection(&self) -> Option<&AutonomyObjectiveRejection> {
		self.rejection.as_ref()
	}

	pub(crate) fn supersession(&self) -> Option<&AutonomyObjectiveSupersession> {
		self.supersession.as_ref()
	}
}
