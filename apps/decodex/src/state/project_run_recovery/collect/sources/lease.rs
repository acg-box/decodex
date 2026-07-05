use std::collections::{BTreeMap, HashSet};

use crate::state::{
	StateData,
	project_run_recovery::candidate::{self, ProjectRunRecoveryCandidate},
};

pub(in crate::state::project_run_recovery::collect) fn collect_lease_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) {
	for lease in state.leases.values() {
		if lease.project_id != project_id
			|| issue_id.is_some_and(|issue_id| lease.issue_id != issue_id)
			|| recorded_run_ids.contains(&lease.run_id)
		{
			continue;
		}

		if let Some(summary) = state.run_activity_summaries.get(&lease.run_id) {
			candidate::upsert_project_run_recovery_candidate(
				candidates,
				project_id,
				&lease.issue_id,
				&lease.run_id,
				summary.attempt_number,
				"running",
				summary.updated_at.clone(),
				summary.updated_at_unix,
				String::from("active_lease+run_activity_summary"),
			);
		}
	}
}
