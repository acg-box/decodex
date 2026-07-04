use std::collections::BTreeMap;

use crate::execution_program::{ExecutionConflictDomain, ExecutionDependencySnapshot};

/// Runtime context supplied to readiness evaluation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExecutionProgramReadinessContext {
	dependency_snapshots: Vec<ExecutionDependencySnapshot>,
	pub(in crate::execution_program) occupied_conflict_domains: Vec<ExecutionConflictDomain>,
	pub(in crate::execution_program) active_issue_ids: Vec<String>,
}
impl ExecutionProgramReadinessContext {
	/// Build an empty readiness context.
	pub(crate) fn new() -> Self {
		Self::default()
	}

	/// Add dependency observations.
	pub(crate) fn with_dependency_snapshots(
		mut self,
		snapshots: impl IntoIterator<Item = ExecutionDependencySnapshot>,
	) -> Self {
		self.dependency_snapshots = snapshots.into_iter().collect();

		self
	}

	/// Add conflict domains already occupied by active or retained work.
	pub(crate) fn with_occupied_conflict_domains(
		mut self,
		domains: impl IntoIterator<Item = ExecutionConflictDomain>,
	) -> Self {
		self.occupied_conflict_domains = domains.into_iter().collect();

		self
	}

	/// Add mapped Linear issues already owned by a live run claim.
	pub(crate) fn with_active_issue_ids(
		mut self,
		issue_ids: impl IntoIterator<Item = impl Into<String>>,
	) -> Self {
		self.active_issue_ids = issue_ids.into_iter().map(Into::into).collect();

		self
	}

	pub(in crate::execution_program) fn dependency_lookup(
		&self,
	) -> BTreeMap<&str, &ExecutionDependencySnapshot> {
		self.dependency_snapshots
			.iter()
			.map(|snapshot| (snapshot.dependency_id.as_str(), snapshot))
			.collect()
	}
}
