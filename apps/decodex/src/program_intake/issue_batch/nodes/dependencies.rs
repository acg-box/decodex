use std::collections::BTreeMap;

use crate::{
	execution_program::{ExecutionDependencySnapshot, ExecutionProgramDependency},
	prelude::Result,
	tracker::TrackerIssue,
};

pub(in crate::program_intake) fn issue_dependencies(
	issue: &TrackerIssue,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<Vec<ExecutionProgramDependency>> {
	let mut dependencies = BTreeMap::new();

	for blocker in &issue.blockers {
		let dependency_id = supplied_dependency_id(&blocker.identifier, supplied_node_ids);

		dependencies
			.entry(dependency_id.clone())
			.or_insert(ExecutionProgramDependency::new(dependency_id)?);
	}

	Ok(dependencies.into_values().collect())
}

pub(in crate::program_intake) fn dependency_snapshots_for(
	issue: &TrackerIssue,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<Vec<ExecutionDependencySnapshot>> {
	let mut snapshots = BTreeMap::new();

	for blocker in &issue.blockers {
		let dependency_id = supplied_dependency_id(&blocker.identifier, supplied_node_ids);
		let snapshot = ExecutionDependencySnapshot::tracker_state(
			dependency_id.clone(),
			blocker.state.name.clone(),
		)?;

		snapshots.entry(dependency_id).or_insert(snapshot);
	}

	Ok(snapshots.into_values().collect())
}

fn supplied_dependency_id(
	identifier: &str,
	supplied_node_ids: &BTreeMap<String, String>,
) -> String {
	supplied_node_ids.get(identifier).cloned().unwrap_or_else(|| identifier.to_owned())
}
