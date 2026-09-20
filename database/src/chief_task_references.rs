//! Exact native-history grants carried by delivered user messages.

use crate::{SqliteStore, StoreError, error::sqlite_error};
use rusqlite::{Connection, params};
use serde_json::Value;
use std::collections::HashSet;

impl SqliteStore {
	/// A delivered user message grants only the exact referenced work and thread.
	/// Pending and rejected steering attempts cannot grant access.
	pub async fn chief_has_task_reference(
		&self,
		recipient: String,
		target: String,
		thread: String,
	) -> Result<bool, StoreError> {
		self.run(move |connection| {
			connection.query_row(
				"SELECT EXISTS(SELECT 1 FROM chief_inbox_events e, json_each(CASE WHEN json_valid(e.payload) THEN e.payload ELSE '{}' END,'$.options.taskReferences') r
				 WHERE e.work_item_id=?1 AND e.delivery_work_item_id=?1
				 AND e.event_kind='user_message' AND e.delivered_turn_id IS NOT NULL AND e.delivered_turn_id<>''
				 AND json_extract(CASE WHEN json_valid(e.payload) THEN e.payload ELSE '{}' END,'$.source')='user'
				 AND json_extract(CASE WHEN r.type='object' THEN r.value ELSE '{}' END,'$.workId')=?2 AND json_extract(CASE WHEN r.type='object' THEN r.value ELSE '{}' END,'$.threadId')=?3)",
				params![recipient,target,thread], |row| row.get(0),
			).map_err(|error| sqlite_error(error).into())
		}).await
	}
}

/// Validate references in the same write transaction that accepts the user input.
pub(crate) fn validate_references(
	connection: &Connection,
	payload: &str,
) -> Result<(), StoreError> {
	let Ok(value) = serde_json::from_str::<Value>(payload) else {
		return Ok(());
	};
	let Some(references) = value.pointer("/options/taskReferences") else {
		return Ok(());
	};
	let references = references
		.as_array()
		.ok_or(StoreError::InvalidInput("task references must be an array"))?;
	if references.len() > 16 || (!references.is_empty() && value["source"] != "user") {
		return Err(StoreError::InvalidInput("invalid task reference authority or count"));
	}
	let mut seen = HashSet::new();
	for reference in references {
		let object =
			reference.as_object().ok_or(StoreError::InvalidInput("invalid task reference"))?;
		if object.len() != 3 {
			return Err(StoreError::InvalidInput("invalid task reference fields"));
		}
		let target = field(reference, "workId", 512)?;
		let thread = field(reference, "threadId", 512)?;
		let _title = field(reference, "title", 1024)?;
		if !seen.insert((target, thread)) {
			return Err(StoreError::InvalidInput("duplicate task reference"));
		}
		let current: bool = connection
			.query_row(
				"SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id=?1 AND codex_thread_id=?2)",
				params![target, thread],
				|row| row.get(0),
			)
			.map_err(sqlite_error)?;
		if !current {
			return Err(StoreError::InvalidInput("referenced task changed; select it again"));
		}
	}
	Ok(())
}

fn field<'a>(reference: &'a Value, key: &str, max: usize) -> Result<&'a str, StoreError> {
	reference[key]
		.as_str()
		.filter(|s| !s.is_empty() && s.len() <= max)
		.ok_or(StoreError::InvalidInput("invalid task reference field"))
}
