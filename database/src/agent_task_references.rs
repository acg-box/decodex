//! Exact native-history grants carried by delivered user messages.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension as _};
use serde_json::Value;

use crate::{SqliteStore, StoreError, error};

impl SqliteStore {
	/// A delivered user message grants only the exact referenced work and thread.
	/// Pending and rejected steering attempts cannot grant access.
	pub async fn agent_has_task_reference(
		&self,
		recipient: String,
		target: String,
		thread: String,
	) -> Result<bool, StoreError> {
		self.run(move |connection| {
			connection.query_row(
				"SELECT EXISTS(SELECT 1 FROM agent_inbox_events e, json_each(CASE WHEN json_valid(e.payload) THEN e.payload ELSE '{}' END,'$.options.taskReferences') r
				 WHERE e.work_item_id=?1 AND e.delivery_work_item_id=?1
				 AND e.event_kind='user_message' AND e.delivered_turn_id IS NOT NULL AND e.delivered_turn_id<>''
				 AND json_extract(CASE WHEN json_valid(e.payload) THEN e.payload ELSE '{}' END,'$.source')='user'
				 AND json_extract(CASE WHEN r.type='object' THEN r.value ELSE '{}' END,'$.workId')=?2 AND json_extract(CASE WHEN r.type='object' THEN r.value ELSE '{}' END,'$.threadId')=?3)",
				rusqlite::params![recipient,target,thread], |row| row.get(0),
			).map_err(|error| error::sqlite_error(error).into())
		}).await
	}

	/// Resolve an exact delivered reference without scanning every work item during native search.
	pub async fn agent_task_reference_target(
		&self,
		recipient: String,
		thread: String,
	) -> Result<Option<String>, StoreError> {
		self.run(move |connection| {
			connection.query_row(
				"SELECT DISTINCT w.id FROM agent_inbox_events e,
				 json_each(CASE WHEN json_valid(e.payload) THEN e.payload ELSE '{}' END,'$.options.taskReferences') r
				 JOIN agent_work_items w ON w.id=json_extract(CASE WHEN r.type='object' THEN r.value ELSE '{}' END,'$.workId')
				 WHERE e.work_item_id=?1 AND e.delivery_work_item_id=?1
				 AND e.event_kind='user_message' AND e.delivered_turn_id IS NOT NULL AND e.delivered_turn_id<>''
				 AND json_extract(CASE WHEN json_valid(e.payload) THEN e.payload ELSE '{}' END,'$.source')='user'
				 AND json_extract(CASE WHEN r.type='object' THEN r.value ELSE '{}' END,'$.threadId')=?2
				 ORDER BY w.id LIMIT 1", rusqlite::params![recipient,thread], |row| row.get(0),
			).optional().map_err(|error| error::sqlite_error(error).into())
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
		let _title = field(reference, "title", 1_024)?;

		if !seen.insert((target, thread)) {
			return Err(StoreError::InvalidInput("duplicate task reference"));
		}

		let current: bool = connection
			.query_row(
				"SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id=?1 AND codex_thread_id=?2)",
				rusqlite::params![target, thread],
				|row| row.get(0),
			)
			.map_err(error::sqlite_error)?;

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

/// A persisted, explicitly selected context reference. Delivery is not proof of reading.
#[derive(Clone, Debug)]
pub struct AgentContextReference {
	pub event_id: i64,
	pub recipient_work_id: String,
	pub source_work_id: String,
	pub source_thread_id: String,
	pub delivery_turn_id: Option<String>,
}

impl SqliteStore {
	/// Read a bounded set of reference receipts, including queued input.
	pub async fn agent_context_references(&self) -> Result<Vec<AgentContextReference>, StoreError> {
		self.run(|connection| {
            let mut statement = connection.prepare(
                "SELECT e.id,e.work_item_id,json_extract(r.value,'$.workId'),json_extract(r.value,'$.threadId'),CASE WHEN e.delivery_work_item_id=e.work_item_id THEN NULLIF(e.delivered_turn_id,'') END
                 FROM agent_inbox_events e, json_each(CASE WHEN json_valid(e.payload) THEN e.payload ELSE '{}' END,'$.options.taskReferences') r
                 WHERE e.event_kind='user_message' AND r.type='object'
                 AND json_extract(CASE WHEN json_valid(e.payload) THEN e.payload ELSE '{}' END,'$.source')='user'
                 AND (e.disposition IS NULL OR (e.delivery_work_item_id=e.work_item_id AND e.delivered_turn_id IS NOT NULL AND e.delivered_turn_id<>''))
                 ORDER BY e.id DESC LIMIT 501"
            ).map_err(error::sqlite_error)?;
            let rows = statement.query_map([], |row| Ok(AgentContextReference {
                event_id: row.get(0)?, recipient_work_id: row.get(1)?, source_work_id: row.get(2)?, source_thread_id: row.get(3)?, delivery_turn_id: row.get(4)?,
            })).map_err(error::sqlite_error)?;
            rows.collect::<Result<Vec<_>,_>>().map_err(|e| error::sqlite_error(e).into())
        }).await
	}
}
