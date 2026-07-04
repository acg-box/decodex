use rusqlite::Connection;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	maintenance::reports::RuntimeProtocolCandidate,
	prelude::{Result, eyre},
};

pub(in crate::maintenance::runtime) fn compact_protocol_events(
	connection: &mut Connection,
	candidates: &[RuntimeProtocolCandidate],
	generated_at: OffsetDateTime,
) -> Result<()> {
	let generated_at_text = generated_at.format(&Rfc3339)?;
	let generated_at_unix = generated_at.unix_timestamp();
	let transaction = connection.transaction()?;

	for candidate in candidates {
		transaction.execute(
			"INSERT OR REPLACE INTO protocol_event_summaries (
				run_id, event_count, last_sequence_number, last_event_type, last_event_at,
				last_event_at_unix, compacted_at, compacted_at_unix
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			rusqlite::params![
				&candidate.run_id,
				i64::try_from(candidate.event_count).map_err(|_error| {
					eyre::eyre!(
						"Protocol event count for run `{}` overflowed i64.",
						candidate.run_id
					)
				})?,
				candidate.last_sequence_number,
				candidate.last_event_type.as_deref(),
				candidate.last_event_at.as_deref(),
				candidate.last_event_at_unix,
				&generated_at_text,
				generated_at_unix,
			],
		)?;
		transaction.execute(
			"DELETE FROM protocol_events WHERE run_id = ?1",
			rusqlite::params![&candidate.run_id],
		)?;
	}

	transaction.commit()?;

	Ok(())
}
