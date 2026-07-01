use rusqlite::{self, Error, Row, types::Type};
use serde::de::DeserializeOwned;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	prelude::eyre,
	state::{ProtocolEventRecord, ProtocolEventSummaryRecord, TimestampParts},
	tracker::records::LinearExecutionEventRecord,
};

pub(in crate::state) fn timestamp_parts() -> TimestampParts {
	let now = OffsetDateTime::now_utc();

	TimestampParts {
		text: now.format(&Rfc3339).expect("timestamp formatting should succeed"),
		unix: now.unix_timestamp(),
	}
}

pub(in crate::state) fn parse_linear_execution_event_unix(
	record: &LinearExecutionEventRecord,
) -> Option<i64> {
	OffsetDateTime::parse(&record.event_timestamp, &Rfc3339)
		.ok()
		.map(|timestamp| timestamp.unix_timestamp())
}

pub(in crate::state) fn validate_private_execution_event_inputs(
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	event_type: &str,
) -> crate::prelude::Result<()> {
	if project_id.trim().is_empty() {
		eyre::bail!("Private execution event project_id must not be empty.");
	}
	if issue_id.trim().is_empty() {
		eyre::bail!("Private execution event issue_id must not be empty.");
	}
	if run_id.trim().is_empty() {
		eyre::bail!("Private execution event run_id must not be empty.");
	}
	if attempt_number < 1 {
		eyre::bail!("Private execution event attempt_number must be greater than zero.");
	}
	if event_type.trim().is_empty() {
		eyre::bail!("Private execution event event_type must not be empty.");
	}

	Ok(())
}

pub(in crate::state) fn protocol_event_summary_from_events(
	events: &[ProtocolEventRecord],
) -> ProtocolEventSummaryRecord {
	let mut summary = ProtocolEventSummaryRecord::default();

	for event in events {
		summary.record_event(event);
	}

	summary
}

pub(in crate::state) fn protocol_event_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<ProtocolEventRecord, Error> {
	Ok(ProtocolEventRecord {
		sequence_number: row.get(0)?,
		event_type: row.get(1)?,
		payload_sha256: row.get(2)?,
		created_at: row.get(3)?,
		created_at_unix: row.get(4)?,
	})
}

pub(in crate::state) fn sqlite_bool_value(value: bool) -> i64 {
	if value { 1 } else { 0 }
}

pub(in crate::state::runtime_row_parsers) fn optional_json_from_row<T>(
	row: &Row<'_>,
	index: usize,
) -> std::result::Result<Option<T>, Error>
where
	T: DeserializeOwned,
{
	let value: Option<String> = row.get(index)?;

	value
		.map(|value| {
			serde_json::from_str(&value).map_err(|error| {
				Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
			})
		})
		.transpose()
}

pub(in crate::state::runtime_row_parsers) fn sqlite_bool(
	row: &Row<'_>,
	index: usize,
) -> std::result::Result<bool, Error> {
	Ok(row.get::<_, i64>(index)? != 0)
}
