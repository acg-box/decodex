//! Read current and preserved model receipts without changing their original evidence.
use crate::{SqliteStore, StoreError, chief_process::owns_work, error::sqlite_error};
use rusqlite::{OptionalExtension as _, params};
use serde::Deserialize;

/// Historical request facts, separate from the task's current native configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefModelHistory {
	pub id: i64,
	pub model: String,
	pub effort: Option<String>,
	pub manual: bool,
	/// Original native response: reserved, queued, rejected or unknown.
	pub response: String,
	/// A later publication matched the requested target; this does not prove causation.
	pub target_observed: bool,
	/// Current settings were reviewed after the original process was confirmed dead.
	pub reconciled: bool,
}

#[derive(Deserialize)]
struct Selection {
	model: String,
	effort: Option<String>,
	manual_review: Option<String>,
	recovery: Option<serde_json::Value>,
}

impl SqliteStore {
	/// Read the latest receipt across both journals for an exact current task owner.
	/// This read does not grant replay and never replaces the pending-mutation guard.
	pub async fn chief_model_history(
		&self,
		work: String,
		thread: String,
		generation: String,
	) -> Result<Option<ChiefModelHistory>, StoreError> {
		self.run(move |connection| {
			let tx = connection.transaction().map_err(sqlite_error)?;
			if !owns_work(&tx, &work, Some(&generation))? { return Ok(None); }
			let row = tx.query_row(
				"SELECT e.id,e.event_kind,json_extract(e.payload,'$.attempt'),COALESCE(json_extract(r.payload,'$.state'),'reserved'),COALESCE(json_extract(o.payload,'$.state')='target_observed',0),(c.id IS NOT NULL OR COALESCE(json_extract(o.payload,'$.state')='superseded',0) OR COALESCE(json_extract(o.payload,'$.generationId')<>json_extract(e.payload,'$.attempt.generation'),0)) FROM chief_work_items w JOIN chief_inbox_events e ON e.work_item_id=w.id AND e.event_kind IN ('model_selection','model_recovery') LEFT JOIN chief_inbox_events r ON r.work_item_id=e.work_item_id AND r.source_event_id=e.source_event_id||':result' AND r.event_kind=e.event_kind||'_result' LEFT JOIN chief_inbox_events o ON o.work_item_id=e.work_item_id AND o.source_event_id=e.source_event_id||':observation' AND o.event_kind=e.event_kind||'_observation' LEFT JOIN chief_inbox_events c ON c.work_item_id=e.work_item_id AND c.source_event_id=e.source_event_id||':reconciliation' AND c.event_kind='model_selection_reconciled' WHERE w.id=?1 AND w.codex_thread_id=?2 AND json_extract(e.payload,'$.attempt.thread')=?2 ORDER BY e.id DESC LIMIT 1",
				params![work,thread],
				|row| Ok((row.get::<_,i64>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?, row.get::<_,String>(3)?, row.get::<_,bool>(4)?, row.get::<_,bool>(5)?)),
			).optional().map_err(sqlite_error)?;
			let Some((id, kind, raw, response, target_observed, reconciled)) = row else { return Ok(None); };
			let selection: Selection = serde_json::from_str(&raw)
				.map_err(|_| StoreError::InvalidInput("invalid saved model history"))?;
			let legacy = kind == "model_recovery";
			if !super::valid(&selection.model, 256)
				|| selection.effort.as_deref().is_some_and(|effort| !super::valid(effort, 128))
				|| (legacy && selection.effort.is_none()) {
				return Err(StoreError::InvalidInput("invalid saved model history"));
			}
			let response = match response.as_str() {
				"reserved" | "queued" | "rejected" | "unknown" => response,
				"uncertain" if legacy => "unknown".into(),
				_ => return Err(StoreError::InvalidInput("invalid saved model response")),
			};
			let history = ChiefModelHistory {
				id, model: selection.model, effort: selection.effort,
				manual: if legacy { selection.manual_review.is_some() } else { selection.recovery.is_none() },
				response, target_observed, reconciled,
			};
			tx.commit().map_err(sqlite_error)?;
			Ok(Some(history))
		}).await
	}
}
