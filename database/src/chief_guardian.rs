//! Durable Guardian evidence. This table never authorizes execution or wakes work.
use crate::{SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, params};
use serde_json::Value;

pub struct ChiefGuardianObservation {
	pub thread_id: String,
	pub turn_id: String,
	pub review_id: String,
	pub connection_id: String,
	pub generation_id: Option<String>,
	pub event_json: String,
}

pub struct ChiefGuardianReview {
	pub id: i64,
	pub thread_id: String,
	pub turn_id: String,
	pub review_id: String,
	pub connection_id: String,
	pub generation_id: Option<String>,
	pub status: String,
	pub event_json: String,
	pub conflicted: bool,
	pub approval_state: Option<String>,
	pub approval_key: Option<String>,
}

impl ChiefGuardianReview {
	/// Identity of the exact observed evidence shown for an explicit decision.
	pub fn digest(&self) -> String {
		use sha2::{Digest as _, Sha256};
		let value = serde_json::json!([
			self.id,
			self.thread_id,
			self.turn_id,
			self.review_id,
			self.event_json,
			self.connection_id,
			self.generation_id,
			self.conflicted
		])
		.to_string();
		Sha256::digest(value.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
	}
}

impl SqliteStore {
	/// Retain a validated observation for a proved turn of the currently bound thread.
	/// Terminal evidence is monotonic. Contradictory evidence is retained as a conflict,
	/// never silently substituted as the action a user might subsequently approve.
	pub async fn record_chief_guardian_review(
		&self,
		observation: ChiefGuardianObservation,
	) -> Result<(), StoreError> {
		let o = observation;
		let valid_id = |s: &str| !s.trim().is_empty() && s.len() <= 512;
		if !valid_id(&o.thread_id)
			|| !valid_id(&o.turn_id)
			|| !valid_id(&o.review_id)
			|| !valid_id(&o.connection_id)
			|| o.generation_id.as_deref().is_some_and(|s| !valid_id(s))
			|| o.event_json.len() > 256 * 1024
		{
			return Err(StoreError::InvalidInput("invalid Guardian observation"));
		}
		let value: Value = serde_json::from_str(&o.event_json)
			.map_err(|_| StoreError::InvalidInput("invalid Guardian event"))?;
		let status = value
			.pointer("/review/status")
			.and_then(Value::as_str)
			.filter(|s| ["inProgress", "approved", "denied", "timedOut", "aborted"].contains(s))
			.ok_or(StoreError::InvalidInput("invalid Guardian status"))?
			.to_owned();
		if value["threadId"] != o.thread_id
			|| value["turnId"] != o.turn_id
			|| value["reviewId"] != o.review_id
			|| !value["action"].is_object()
		{
			return Err(StoreError::InvalidInput("Guardian event identity differs"));
		}
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let terminal = serde_json::json!(["turn/completed",o.thread_id,o.turn_id]).to_string();
			let work: Option<String> = tx.query_row(
				"SELECT w.id FROM chief_work_items w WHERE w.codex_thread_id=?1 AND
				 ((w.active_turn_id=?2 AND w.dispatch_state='running') OR EXISTS(
				 SELECT 1 FROM chief_inbox_events e WHERE e.work_item_id=w.id AND e.source_event_id=?3
				 AND e.event_kind IN ('chief_turn_completed','worker_turn_completed','capacity_retry')))",
				params![o.thread_id,o.turn_id,terminal], |row| row.get(0)).optional().map_err(sqlite_error)?;
			let Some(work) = work else { return Ok(()); };
			// A production connection must still own this work through its latest ready
			// native generation. Manager work is owned by its own process subtree.
			if !owns_work(&tx, &work, o.generation_id.as_deref())? { return Ok(()); }
			let previous: Option<(i64,String,String)> = tx.query_row(
				"SELECT id,status,event_json FROM chief_guardian_reviews WHERE work_id=?1 AND thread_id=?2 AND turn_id=?3 AND review_id=?4",
				params![work,o.thread_id,o.turn_id,o.review_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(sqlite_error)?;
			let now = unix_micros()?;
			if let Some((id, old_status, old_json)) = previous {
				let old: Value = serde_json::from_str(&old_json).map_err(|_|StoreError::InvalidInput("invalid retained Guardian event"))?;
				let same_action = old["action"] == value["action"] && old["targetItemId"] == value["targetItemId"] && old["startedAtMs"] == value["startedAtMs"];
				if !same_action || (old_status != "inProgress" && status != "inProgress" && old != value) {
					tx.execute("UPDATE chief_guardian_reviews SET conflicted=1,updated_at_micros=max(updated_at_micros,?2) WHERE id=?1",params![id,now]).map_err(sqlite_error)?;
				} else if old_status == "inProgress" && status != "inProgress" {
					tx.execute("UPDATE chief_guardian_reviews SET status=?2,event_json=?3,connection_id=?4,generation_id=?5,updated_at_micros=max(updated_at_micros,?6) WHERE id=?1",params![id,status,o.event_json,o.connection_id,o.generation_id,now]).map_err(sqlite_error)?;
				}
			} else {
				tx.execute("INSERT INTO chief_guardian_reviews(work_id,thread_id,turn_id,review_id,connection_id,generation_id,status,event_json,created_at_micros,updated_at_micros) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",params![work,o.thread_id,o.turn_id,o.review_id,o.connection_id,o.generation_id,status,o.event_json,now]).map_err(sqlite_error)?;
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}

	/// Page observations for the work's current native thread, newest first.
	pub async fn read_chief_guardian_reviews(
		&self,
		work: String,
		before: Option<i64>,
		limit: usize,
	) -> Result<Vec<ChiefGuardianReview>, StoreError> {
		let limit = limit.clamp(1, 100) as i64;
		self.run(move |connection| {
			connection
				.prepare(&format!(
					"{READ_REVIEW} WHERE r.work_id=?1 AND (?2 IS NULL OR r.id<?2) ORDER BY r.id DESC LIMIT ?3"
				))
				.map_err(sqlite_error)?
				.query_map(params![work, before, limit], review_row)
				.map_err(sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(|e| sqlite_error(e).into())
		})
		.await
	}

	pub async fn chief_guardian_review(
		&self,
		work: String,
		id: i64,
	) -> Result<Option<ChiefGuardianReview>, StoreError> {
		self.run(move |connection| read_review(connection, &work, id)).await
	}

	/// Atomically reserve one explicit submission for the exact displayed denial.
	pub async fn begin_chief_guardian_approval(
		&self,
		work: String,
		id: i64,
		digest: String,
		connection_id: String,
		generation: Option<String>,
		key: String,
	) -> Result<i64, StoreError> {
		if key.is_empty()
			|| key.len() > 512
			|| connection_id.is_empty()
			|| connection_id.len() > 512
		{
			return Err(StoreError::InvalidInput("invalid Guardian approval identity"));
		}
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let review=read_review(&tx,&work,id)?.ok_or(crate::DatabaseError::Conflict)?;
			if review.status!="denied" || review.conflicted || review.digest()!=digest
				|| matches!(review.approval_state.as_deref(),Some("pending"|"submitted"))
				|| !owns_work(&tx,&work,generation.as_deref())?
				|| (generation.is_none() && review.connection_id!=connection_id)
			{
				return Err(crate::DatabaseError::Conflict.into());
			}
			let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id=?1 AND codex_thread_id=?2 AND ((dispatch_state='idle' AND active_turn_id IS NULL) OR (dispatch_state='running' AND active_turn_id=?3)))",params![work,review.thread_id,review.turn_id],|r|r.get(0)).map_err(sqlite_error)?;
			if !current { return Err(crate::DatabaseError::Conflict.into()); }
			tx.execute("INSERT INTO chief_guardian_approvals(review_row_id,command_key,review_digest,connection_id,generation_id,state,created_at_micros) VALUES(?1,?2,?3,?4,?5,'pending',?6)",params![id,key,digest,connection_id,generation,unix_micros()?]).map_err(sqlite_error)?;
			let claim=tx.last_insert_rowid();
			tx.commit().map_err(sqlite_error)?;
			Ok(claim)
		}).await
	}

	/// Only a correlated RPC response finishes a claim. Transport loss leaves it pending.
	pub async fn finish_chief_guardian_approval(
		&self,
		claim: i64,
		submitted: bool,
	) -> Result<(), StoreError> {
		self.run(move |connection| {
			let changed=connection.execute("UPDATE chief_guardian_approvals SET state=?2,finished_at_micros=max(created_at_micros,?3) WHERE id=?1 AND state='pending'",params![claim,if submitted {"submitted"} else {"rejected"},unix_micros()?]).map_err(sqlite_error)?;
			if changed!=1 { return Err(crate::DatabaseError::Conflict.into()); }
			Ok(())
		}).await
	}
}

const READ_REVIEW: &str = "SELECT r.id,r.thread_id,r.turn_id,r.review_id,r.connection_id,r.generation_id,r.status,r.event_json,r.conflicted,(SELECT state FROM chief_guardian_approvals a WHERE a.review_row_id=r.id ORDER BY a.id DESC LIMIT 1),(SELECT command_key FROM chief_guardian_approvals a WHERE a.review_row_id=r.id ORDER BY a.id DESC LIMIT 1) FROM chief_guardian_reviews r JOIN chief_work_items w ON w.id=r.work_id AND w.codex_thread_id=r.thread_id";

fn review_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChiefGuardianReview> {
	Ok(ChiefGuardianReview {
		id: row.get(0)?,
		thread_id: row.get(1)?,
		turn_id: row.get(2)?,
		review_id: row.get(3)?,
		connection_id: row.get(4)?,
		generation_id: row.get(5)?,
		status: row.get(6)?,
		event_json: row.get(7)?,
		conflicted: row.get(8)?,
		approval_state: row.get(9)?,
		approval_key: row.get(10)?,
	})
}

fn read_review(
	connection: &rusqlite::Connection,
	work: &str,
	id: i64,
) -> Result<Option<ChiefGuardianReview>, StoreError> {
	connection
		.query_row(
			&format!("{READ_REVIEW} WHERE r.work_id=?1 AND r.id=?2"),
			params![work, id],
			review_row,
		)
		.optional()
		.map_err(|e| sqlite_error(e).into())
}

fn owns_work(
	connection: &rusqlite::Connection,
	work: &str,
	generation: Option<&str>,
) -> Result<bool, StoreError> {
	if let Some(generation) = generation {
		connection.query_row("WITH RECURSIVE owned(id,root_id) AS (
			SELECT b.root_id,b.root_id FROM chief_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id
			WHERE b.generation_id=?1 AND g.state='ready' AND b.rowid=(SELECT rowid FROM chief_process_bindings WHERE root_id=b.root_id ORDER BY created_at_micros DESC,rowid DESC LIMIT 1)
			UNION SELECT w.id,owned.root_id FROM chief_work_items w JOIN owned ON w.parent_goal_id=owned.id
			WHERE NOT EXISTS(SELECT 1 FROM chief_managers WHERE work_id=w.id))
			SELECT EXISTS(SELECT 1 FROM owned WHERE id=?2)", params![generation,work], |r|r.get(0)).map_err(|e|sqlite_error(e).into())
	} else {
		// Direct coordinator transports have no durable process host. They cannot
		// bypass ownership once any native process admission exists in this store.
		connection
			.query_row("SELECT NOT EXISTS(SELECT 1 FROM chief_process_bindings)", [], |r| r.get(0))
			.map_err(|e| sqlite_error(e).into())
	}
}
