//! Non-waking live-turn setting receipts. A reservation is never replay authority.
use crate::{SqliteStore, StoreError, chief_process::owns_work, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
#[cfg(test)] use serde_json::Value;
use serde_json::json;
use sha2::{Digest as _, Sha256};

pub struct ChiefLiveReviewerAttempt {
	pub work_id: String,
	pub thread_id: String,
	pub turn_id: String,
	pub generation_id: Option<String>,
	pub review_token: String,
	pub reviewer: String,
	pub attempt_id: String,
	pub previous_id: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefLiveReviewerReceipt {
	pub id: i64,
	pub reviewer: String,
	pub outcome: String,
	pub generation_id: Option<String>,
}

fn latest(
	connection: &rusqlite::Connection,
	work: &str,
	thread: &str,
	turn: &str,
) -> Result<Option<ChiefLiveReviewerReceipt>, StoreError> {
	Ok(connection.query_row(
		"SELECT a.id,json_extract(a.payload,'$.reviewer'),COALESCE(r.disposition_note,'reserved'),json_extract(a.payload,'$.generationId') FROM chief_inbox_events a LEFT JOIN chief_inbox_events r ON r.source_event_id='live-reviewer-result:'||a.id AND r.work_item_id=a.work_item_id AND r.event_kind='live_reviewer_result' WHERE a.work_item_id=?1 AND a.event_kind='live_reviewer_attempt' AND json_extract(a.payload,'$.threadId')=?2 AND json_extract(a.payload,'$.turnId')=?3 ORDER BY a.id DESC LIMIT 1",
		params![work,thread,turn], |row| Ok(ChiefLiveReviewerReceipt {id:row.get(0)?,reviewer:row.get(1)?,outcome:row.get(2)?,generation_id:row.get(3)?})
	).optional().map_err(sqlite_error)?)
}

impl SqliteStore {
	/// Return the latest publication receipt, not the effective native reviewer.
	pub async fn chief_live_reviewer_receipt(
		&self,
		work: String,
		thread: String,
		turn: String,
	) -> Result<Option<ChiefLiveReviewerReceipt>, StoreError> {
		self.run(move |connection| latest(connection, &work, &thread, &turn)).await
	}

	/// Reserve an explicit edit against exact ownership, live turn and previously reviewed receipt.
	/// A different client key cannot replay the same review. Records are already resolved and never
	/// wake work.
	pub async fn reserve_chief_live_reviewer(
		&self,
		a: ChiefLiveReviewerAttempt,
	) -> Result<Option<i64>, StoreError> {
		if !matches!(a.reviewer.as_str(), "user" | "auto_review")
			|| a.review_token.len() != 64
			|| !a.review_token.bytes().all(|b| b.is_ascii_hexdigit())
			|| a.previous_id.is_some_and(|id| id <= 0)
			|| [&a.work_id, &a.thread_id, &a.turn_id, &a.attempt_id]
				.iter()
				.any(|v| v.trim().is_empty() || v.len() > 4096 || v.chars().any(char::is_control))
		{
			return Err(StoreError::InvalidInput("invalid live reviewer attempt"));
		}
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let live:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id=?1 AND codex_thread_id=?2 AND active_turn_id=?3 AND dispatch_state='running')",params![a.work_id,a.thread_id,a.turn_id],|r|r.get(0)).map_err(sqlite_error)?;
			if !live || !owns_work(&tx,&a.work_id,a.generation_id.as_deref())?
				|| tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_misalignment WHERE work_id=?1 AND thread_id=?2)",params![a.work_id,a.thread_id],|r|r.get::<_,bool>(0)).map_err(sqlite_error)? {
				return Err(StoreError::OwnershipLost("live reviewer target"));
			}
			let prior=latest(&tx,&a.work_id,&a.thread_id,&a.turn_id)?;
			if prior.as_ref().map(|r|r.id)!=a.previous_id || prior.as_ref().is_some_and(|r|r.outcome=="reserved"&&r.generation_id==a.generation_id) {return Ok(None);}
			let identity=json!([a.work_id,a.thread_id,a.turn_id,a.review_token]);
			let digest:String=Sha256::digest(identity.to_string().as_bytes()).iter().map(|b|format!("{b:02x}")).collect();
			let source=format!("live-reviewer-attempt:{digest}");
			if tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE source_event_id=?1)",[&source],|r|r.get::<_,bool>(0)).map_err(sqlite_error)? {return Ok(None);}
			let now=unix_micros()?;
			let payload=json!({"threadId":a.thread_id,"turnId":a.turn_id,"generationId":a.generation_id,"reviewToken":a.review_token,"reviewer":a.reviewer,"attemptId":a.attempt_id}).to_string();
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'live_reviewer_attempt',?3,?4,'resolved','reserved',?4)",params![source,a.work_id,payload,now]).map_err(sqlite_error)?;
			let id=tx.last_insert_rowid();tx.commit().map_err(sqlite_error)?;Ok(Some(id))
		}).await
	}

	/// Append one immutable terminal result for the original attempt. Late duplicates cannot change
	/// it. `applied` records publication only; `unknown` never becomes automatic retry permission.
	pub async fn finish_chief_live_reviewer(
		&self,
		id: i64,
		attempt: String,
		outcome: String,
	) -> Result<bool, StoreError> {
		if !matches!(outcome.as_str(), "applied" | "target_unavailable" | "rejected" | "unknown") {
			return Err(StoreError::InvalidInput("invalid live reviewer outcome"));
		}
		self.run(move |connection| {
			Ok(connection.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT 'live-reviewer-result:'||id,work_item_id,'live_reviewer_result',json_set(payload,'$.attemptEventId',id),?4,'resolved',?3,?4 FROM chief_inbox_events WHERE id=?1 AND event_kind='live_reviewer_attempt' AND json_extract(payload,'$.attemptId')=?2",params![id,attempt,outcome,unix_micros()?]).map(|changed|changed==1).map_err(sqlite_error)?)
		}).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus};

	fn attempt(token: char, previous_id: Option<i64>) -> ChiefLiveReviewerAttempt {
		ChiefLiveReviewerAttempt {
			work_id: "work".into(),
			thread_id: "thread".into(),
			turn_id: "turn".into(),
			generation_id: None,
			review_token: token.to_string().repeat(64),
			reviewer: "user".into(),
			attempt_id: format!("attempt-{token}"),
			previous_id,
		}
	}

	#[tokio::test]
	async fn live_reviewer_reservations_survive_crash_and_reject_competing_or_stale_edits() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("state.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store
			.create_chief_work_item(ChiefWorkItem {
				id: "work".into(),
				parent_goal_id: None,
				kind: ChiefWorkKind::Goal,
				title: "Fixture".into(),
				instructions: "Fixture".into(),
				codex_thread_id: None,
				status: ChiefWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
				active_turn_id: None,
				dispatch_state: ChiefDispatchState::Idle,
			})
			.await
			.unwrap();
		store.bind_chief_thread("work".into(), "thread".into()).await.unwrap();
		assert!(store.reserve_chief_live_reviewer(attempt('a', None)).await.is_err());
		store.begin_chief_dispatch("work".into()).await.unwrap();
		store.acknowledge_chief_dispatch("work".into(), "turn".into()).await.unwrap();
		let (a, b) = tokio::join!(
			store.reserve_chief_live_reviewer(attempt('a', None)),
			store.reserve_chief_live_reviewer(attempt('b', None))
		);
		assert_ne!(a.as_ref().unwrap().is_some(), b.as_ref().unwrap().is_some());
		let id = a.unwrap().or(b.unwrap()).unwrap();
		let event = store.get_chief_inbox_event(id).await.unwrap();
		assert!(event.disposition.is_some(), "must not wake or deliver to model");
		let payload: Value = serde_json::from_str(&event.payload).unwrap();
		let key = payload["attemptId"].as_str().unwrap().to_owned();
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert!(store.reserve_chief_live_reviewer(attempt('c', Some(id))).await.unwrap().is_none());
		assert!(
			!store
				.finish_chief_live_reviewer(id, "different".into(), "applied".into())
				.await
				.unwrap()
		);
		assert!(store.finish_chief_live_reviewer(id, key.clone(), "unknown".into()).await.unwrap());
		assert!(!store.finish_chief_live_reviewer(id, key, "applied".into()).await.unwrap());
		let state = store
			.chief_live_reviewer_receipt("work".into(), "thread".into(), "turn".into())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(state.outcome, "unknown");
		let original = store.get_chief_inbox_event(id).await.unwrap();
		assert_eq!(original.payload, event.payload);
		assert_eq!(original.disposition_note.as_deref(), Some("reserved"));
		assert!(store.list_pending_chief_events(100).await.unwrap().is_empty());
		let mut replay = attempt('f', Some(id));
		replay.review_token = payload["reviewToken"].as_str().unwrap().into();
		replay.reviewer = "auto_review".into();
		assert!(store.reserve_chief_live_reviewer(replay).await.unwrap().is_none());
		assert!(store.reserve_chief_live_reviewer(attempt('d', None)).await.unwrap().is_none());
		let next =
			store.reserve_chief_live_reviewer(attempt('d', Some(id))).await.unwrap().unwrap();
		assert_ne!(next, id);
		let mut stale = attempt('e', Some(next));
		stale.turn_id = "old-turn".into();
		assert!(store.reserve_chief_live_reviewer(stale).await.is_err());
		assert_eq!(
			store.get_chief_work_item("work".into()).await.unwrap().active_turn_id.as_deref(),
			Some("turn")
		);
	}
}
