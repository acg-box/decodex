//! Read and settle preserved model-recovery events through the current model owner.
use crate::{SqliteStore, StoreError, chief_process::owns_work, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, params};
use serde::Deserialize;
use serde_json::{Value, json};

// A queued or uncertain result does not resolve a reservation. Keep original event IDs and keys.
const UNRESOLVED: &str = "e.event_kind='model_recovery' AND NOT EXISTS(SELECT 1 FROM chief_inbox_events r WHERE r.work_item_id=e.work_item_id AND ((r.source_event_id=e.source_event_id||':result' AND r.event_kind='model_recovery_result' AND json_extract(r.payload,'$.state')='rejected') OR (r.source_event_id=e.source_event_id||':observation' AND r.event_kind='model_recovery_observation' AND json_extract(r.payload,'$.state')='target_observed') OR (r.source_event_id=e.source_event_id||':reconciliation' AND r.event_kind='model_selection_reconciled')))";

/// A preserved pre-upgrade model request that still prohibits replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefLegacyModelPending {
	/// Requested model, not proof of the active model.
	pub model: String,
	/// Requested configured effort.
	pub effort: String,
	/// Current receipt vocabulary: reserved, queued or unknown.
	pub state: String,
}

#[derive(Deserialize)]
struct Attempt {
	generation: String,
	account: String,
	account_revision: i64,
	model: String,
	effort: String,
	service_tier: Option<String>,
	manual_review: Option<String>,
}

fn decode(raw: &str) -> Result<Attempt, StoreError> {
	serde_json::from_str(raw).map_err(|_| StoreError::InvalidInput("invalid legacy model attempt"))
}

pub(super) fn pending(connection: &rusqlite::Connection, work: &str) -> Result<bool, StoreError> {
	connection
		.query_row(
			&format!(
				"SELECT EXISTS(SELECT 1 FROM chief_inbox_events e WHERE e.work_item_id=?1 AND {UNRESOLVED})"
			),
			[work],
			|row| row.get(0),
		)
		.map_err(|error| sqlite_error(error).into())
}

impl SqliteStore {
	/// Check both current and preserved model journals before another setting mutation.
	pub async fn has_pending_chief_model_change(&self, work: String) -> Result<bool, StoreError> {
		self.run(move |connection| crate::chief_models::pending(connection, &work)).await
	}

	/// Read an unresolved legacy request only for the current exact task owner.
	pub async fn pending_chief_legacy_model_change(
		&self,
		work: String,
		thread: String,
		generation: String,
	) -> Result<Option<ChiefLegacyModelPending>, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction().map_err(sqlite_error)?;
			let connection = &transaction;
			if !owns_work(connection, &work, Some(&generation))? { return Ok(None); }
			let row: Option<(String, Option<String>)> = connection.query_row(&format!("SELECT json_extract(e.payload,'$.attempt'),json_extract(r.payload,'$.state') FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id AND w.codex_thread_id=?2 LEFT JOIN chief_inbox_events r ON r.source_event_id=e.source_event_id||':result' AND r.event_kind='model_recovery_result' AND r.work_item_id=e.work_item_id WHERE e.work_item_id=?1 AND json_extract(e.payload,'$.attempt.thread')=?2 AND {UNRESOLVED} ORDER BY e.id DESC LIMIT 1"), params![work,thread], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(sqlite_error)?;
			let Some((raw, response)) = row else { return Ok(None); };
			let attempt = decode(&raw)?;
			let state = match response.as_deref() {
				None => "reserved",
				Some("queued") => "queued",
				Some("uncertain") => "unknown",
				_ => return Err(StoreError::InvalidInput("invalid legacy model response")),
			};
			let pending = ChiefLegacyModelPending { model: attempt.model, effort: attempt.effort, state: state.into() };
			transaction.commit().map_err(sqlite_error)?;
			Ok(Some(pending))
		}).await
	}
}

// The caller has validated complete current model facts, account availability and ownership.
pub(super) fn observe(
	connection: &rusqlite::Connection,
	work: &str,
	thread: &str,
	generation: Option<&str>,
	observation: i64,
	settings: &Value,
) -> Result<(), StoreError> {
	let Some(generation) = generation else {
		return Ok(());
	};
	let row: Option<(i64, String, String)> = connection.query_row(&format!("SELECT e.id,e.source_event_id,json_extract(e.payload,'$.attempt') FROM chief_inbox_events e WHERE e.work_item_id=?1 AND json_extract(e.payload,'$.attempt.thread')=?2 AND e.id<?3 AND {UNRESOLVED} ORDER BY e.id DESC LIMIT 1"), params![work,thread,observation], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(sqlite_error)?;
	let Some((reservation, key, raw)) = row else {
		return Ok(());
	};
	let attempt = decode(&raw)?;
	let (suffix, kind, payload, note) = if attempt.generation == generation {
		let account_current: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND revision=?2 AND enabled=1 AND tombstoned_at_micros IS NULL)", params![attempt.account,attempt.account_revision], |row| row.get(0)).map_err(sqlite_error)?;
		if !account_current
			|| settings["model"].as_str() != Some(&attempt.model)
			|| settings["effort"].as_str() != Some(&attempt.effort)
			|| (attempt.manual_review.is_none()
				&& settings["serviceTier"].as_str() != attempt.service_tier.as_deref())
		{
			return Ok(());
		}
		(
			":observation",
			"model_recovery_observation",
			json!({"reservation":reservation,"settingsEvent":observation,"state":"target_observed"}),
			"Current native target observed; legacy request causation is not asserted.",
		)
	} else {
		let dead: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM process_generations g JOIN process_generation_death_evidence d ON d.evidence_id=g.death_evidence_id AND d.generation_id=g.generation_id WHERE g.generation_id=?1 AND g.state='dead')", [&attempt.generation], |row| row.get(0)).map_err(sqlite_error)?;
		if !dead {
			return Ok(());
		}
		(
			":reconciliation",
			"model_selection_reconciled",
			json!({"reservation":reservation,"settingsEvent":observation,"generationId":generation}),
			"Current settings reviewed after prior process death; old delivery remains unconfirmed.",
		)
	};
	let now = unix_micros()?;
	connection.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,?3,?4,?5,'resolved',?6,?5)", params![format!("{key}{suffix}"),work,kind,payload.to_string(),now,note]).map_err(sqlite_error)?;
	Ok(())
}
