use std::collections::{BTreeMap, BTreeSet, HashSet};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
	ProjectRunStatus, RUN_CONTROL_CHANNEL_STATUS_ACTIVE, Result, RunControlChannelRecord,
	StateData, read_run_activity_marker_snapshot, timestamp_parts,
};

#[derive(Clone, Debug)]
pub(super) struct ProjectRunRecoveryCandidate {
	project_id: String,
	issue_id: String,
	run_id: String,
	attempt_number: i64,
	status: String,
	thread_id: Option<String>,
	turn_id: Option<String>,
	updated_at: String,
	updated_at_unix: i64,
	evidence: BTreeSet<String>,
	gaps: BTreeSet<String>,
}
impl ProjectRunRecoveryCandidate {
	fn new(input: ProjectRunRecoveryCandidateInput<'_>) -> Self {
		let mut evidence_set = BTreeSet::new();

		evidence_set.insert(input.evidence);
		Self {
			project_id: input.project_id.to_owned(),
			issue_id: input.issue_id.to_owned(),
			run_id: input.run_id.to_owned(),
			attempt_number: input.attempt_number,
			status: input.status.to_owned(),
			thread_id: None,
			turn_id: None,
			updated_at: input.updated_at,
			updated_at_unix: input.updated_at_unix,
			evidence: evidence_set,
			gaps: BTreeSet::new(),
		}
	}

	pub(super) fn run_id(&self) -> &str {
		&self.run_id
	}

	fn merge(
		&mut self,
		attempt_number: i64,
		status: &str,
		updated_at: String,
		updated_at_unix: i64,
		evidence: String,
	) {
		if self.attempt_number != attempt_number {
			self.gaps.insert(format!(
				"conflicting_attempt_number:{}:{}",
				self.attempt_number, attempt_number
			));
		}
		if project_run_recovery_status_rank(status) > project_run_recovery_status_rank(&self.status)
		{
			self.status = status.to_owned();
		}
		if updated_at_unix >= self.updated_at_unix {
			self.updated_at = updated_at;
			self.updated_at_unix = updated_at_unix;
		}

		self.evidence.insert(evidence);
	}

	fn merge_thread_fields(&mut self, thread_id: Option<&str>, turn_id: Option<&str>) {
		if self.thread_id.is_none() {
			self.thread_id = thread_id.map(str::to_owned);
		}
		if self.turn_id.is_none() {
			self.turn_id = turn_id.map(str::to_owned);
		}
	}
}

struct ProjectRunRecoveryCandidateInput<'a> {
	project_id: &'a str,
	issue_id: &'a str,
	run_id: &'a str,
	attempt_number: i64,
	status: &'a str,
	updated_at: String,
	updated_at_unix: i64,
	evidence: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProjectRunListingMode {
	AllowMarkerIdentityPersistence,
	ReadOnly,
}

fn project_run_recovery_status_rank(status: &str) -> u8 {
	match status {
		"running" => 4,
		"starting" => 3,
		"recovered" => 2,
		_ => 1,
	}
}

pub(super) fn project_lease_run_ids(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
) -> Vec<String> {
	state
		.leases
		.values()
		.filter(|lease| lease.project_id == project_id)
		.filter(|lease| issue_id.is_none_or(|issue_id| lease.issue_id == issue_id))
		.map(|lease| lease.run_id.clone())
		.collect()
}

pub(super) fn project_run_recovery_candidates(
	state: &StateData,
	project_id: &str,
	issue_id: Option<&str>,
) -> Result<Vec<ProjectRunRecoveryCandidate>> {
	let recorded_run_ids = state.run_attempts.keys().cloned().collect::<HashSet<_>>();
	let mut candidates = BTreeMap::<String, ProjectRunRecoveryCandidate>::new();

	collect_control_channel_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	);
	collect_private_event_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	);
	collect_review_checkpoint_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	);
	collect_lease_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	);
	collect_worktree_marker_recovery_candidates(
		state,
		project_id,
		issue_id,
		&recorded_run_ids,
		&mut candidates,
	)?;
	finalize_project_run_recovery_candidates(state, &mut candidates);

	Ok(candidates.into_values().collect())
}

