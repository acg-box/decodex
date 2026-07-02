use std::collections::{BTreeMap, BTreeSet};

use crate::state::StateData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::state) enum ProjectRunListingMode {
	AllowMarkerIdentityPersistence,
	ReadOnly,
}

#[derive(Clone, Debug)]
pub(in crate::state) struct ProjectRunRecoveryCandidate {
	pub(in crate::state::project_run_recovery) project_id: String,
	pub(in crate::state::project_run_recovery) issue_id: String,
	pub(in crate::state::project_run_recovery) run_id: String,
	pub(in crate::state::project_run_recovery) attempt_number: i64,
	pub(in crate::state::project_run_recovery) status: String,
	pub(in crate::state::project_run_recovery) thread_id: Option<String>,
	pub(in crate::state::project_run_recovery) turn_id: Option<String>,
	pub(in crate::state::project_run_recovery) updated_at: String,
	pub(in crate::state::project_run_recovery) updated_at_unix: i64,
	pub(in crate::state::project_run_recovery) evidence: BTreeSet<String>,
	pub(in crate::state::project_run_recovery) gaps: BTreeSet<String>,
}
impl ProjectRunRecoveryCandidate {
	#[allow(clippy::too_many_arguments)]
	fn new(
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		status: &str,
		updated_at: String,
		updated_at_unix: i64,
		evidence: String,
	) -> Self {
		let mut evidence_set = BTreeSet::new();

		evidence_set.insert(evidence);
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			run_id: run_id.to_owned(),
			attempt_number,
			status: status.to_owned(),
			thread_id: None,
			turn_id: None,
			updated_at,
			updated_at_unix,
			evidence: evidence_set,
			gaps: BTreeSet::new(),
		}
	}

	pub(in crate::state) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(in crate::state::project_run_recovery) fn merge(
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

	pub(in crate::state::project_run_recovery) fn merge_thread_fields(
		&mut self,
		thread_id: Option<&str>,
		turn_id: Option<&str>,
	) {
		if self.thread_id.is_none() {
			self.thread_id = thread_id.map(str::to_owned);
		}
		if self.turn_id.is_none() {
			self.turn_id = turn_id.map(str::to_owned);
		}
	}
}

pub(in crate::state) fn project_run_recovery_candidate_counts_as_project_run(
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
pub(in crate::state::project_run_recovery) fn upsert_project_run_recovery_candidate<'a>(
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
			ProjectRunRecoveryCandidate::new(
				project_id,
				issue_id,
				run_id,
				attempt_number,
				status,
				updated_at,
				updated_at_unix,
				evidence,
			)
		})
}

fn project_run_recovery_status_rank(status: &str) -> u8 {
	match status {
		"running" => 4,
		"starting" => 3,
		"recovered" => 2,
		_ => 1,
	}
}
