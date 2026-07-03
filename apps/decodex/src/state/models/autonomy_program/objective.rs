use crate::autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState};

/// SQLite-backed Objective Contract authority version retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyObjectiveRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) objective: AutonomyObjectiveContract,
	pub(in crate::state) state: AutonomyObjectiveState,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl AutonomyObjectiveRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn objective(&self) -> &AutonomyObjectiveContract {
		&self.objective
	}

	pub(crate) fn objective_id(&self) -> &str {
		self.objective.id()
	}

	pub(crate) fn version(&self) -> u64 {
		self.objective.version()
	}

	pub(crate) fn state(&self) -> AutonomyObjectiveState {
		self.state
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}
