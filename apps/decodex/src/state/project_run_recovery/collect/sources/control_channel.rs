use std::collections::{BTreeMap, HashSet};

use crate::state::{
	RUN_CONTROL_CHANNEL_STATUS_ACTIVE, StateData,
	project_run_recovery::{
		candidate::{self, ProjectRunRecoveryCandidate},
		collect::sources::scope,
	},
};

pub(in crate::state::project_run_recovery::collect) fn collect_control_channel_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) {
	for channel in state.control_channels.values() {
		if scope::project_recovery_record_is_out_of_scope(
			project_id,
			issue_id,
			recorded_run_ids,
			&channel.project_id,
			&channel.issue_id,
			&channel.run_id,
		) {
			continue;
		}

		let status = if channel.status == RUN_CONTROL_CHANNEL_STATUS_ACTIVE {
			"running"
		} else {
			"recovered"
		};

		candidate::upsert_project_run_recovery_candidate(
			candidates,
			project_id,
			&channel.issue_id,
			&channel.run_id,
			channel.attempt_number,
			status,
			channel.updated_at.clone(),
			channel.updated_at_unix,
			format!("run_control_channel:{}", channel.status),
		);
	}
}
