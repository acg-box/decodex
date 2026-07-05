use std::collections::{BTreeMap, HashSet};

use crate::state::{
	self, Result, StateData,
	project_run_recovery::{
		candidate::{self, ProjectRunRecoveryCandidate},
		time,
	},
	runtime_row_parsers,
};

pub(in crate::state::project_run_recovery::collect) fn collect_worktree_marker_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) -> Result<()> {
	for mapping in state.worktrees.values() {
		if mapping.project_id != project_id
			|| issue_id.is_some_and(|issue_id| mapping.issue_id != issue_id)
		{
			continue;
		}

		let marker = match state::read_run_activity_marker_snapshot(&mapping.worktree_path) {
			Ok(Some(marker)) => marker,
			Ok(None) | Err(_) => continue,
		};

		if recorded_run_ids.contains(marker.run_id()) {
			continue;
		}

		let updated_at_unix = marker
			.last_activity_unix_epoch()
			.or_else(|| marker.last_protocol_activity_unix_epoch())
			.or_else(|| marker.last_progress_unix_epoch())
			.unwrap_or_else(|| runtime_row_parsers::timestamp_parts().unix);
		let updated_at = time::timestamp_text_from_unix(updated_at_unix);
		let candidate = candidate::upsert_project_run_recovery_candidate(
			candidates,
			project_id,
			&mapping.issue_id,
			marker.run_id(),
			marker.attempt_number(),
			"running",
			updated_at,
			updated_at_unix,
			String::from("worktree_activity_marker"),
		);

		candidate.merge_thread_fields(marker.thread_id(), marker.turn_id());
	}

	Ok(())
}
