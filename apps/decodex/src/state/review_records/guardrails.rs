use crate::state::runtime_row_parsers;
use crate::{
	prelude::Result,
	state::{
		LoopGuardrailCheckpoint, LoopGuardrailCheckpointInput, StateStore,
		runtime_records::{LoopGuardrailKey, LoopGuardrailRuntimeRecord},
	},
};

impl StateStore {
	/// Record one loop-guardrail observation and return its consecutive count.
	pub(crate) fn observe_loop_guardrail_checkpoint(
		&self,
		input: LoopGuardrailCheckpointInput<'_>,
	) -> Result<LoopGuardrailCheckpoint> {
		let now = runtime_row_parsers::timestamp_parts();
		let key = LoopGuardrailKey::new(input.project_id, input.issue_id, input.reason);
		let mut state = self.lock()?;
		let previous = state.loop_guardrail_checkpoints.get(&key);
		let consecutive_count = previous.map_or(1, |record| {
			if record.fingerprint == input.fingerprint {
				record.consecutive_count.saturating_add(1)
			} else {
				1
			}
		});
		let record = LoopGuardrailRuntimeRecord {
			project_id: input.project_id.to_owned(),
			issue_id: input.issue_id.to_owned(),
			reason: input.reason.to_owned(),
			fingerprint: input.fingerprint.to_owned(),
			run_id: input.run_id.to_owned(),
			attempt_number: input.attempt_number,
			consecutive_count,
			details_json: input.details_json.to_owned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.loop_guardrail_checkpoints.insert(key, record.clone());
		self.persist_runtime_state_locked(&state)?;

		Ok(record.as_public())
	}

	/// Read one loop-guardrail checkpoint by project, issue, and reason.
	#[cfg(test)]
	pub(crate) fn loop_guardrail_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<Option<LoopGuardrailCheckpoint>> {
		let state = self.lock()?;
		let key = LoopGuardrailKey::new(project_id, issue_id, reason);

		Ok(state.loop_guardrail_checkpoints.get(&key).map(LoopGuardrailRuntimeRecord::as_public))
	}

	/// Clear loop-guardrail checkpoints for one issue.
	pub(crate) fn clear_loop_guardrail_checkpoints_for_issue(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<()> {
		let mut state = self.lock()?;

		state
			.loop_guardrail_checkpoints
			.retain(|key, _record| key.project_id != project_id || key.issue_id != issue_id);

		self.delete_loop_guardrail_checkpoints_for_issue_locked(project_id, issue_id)
	}

	/// Clear one loop-guardrail checkpoint reason for one issue.
	pub(crate) fn clear_loop_guardrail_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<()> {
		let key = LoopGuardrailKey::new(project_id, issue_id, reason);
		let mut state = self.lock()?;

		state.loop_guardrail_checkpoints.remove(&key);

		self.delete_loop_guardrail_checkpoint_locked(project_id, issue_id, reason)
	}
}
