use crate::{
	prelude::{Result, eyre},
	state::{
		AutonomyRuntimePolicyReceiptInput, AutonomyRuntimePolicyRecord, StateStore,
		runtime_records::{AutonomyRuntimePolicyKey, AutonomyRuntimePolicyRuntimeRecord},
		runtime_row_parsers,
	},
};

impl StateStore {
	pub(crate) fn issue_autonomy_runtime_policy_receipt(
		&self,
		input: AutonomyRuntimePolicyReceiptInput<'_>,
	) -> Result<()> {
		input.candidate.validate()?;

		let now = runtime_row_parsers::timestamp_parts();

		if input.expires_at_unix <= now.unix || input.expires_at_unix - now.unix > 600 {
			eyre::bail!("runtime_policy_receipt_expiry_invalid");
		}

		let sqlite = self.sqlite.as_ref().ok_or_else(|| {
			eyre::eyre!("Runtime-policy operator receipts require the persistent runtime store.")
		})?;
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

		sqlite.issue_autonomy_runtime_policy_receipt(input)
	}

	pub(crate) fn accept_autonomy_runtime_policy_with_receipt(
		&self,
		project_id: &str,
		receipt_id: &str,
		principal: &str,
	) -> Result<AutonomyRuntimePolicyRecord> {
		let now = runtime_row_parsers::timestamp_parts();
		let sqlite = self.sqlite.as_ref().ok_or_else(|| {
			eyre::eyre!("Runtime-policy operator receipts require the persistent runtime store.")
		})?;
		let stored = {
			let mut sqlite =
				sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			sqlite.consume_autonomy_runtime_policy_receipt(
				project_id, receipt_id, principal, &now.text, now.unix,
			)?
		};
		let mut state = self.lock()?;

		state.autonomy_runtime_policies.insert(stored.key(), stored.clone());

		Ok(stored.as_public())
	}

	/// Accept one immutable autonomy runtime-policy authority record.
	#[cfg(test)]
	pub(crate) fn accept_autonomy_runtime_policy(
		&self,
		record: AutonomyRuntimePolicyRecord,
	) -> Result<AutonomyRuntimePolicyRecord> {
		record.validate()?;

		let candidate = AutonomyRuntimePolicyRuntimeRecord::from(record);
		let key = candidate.key();
		let mut state = self.lock()?;

		if let Some(existing) = state.autonomy_runtime_policies.get(&key) {
			existing.ensure_exact_replay(&candidate)?;

			return Ok(existing.as_public());
		}

		let stored = self.upsert_autonomy_runtime_policy_locked(&candidate)?;

		state.autonomy_runtime_policies.insert(stored.key(), stored.clone());

		Ok(stored.as_public())
	}

	/// Read one accepted policy by project, policy id, and policy version.
	#[allow(dead_code)]
	pub(crate) fn autonomy_runtime_policy(
		&self,
		project_id: &str,
		policy_id: &str,
		policy_version: &str,
	) -> Result<Option<AutonomyRuntimePolicyRecord>> {
		AutonomyRuntimePolicyRecord::validate_key(project_id, policy_id, policy_version)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.autonomy_runtime_policy(project_id, policy_id, policy_version)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.autonomy_runtime_policies
			.get(&AutonomyRuntimePolicyKey::new(project_id, policy_id, policy_version))
			.map(AutonomyRuntimePolicyRuntimeRecord::as_public))
	}
}
