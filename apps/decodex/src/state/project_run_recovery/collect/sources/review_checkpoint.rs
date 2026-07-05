use std::collections::{BTreeMap, HashSet};

use crate::state::{
	StateData,
	project_run_recovery::{
		candidate::{self, ProjectRunRecoveryCandidate},
		collect::sources::scope,
	},
};

pub(in crate::state::project_run_recovery::collect) fn collect_review_checkpoint_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) {
	for checkpoint in state.review_policy_checkpoints.values() {
		if scope::project_recovery_record_is_out_of_scope(
			project_id,
			issue_id,
			recorded_run_ids,
			&checkpoint.project_id,
			&checkpoint.issue_id,
			&checkpoint.run_id,
		) {
			continue;
		}

		candidate::upsert_project_run_recovery_candidate(
			candidates,
			project_id,
			&checkpoint.issue_id,
			&checkpoint.run_id,
			checkpoint.attempt_number,
			"recovered",
			checkpoint.updated_at.clone(),
			checkpoint.updated_at_unix,
			format!("review_policy_checkpoint:{}:{}", checkpoint.phase, checkpoint.status),
		);
	}
}
