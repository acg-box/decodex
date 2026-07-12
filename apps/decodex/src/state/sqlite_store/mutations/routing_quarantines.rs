use rusqlite::OptionalExtension as _;

use crate::{
	lane_authority::{AuthorityEventDraft, RoutingQuarantine},
	prelude::{Result, eyre},
	state::sqlite_store::{
		SqliteStateStore,
		mutations::{authority_events::append_authority_event_in_transaction, params},
	},
};

impl SqliteStateStore {
	pub(in crate::state) fn record_routing_quarantine(
		&mut self,
		quarantine: &RoutingQuarantine,
		event: Option<AuthorityEventDraft>,
	) -> Result<()> {
		let payload = serde_json::to_string(quarantine)?;
		let transaction = self.connection.transaction()?;
		let inserted = transaction.execute(
			"INSERT OR IGNORE INTO routing_quarantines
			 (tracker_issue_id, epoch, payload_json, created_at_unix)
			 VALUES (?1, ?2, ?3, ?4)",
			params![
				&quarantine.tracker_issue_id,
				i64::try_from(quarantine.epoch)?,
				&payload,
				crate::state::timestamp_parts().unix,
			],
		)?;
		if inserted == 0 {
			let existing: String = transaction.query_row(
				"SELECT payload_json FROM routing_quarantines WHERE tracker_issue_id = ?1",
				[&quarantine.tracker_issue_id],
				|row| row.get(0),
			)?;
			if existing != payload {
				eyre::bail!("routing_quarantine_authority_collision");
			}
		} else if let Some(event) = event {
			append_authority_event_in_transaction(&transaction, event)?;
		}
		transaction.commit()?;
		Ok(())
	}

	pub(in crate::state) fn routing_quarantine(
		&self,
		tracker_issue_id: &str,
	) -> Result<Option<RoutingQuarantine>> {
		let payload = self
			.connection
			.query_row(
				"SELECT payload_json FROM routing_quarantines WHERE tracker_issue_id = ?1",
				[tracker_issue_id],
				|row| row.get::<_, String>(0),
			)
			.optional()?;
		payload.map(|payload| serde_json::from_str(&payload).map_err(Into::into)).transpose()
	}
}
