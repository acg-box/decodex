use crate::{
	prelude::Result,
	state::{IssueLease, StateStore, store},
};

impl StateStore {
	/// Retarget runtime records from a visible issue identifier to the canonical tracker id.
	pub fn canonicalize_issue_identity(
		&self,
		previous_issue_id: &str,
		canonical_issue_id: &str,
	) -> Result<()> {
		if previous_issue_id == canonical_issue_id {
			return Ok(());
		}

		let mut state = self.lock_without_refresh()?;

		if let Some(mut lease) = state.leases.remove(previous_issue_id) {
			lease.issue_id = canonical_issue_id.to_owned();

			state.leases.entry(canonical_issue_id.to_owned()).or_insert(lease);
		}
		if let Some(mut mapping) = state.worktrees.remove(previous_issue_id) {
			mapping.issue_id = canonical_issue_id.to_owned();

			state.worktrees.entry(canonical_issue_id.to_owned()).or_insert(mapping);
		}

		store::retarget_review_lifecycle_issue(
			&mut state.review_lifecycle_records,
			previous_issue_id,
			canonical_issue_id,
		);
		store::retarget_review_policy_issue(
			&mut state.review_policy_checkpoints,
			previous_issue_id,
			canonical_issue_id,
		);
		store::retarget_evidence_artifact_issue(
			&mut state.evidence_artifacts,
			previous_issue_id,
			canonical_issue_id,
		);
		store::retarget_loop_guardrail_issue(
			&mut state.loop_guardrail_checkpoints,
			previous_issue_id,
			canonical_issue_id,
		);

		if let Some(guard) = state.issue_claim_guards.remove(previous_issue_id) {
			state.issue_claim_guards.entry(canonical_issue_id.to_owned()).or_insert(guard);
		}
		if let Some(guard) = state.dispatch_slot_guards.remove(previous_issue_id) {
			state.dispatch_slot_guards.entry(canonical_issue_id.to_owned()).or_insert(guard);
		}

		for attempt in
			state.run_attempts.values_mut().filter(|attempt| attempt.issue_id == previous_issue_id)
		{
			attempt.issue_id = canonical_issue_id.to_owned();
		}
		for channel in state
			.control_channels
			.values_mut()
			.filter(|channel| channel.issue_id == previous_issue_id)
		{
			channel.issue_id = canonical_issue_id.to_owned();
		}
		for record in state
			.private_execution_events
			.iter_mut()
			.filter(|record| record.issue_id == previous_issue_id)
		{
			record.issue_id = canonical_issue_id.to_owned();
		}
		for record in state
			.decision_contracts
			.values_mut()
			.filter(|record| record.source_issue_id.as_deref() == Some(previous_issue_id))
		{
			record.source_issue_id = Some(canonical_issue_id.to_owned());
		}
		for record in state
			.program_issue_mappings
			.values_mut()
			.filter(|record| record.issue_id == previous_issue_id)
		{
			record.issue_id = canonical_issue_id.to_owned();
		}

		self.retarget_issue_identity_locked(previous_issue_id, canonical_issue_id)
	}

	/// Create or replace the run lease for one issue.
	pub fn upsert_lease(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		issue_state: &str,
	) -> Result<()> {
		let lease = IssueLease {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			run_id: run_id.to_owned(),
			issue_state: issue_state.to_owned(),
		};
		let mut state = self.lock_without_refresh()?;

		state.leases.insert(issue_id.to_owned(), lease.clone());
		state.remember_run_project(project_id, issue_id, Some(run_id));

		self.upsert_lease_and_remember_run_project_locked(&lease)
	}
}
