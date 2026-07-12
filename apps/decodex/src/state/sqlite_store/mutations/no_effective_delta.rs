use crate::{
	lane_authority::NoEffectiveDeltaRecovery,
	prelude::{Result, eyre},
	state::sqlite_store::{SqliteStateStore, mutations::params},
};

impl SqliteStateStore {
	pub(in crate::state) fn insert_no_effective_delta_recovery(
		&self,
		recovery: &NoEffectiveDeltaRecovery,
	) -> Result<()> {
		recovery.validate()?;
		let payload = serde_json::to_string(recovery)?;
		let inserted = self.connection.execute(
			"INSERT OR IGNORE INTO no_effective_delta_recoveries (
				operation_id, project_key, tracker_issue_id, ordinal, payload_json, created_at_unix
			) VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())",
			params![
				recovery.operation_id(),
				recovery.lane_id().project_key(),
				recovery.lane_id().tracker_issue_id(),
				i64::from(recovery.ordinal()),
				payload,
			],
		)?;
		if inserted == 1 {
			return Ok(());
		}
		let existing = self.connection.query_row(
			"SELECT payload_json FROM no_effective_delta_recoveries WHERE operation_id = ?1",
			params![recovery.operation_id()],
			|row| row.get::<_, String>(0),
		)?;
		if existing != serde_json::to_string(recovery)? {
			eyre::bail!("Immutable no-effective-delta recovery conflicts with durable state.");
		}
		Ok(())
	}

	pub(in crate::state) fn replace_no_effective_delta_recovery(
		&self,
		current: &NoEffectiveDeltaRecovery,
		next: &NoEffectiveDeltaRecovery,
	) -> Result<()> {
		current.validate()?;
		next.validate()?;
		if current.operation_id() != next.operation_id()
			|| current.lane_id() != next.lane_id()
			|| current.source_attempt_number() != next.source_attempt_number()
			|| current.ordinal() != next.ordinal()
		{
			eyre::bail!("No-effective-delta recovery identity cannot change.");
		}
		let current_payload = serde_json::to_string(current)?;
		let next_payload = serde_json::to_string(next)?;
		let updated = self.connection.execute(
			"UPDATE no_effective_delta_recoveries SET payload_json = ?1
			 WHERE operation_id = ?2 AND payload_json = ?3",
			params![next_payload, next.operation_id(), current_payload],
		)?;
		if updated != 1 {
			eyre::bail!("No-effective-delta recovery compare-and-swap failed.");
		}
		Ok(())
	}
}
