//! Complete native approval details; inbox scans retain only routing identity.
use crate::{ChiefInboxEvent, EnqueueChiefEvent, StoreError, error::sqlite_error};
use rusqlite::{Connection, OptionalExtension as _};
use serde_json::{Value, json};

pub(crate) fn compact(input: &EnqueueChiefEvent) -> Result<Option<String>, StoreError> {
	if input.payload.len() <= 65536 {
		return Ok(None);
	}
	if !matches!(input.event_kind.as_str(), "permission_pending" | "server_request_pending")
		|| input.payload.len() > decodex_core::MAX_APPROVAL_ENVELOPE_BYTES
	{
		return Err(StoreError::InvalidInput("Chief event payload is too large"));
	}
	let value: Value = serde_json::from_str(&input.payload)
		.map_err(|_| StoreError::InvalidInput("native approval payload is invalid"))?;
	if !matches!(
		(input.event_kind.as_str(), value["method"].as_str()),
		(
			"permission_pending",
			Some(
				"item/commandExecution/requestApproval"
					| "item/fileChange/requestApproval"
					| "item/permissions/requestApproval"
			)
		) | ("server_request_pending", Some("mcpServer/elicitation/request"))
	) {
		return Err(StoreError::InvalidInput("large event is not a native approval"));
	}
	let frame_limit = decodex_core::MAX_NATIVE_MESSAGE_BYTES;
	if value["params"].to_string().len() > frame_limit {
		return Err(StoreError::InvalidInput("native request parameters are too large"));
	}
	if let Some(file) = value.get("fileChange")
		&& (value["method"] != "item/fileChange/requestApproval"
			|| file["type"] != "fileChange"
			|| file["id"] != value["params"]["itemId"]
			|| file.to_string().len() > frame_limit)
	{
		return Err(StoreError::InvalidInput("native file evidence is invalid or too large"));
	}
	let metadata: serde_json::Map<String, Value> = value
		.as_object()
		.ok_or(StoreError::InvalidInput("native approval envelope is invalid"))?
		.iter()
		.filter(|(key, _)| !["params", "fileChange"].contains(&key.as_str()))
		.map(|(key, value)| (key.clone(), value.clone()))
		.collect();
	if Value::Object(metadata).to_string().len() > 65536 {
		return Err(StoreError::InvalidInput("native approval metadata is too large"));
	}
	let mut params = serde_json::Map::new();
	for key in ["threadId", "turnId", "itemId"] {
		if let Some(field) = value["params"].get(key) {
			params.insert(key.into(), field.clone());
		}
	}
	let compact = json!({"id":value["id"],"method":value["method"],
		"params":params,"ownerThreadId":value["ownerThreadId"],"detailsStored":true})
	.to_string();
	if compact.len() > 65536 {
		return Err(StoreError::InvalidInput("native approval identity is too large"));
	}
	Ok(Some(compact))
}

pub(crate) fn hydrate(
	connection: &Connection,
	mut event: ChiefInboxEvent,
) -> Result<ChiefInboxEvent, StoreError> {
	if let Some(payload) = connection
		.query_row(
			"SELECT payload FROM chief_request_payloads WHERE event_id=?1",
			[event.id],
			|row| row.get::<_, String>(0),
		)
		.optional()
		.map_err(sqlite_error)?
	{
		event.payload = payload;
	}
	Ok(event)
}
