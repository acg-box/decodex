use std::collections::{BTreeMap, HashSet};

use crate::state::{
	StateData,
	project_run_recovery::{
		candidate::{self, ProjectRunRecoveryCandidate},
		collect::sources::scope,
	},
};

pub(in crate::state::project_run_recovery::collect) fn collect_private_event_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) {
	for record in &state.private_execution_events {
		if scope::project_recovery_record_is_out_of_scope(
			project_id,
			issue_id,
			recorded_run_ids,
			&record.project_id,
			&record.issue_id,
			&record.run_id,
		) {
			continue;
		}

		candidate::upsert_project_run_recovery_candidate(
			candidates,
			project_id,
			&record.issue_id,
			&record.run_id,
			record.attempt_number,
			"recovered",
			record.recorded_at.clone(),
			record.recorded_at_unix,
			format!("private_execution_event:{}", record.event_type),
		);
	}
}
