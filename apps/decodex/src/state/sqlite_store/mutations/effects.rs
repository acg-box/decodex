use crate::prelude::{Result, eyre};
use crate::{
	lane_authority::LaneEffect,
	state::sqlite_store::{SqliteStateStore, mutations::params},
};

impl SqliteStateStore {
	pub(in crate::state) fn insert_lane_effect(&self, effect: &LaneEffect) -> Result<()> {
		let payload = serde_json::to_string(effect)?;
		let inserted = self.connection.execute(
			"INSERT OR IGNORE INTO lane_effects (
				effect_id, operation_id, ordinal, project_key, tracker_issue_id,
				journal_epoch, kind, payload_json, updated_at_unix
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())",
			params![
				effect.effect_id(),
				effect.operation_id(),
				i64::from(effect.ordinal()),
				effect.lane_id().project_key(),
				effect.lane_id().tracker_issue_id(),
				i64::try_from(effect.journal_epoch())?,
				effect.kind().registry_name(),
				payload,
			],
		)?;
		if inserted == 1 {
			return Ok(());
		}
		let existing = self.connection.query_row(
			"SELECT payload_json FROM lane_effects WHERE effect_id = ?1",
			params![effect.effect_id()],
			|row| row.get::<_, String>(0),
		)?;
		if existing != serde_json::to_string(effect)? {
			eyre::bail!("Immutable lane effect identity conflicts with existing journal state.");
		}
		Ok(())
	}

	pub(in crate::state) fn cas_lane_effect(
		&self,
		expected_journal_epoch: u64,
		effect: &LaneEffect,
	) -> Result<()> {
		let updated = self.connection.execute(
			"UPDATE lane_effects
			 SET journal_epoch = ?1, payload_json = ?2, updated_at_unix = unixepoch()
			 WHERE effect_id = ?3 AND journal_epoch = ?4",
			params![
				i64::try_from(effect.journal_epoch())?,
				serde_json::to_string(effect)?,
				effect.effect_id(),
				i64::try_from(expected_journal_epoch)?,
			],
		)?;
		if updated != 1 {
			eyre::bail!("Lane effect journal CAS rejected a stale writer.");
		}
		Ok(())
	}
}
