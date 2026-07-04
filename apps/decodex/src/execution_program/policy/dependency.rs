use crate::{
	execution_program::{model::ExecutionQueueIntent, validation},
	prelude::Result,
};

/// Runtime dependency observation used by readiness evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionDependencySnapshot {
	pub(in crate::execution_program) dependency_id: String,
	pub(in crate::execution_program) tracker_state: Option<String>,
	pub(in crate::execution_program) queue_intent: Option<ExecutionQueueIntent>,
}
impl ExecutionDependencySnapshot {
	/// Observe a dependency through a tracker state.
	pub(crate) fn tracker_state(
		dependency_id: impl Into<String>,
		state: impl Into<String>,
	) -> Result<Self> {
		let snapshot = Self {
			dependency_id: dependency_id.into(),
			tracker_state: Some(state.into()),
			queue_intent: None,
		};

		snapshot.validate()?;

		Ok(snapshot)
	}

	fn validate(&self) -> Result<()> {
		validation::validate_required(
			"execution dependency snapshot.dependency_id",
			&self.dependency_id,
		)?;

		validation::validate_optional(
			"execution dependency snapshot.tracker_state",
			self.tracker_state.as_deref(),
		)
	}
}
