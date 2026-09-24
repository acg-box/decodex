//! Requested execution selection bound to one acknowledged native turn.
use crate::{SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Requested settings acknowledged for one exact native turn, not inference telemetry.
pub struct ChiefTurnExecution {
	/// Exact requested native model.
	pub model: String,
	/// Requested effort; null is known unset.
	pub effort: Option<String>,
}

impl ChiefTurnExecution {
	pub(crate) fn validate(&self) -> Result<(), StoreError> {
		let valid = |s: &str, max| {
			!s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
		};
		if !valid(&self.model, 512) || self.effort.as_deref().is_some_and(|s| !valid(s, 128)) {
			return Err(StoreError::InvalidInput("invalid turn execution selection"));
		}
		Ok(())
	}
}

pub(crate) fn record(
	connection: &rusqlite::Connection,
	work: &str,
	thread: &str,
	turn: &str,
	execution: &ChiefTurnExecution,
) -> Result<(), StoreError> {
	let payload = serde_json::json!({"threadId":thread,"turnId":turn,"execution":execution});
	let source = serde_json::json!(["turn_execution", work, thread, turn]).to_string();
	connection.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'turn_execution',?3,?4,'resolved','Requested selection acknowledged; not inference telemetry.',?4)",params![source,work,payload.to_string(),unix_micros()?]).map_err(sqlite_error)?;
	Ok(())
}

impl SqliteStore {
	/// Read the requested selection for an exact work/thread/turn. Missing ACK is not evidence.
	pub async fn chief_turn_execution(
		&self,
		work: String,
		thread: String,
		turn: String,
	) -> Result<Option<ChiefTurnExecution>, StoreError> {
		self.run(move |connection| {
			let raw: Option<String> = connection.query_row("SELECT json_extract(e.payload,'$.execution') FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id WHERE e.work_item_id=?1 AND w.codex_thread_id=?2 AND e.event_kind='turn_execution' AND json_extract(e.payload,'$.threadId')=?2 AND json_extract(e.payload,'$.turnId')=?3",params![work,thread,turn],|r|r.get(0)).optional().map_err(sqlite_error)?;
			raw.map(|s| {
				let value: ChiefTurnExecution = serde_json::from_str(&s).map_err(|_|StoreError::InvalidInput("invalid saved turn execution"))?;
				value.validate()?; Ok(value)
			}).transpose()
		}).await
	}
}
