use std::collections::{BTreeMap, HashSet};

use crate::state::{
	self, RUN_CONTROL_CHANNEL_STATUS_ACTIVE, Result, StateData,
	project_run_recovery::{
		candidate::{self, ProjectRunRecoveryCandidate},
		time,
	},
	runtime_row_parsers,
};

pub(in crate::state::project_run_recovery::collect) fn collect_control_channel_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) {
	for channel in state.control_channels.values() {
		if project_recovery_record_is_out_of_scope(
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

pub(in crate::state::project_run_recovery::collect) fn collect_private_event_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) {
	for record in &state.private_execution_events {
		if project_recovery_record_is_out_of_scope(
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

pub(in crate::state::project_run_recovery::collect) fn collect_review_checkpoint_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) {
	for checkpoint in state.review_policy_checkpoints.values() {
		if project_recovery_record_is_out_of_scope(
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

fn project_recovery_record_is_out_of_scope(
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	record_project_id: &str,
	record_issue_id: &str,
	record_run_id: &str,
) -> bool {
	record_project_id != project_id
		|| issue_id.is_some_and(|issue_id| record_issue_id != issue_id)
		|| recorded_run_ids.contains(record_run_id)
}