fn collect_control_channel_recovery_candidates(
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

		upsert_project_run_recovery_candidate(
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

fn collect_private_event_recovery_candidates(
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

		upsert_project_run_recovery_candidate(
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

fn collect_review_checkpoint_recovery_candidates(
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

		upsert_project_run_recovery_candidate(
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

fn collect_lease_recovery_candidates(
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
			upsert_project_run_recovery_candidate(
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

fn collect_worktree_marker_recovery_candidates(
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

		let marker = match read_run_activity_marker_snapshot(&mapping.worktree_path) {
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
			.unwrap_or_else(|| timestamp_parts().unix);
		let updated_at = timestamp_text_from_unix(updated_at_unix);
		let candidate = upsert_project_run_recovery_candidate(
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

fn finalize_project_run_recovery_candidates(
	state: &StateData,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) {
	for candidate in candidates.values_mut() {
		if let Some(summary) = state.run_activity_summaries.get(&candidate.run_id) {
			let status = candidate.status.clone();

			candidate.merge(
				summary.attempt_number,
				&status,
				summary.updated_at.clone(),
				summary.updated_at_unix,
				String::from("run_activity_summary"),
			);
		}

		let event_summary = state.protocol_event_summary(&candidate.run_id);

		if event_summary.event_count > 0 {
			candidate.evidence.insert(format!("protocol_events:{}", event_summary.event_count));
		}
		if !state.run_activity_summaries.contains_key(&candidate.run_id)
			&& event_summary.event_count == 0
		{
			candidate.gaps.insert(String::from("no_activity_or_protocol_summary"));
		}
	}
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

pub(super) fn project_run_recovery_candidate_counts_as_project_run(
	state: &StateData,
	candidate: &ProjectRunRecoveryCandidate,
) -> bool {
	let has_active_lease = state.leases.get(&candidate.issue_id).is_some_and(|lease| {
		lease.project_id == candidate.project_id && lease.run_id == candidate.run_id
	});
	let has_control_channel =
		state.control_channels.get(&candidate.run_id).is_some_and(|channel| {
			channel.project_id == candidate.project_id
				&& channel.issue_id == candidate.issue_id
				&& channel.attempt_number == candidate.attempt_number
		});
	let has_worktree_marker =
		candidate.evidence.iter().any(|evidence| evidence == "worktree_activity_marker");
	let issue_has_recorded_attempt =
		state.run_attempts.values().any(|attempt| attempt.issue_id == candidate.issue_id);

	has_active_lease || has_control_channel || (has_worktree_marker && !issue_has_recorded_attempt)
}

#[allow(clippy::too_many_arguments)]
fn upsert_project_run_recovery_candidate<'a>(
	candidates: &'a mut BTreeMap<String, ProjectRunRecoveryCandidate>,
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	status: &str,
	updated_at: String,
	updated_at_unix: i64,
	evidence: String,
) -> &'a mut ProjectRunRecoveryCandidate {
	candidates
		.entry(run_id.to_owned())
		.and_modify(|candidate| {
			candidate.merge(
				attempt_number,
				status,
				updated_at.clone(),
				updated_at_unix,
				evidence.clone(),
			);
		})
		.or_insert_with(|| {
			ProjectRunRecoveryCandidate::new(ProjectRunRecoveryCandidateInput {
				project_id,
				issue_id,
				run_id,
				attempt_number,
				status,
				updated_at,
				updated_at_unix,
				evidence,
			})
		})
}

pub(super) fn project_run_status_from_recovery_candidate(
	state: &StateData,
	candidate: &ProjectRunRecoveryCandidate,
) -> Option<ProjectRunStatus> {
	if state.run_attempts.contains_key(&candidate.run_id) {
		return None;
	}

	let worktree = state.worktrees.get(&candidate.issue_id);
	let run_lease = state.leases.get(&candidate.issue_id).is_some_and(|lease| {
		lease.project_id == candidate.project_id && lease.run_id == candidate.run_id
	});
	let event_summary = state.protocol_event_summary(&candidate.run_id);
	let run_activity_summary = state.run_activity_summaries.get(&candidate.run_id);
	let control_channel = state
		.control_channels
		.get(&candidate.run_id)
		.filter(|channel| {
			channel.project_id == candidate.project_id
				&& channel.issue_id == candidate.issue_id
				&& channel.attempt_number == candidate.attempt_number
		})
		.map(RunControlChannelRecord::as_public);

	Some(ProjectRunStatus {
		run_id: candidate.run_id.clone(),
		issue_id: candidate.issue_id.clone(),
		attempt_number: candidate.attempt_number,
		status: candidate.status.clone(),
		thread_id: candidate.thread_id.clone(),
		turn_id: candidate.turn_id.clone(),
		updated_at: candidate.updated_at.clone(),
		updated_at_unix: candidate.updated_at_unix,
		branch_name: worktree.map(|mapping| mapping.branch_name.clone()),
		worktree_path: worktree.map(|mapping| mapping.worktree_path.clone()),
		run_lease,
		event_count: event_summary.event_count,
		last_event_type: event_summary.last_event_type,
		last_event_at: event_summary.last_event_at,
		last_event_at_unix: event_summary.last_event_at_unix,
		control_channel,
		child_agent_activity: run_activity_summary
			.and_then(|summary| summary.child_agent_activity.clone()),
		protocol_activity: run_activity_summary
			.and_then(|summary| summary.protocol_activity.clone()),
		recovery_source: String::from("recovered"),
		recovery_evidence: candidate.evidence.iter().cloned().collect(),
		recovery_gaps: candidate.gaps.iter().cloned().collect(),
	})
}

fn timestamp_text_from_unix(unix_epoch: i64) -> String {
	OffsetDateTime::from_unix_timestamp(unix_epoch)
		.ok()
		.and_then(|timestamp| timestamp.format(&Rfc3339).ok())
		.unwrap_or_else(|| timestamp_parts().text)
}
