use crate::state::sqlite_store::persist::{self, Result, StateData, Transaction};

pub(in crate::state::sqlite_store) fn persist_protocol_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for (run_id, events) in &state.events {
		for event in events {
			transaction.execute(
				"INSERT OR REPLACE INTO protocol_events (
						run_id, sequence_number, event_type, payload_sha256, created_at,
						created_at_unix
					) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
				persist::params![
					run_id,
					event.sequence_number,
					&event.event_type,
					&event.payload_sha256,
					&event.created_at,
					event.created_at_unix,
				],
			)?;
		}
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_linear_execution_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.linear_execution_events.values() {
		let payload_json = serde_json::to_string(&record.record)?;

		transaction.execute(
			"INSERT OR REPLACE INTO linear_execution_events (
					idempotency_key, service_id, issue_id, event_type, event_timestamp,
					event_unix, payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			persist::params![
				&record.record.idempotency_key,
				&record.record.service_id,
				&record.record.issue_id,
				&record.record.event_type,
				&record.record.event_timestamp,
				record.event_unix,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn persist_private_execution_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in &state.private_execution_events {
		let payload_json = serde_json::to_string(&record.payload)?;

		transaction.execute(
			"INSERT OR REPLACE INTO private_execution_events (
					record_id, project_id, issue_id, run_id, attempt_number, event_type,
					payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			persist::params![
				record.record_id,
				&record.project_id,
				&record.issue_id,
				&record.run_id,
				record.attempt_number,
				&record.event_type,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;
	}

	Ok(())
}
