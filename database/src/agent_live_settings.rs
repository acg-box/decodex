//! Non-waking live-turn setting receipts. A reservation is never replay authority.
//! Persisted live_reviewer event names remain stable so old attempts share the same sequence.
use crate::{SqliteStore, StoreError, agent_process::owns_work, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
#[cfg(test)] use serde_json::Value;
use serde_json::json;
use sha2::{Digest as _, Sha256};

/// One explicit live-turn edit. Model and reviewer edits share one reservation sequence.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentLiveSettingsEdit {
	Reviewer { reviewer: String },
	Model { model: String, effort: String },
}

impl AgentLiveSettingsEdit {
	fn valid(&self) -> bool {
		let bounded = |s: &str, limit| {
			!s.trim().is_empty() && s.len() <= limit && !s.chars().any(char::is_control)
		};
		match self {
			Self::Reviewer { reviewer } => matches!(reviewer.as_str(), "user" | "auto_review"),
			Self::Model { model, effort } =>
				bounded(model, 256) && model != "gpt-reserve" && bounded(effort, 128),
		}
	}
}

pub struct AgentLiveSettingsAttempt {
	pub work_id: String,
	pub thread_id: String,
	pub turn_id: String,
	pub generation_id: Option<String>,
	pub review_token: String,
	pub edit: AgentLiveSettingsEdit,
	pub attempt_id: String,
	pub previous_id: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLiveSettingsReceipt {
	pub id: i64,
	pub edit: AgentLiveSettingsEdit,
	pub outcome: String,
	pub generation_id: Option<String>,
}

fn latest(
	connection: &rusqlite::Connection,
	work: &str,
	thread: &str,
	turn: &str,
) -> Result<Option<AgentLiveSettingsReceipt>, StoreError> {
	Ok(connection.query_row(
		"SELECT a.id,COALESCE(json_extract(a.payload,'$.edit'),json_object('kind','reviewer','reviewer',json_extract(a.payload,'$.reviewer'))),COALESCE(r.disposition_note,'reserved'),json_extract(a.payload,'$.generationId') FROM agent_inbox_events a LEFT JOIN agent_inbox_events r ON r.source_event_id='live-reviewer-result:'||a.id AND r.work_item_id=a.work_item_id AND r.event_kind='live_reviewer_result' WHERE a.work_item_id=?1 AND a.event_kind='live_reviewer_attempt' AND json_extract(a.payload,'$.threadId')=?2 AND json_extract(a.payload,'$.turnId')=?3 ORDER BY a.id DESC LIMIT 1",
		params![work,thread,turn], |row| Ok(AgentLiveSettingsReceipt {id:row.get(0)?,edit:serde_json::from_str(&row.get::<_,String>(1)?).map_err(|e|rusqlite::Error::FromSqlConversionFailure(1,rusqlite::types::Type::Text,Box::new(e)))?,outcome:row.get(2)?,generation_id:row.get(3)?})
	).optional().map_err(sqlite_error)?)
}

impl SqliteStore {
	/// Return the latest publication receipt, not the effective native settings.
	pub async fn agent_live_settings_receipt(
		&self,
		work: String,
		thread: String,
		turn: String,
	) -> Result<Option<AgentLiveSettingsReceipt>, StoreError> {
		self.run(move |connection| latest(connection, &work, &thread, &turn)).await
	}

	/// Reserve an explicit edit against exact ownership, live turn and previously reviewed receipt.
	/// A different client key cannot replay the same review. Records are already resolved and never
	/// wake work.
	pub async fn reserve_agent_live_settings(
		&self,
		a: AgentLiveSettingsAttempt,
	) -> Result<Option<i64>, StoreError> {
		if !a.edit.valid()
			|| a.review_token.len() != 64
			|| !a.review_token.bytes().all(|b| b.is_ascii_hexdigit())
			|| a.previous_id.is_some_and(|id| id <= 0)
			|| [&a.work_id, &a.thread_id, &a.turn_id, &a.attempt_id]
				.iter()
				.any(|v| v.trim().is_empty() || v.len() > 4096 || v.chars().any(char::is_control))
		{
			return Err(StoreError::InvalidInput("invalid live settings attempt"));
		}
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let live:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE id=?1 AND codex_thread_id=?2 AND active_turn_id=?3 AND dispatch_state='running')",params![a.work_id,a.thread_id,a.turn_id],|r|r.get(0)).map_err(sqlite_error)?;
			if !live || !owns_work(&tx,&a.work_id,a.generation_id.as_deref())?
				|| tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_misalignment WHERE work_id=?1 AND thread_id=?2)",params![a.work_id,a.thread_id],|r|r.get::<_,bool>(0)).map_err(sqlite_error)? {
				return Err(StoreError::OwnershipLost("live settings target"));
			}
			let prior=latest(&tx,&a.work_id,&a.thread_id,&a.turn_id)?;
			if prior.as_ref().map(|r|r.id)!=a.previous_id || prior.as_ref().is_some_and(|r|r.outcome=="reserved"&&r.generation_id==a.generation_id) {return Ok(None);}
			let identity=json!([a.work_id,a.thread_id,a.turn_id,a.review_token]);
			let digest:String=Sha256::digest(identity.to_string().as_bytes()).iter().map(|b|format!("{b:02x}")).collect();
			let source=format!("live-reviewer-attempt:{digest}");
			if tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events WHERE source_event_id=?1)",[&source],|r|r.get::<_,bool>(0)).map_err(sqlite_error)? {return Ok(None);}
			let now=unix_micros()?;
			let payload=json!({"threadId":a.thread_id,"turnId":a.turn_id,"generationId":a.generation_id,"reviewToken":a.review_token,"edit":a.edit,"attemptId":a.attempt_id}).to_string();
			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'live_reviewer_attempt',?3,?4,'resolved','reserved',?4)",params![source,a.work_id,payload,now]).map_err(sqlite_error)?;
			let id=tx.last_insert_rowid();tx.commit().map_err(sqlite_error)?;Ok(Some(id))
		}).await
	}

	/// Append one immutable terminal result for the original attempt. Late duplicates cannot change
	/// it. `applied` records publication only; `unknown` never becomes automatic retry permission.
	pub async fn finish_agent_live_settings(
		&self,
		id: i64,
		attempt: String,
		outcome: String,
	) -> Result<bool, StoreError> {
		if !matches!(outcome.as_str(), "applied" | "target_unavailable" | "rejected" | "unknown") {
			return Err(StoreError::InvalidInput("invalid live settings outcome"));
		}
		self.run(move |connection| {
			Ok(connection.execute("INSERT OR IGNORE INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT 'live-reviewer-result:'||id,work_item_id,'live_reviewer_result',json_set(payload,'$.attemptEventId',id),?4,'resolved',?3,?4 FROM agent_inbox_events WHERE id=?1 AND event_kind='live_reviewer_attempt' AND json_extract(payload,'$.attemptId')=?2",params![id,attempt,outcome,unix_micros()?]).map(|changed|changed==1).map_err(sqlite_error)?)
		}).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus};

	fn attempt(token: char, previous_id: Option<i64>) -> AgentLiveSettingsAttempt {
		AgentLiveSettingsAttempt {
			work_id: "work".into(),
			thread_id: "thread".into(),
			turn_id: "turn".into(),
			generation_id: None,
			review_token: token.to_string().repeat(64),
			edit: AgentLiveSettingsEdit::Reviewer { reviewer: "user".into() },
			attempt_id: format!("attempt-{token}"),
			previous_id,
		}
	}

	#[test]
	fn legacy_reviewer_payload_remains_readable() {
		let connection = rusqlite::Connection::open_in_memory().unwrap();
		connection.execute_batch("CREATE TABLE agent_inbox_events (id INTEGER,source_event_id TEXT,work_item_id TEXT,event_kind TEXT,payload TEXT,disposition_note TEXT);").unwrap();
		connection
			.execute(
				"INSERT INTO agent_inbox_events VALUES (1,'legacy','work','live_reviewer_attempt',?1,NULL)",
				[
					r#"{"threadId":"thread","turnId":"turn","reviewer":"auto_review","generationId":null}"#,
				],
			)
			.unwrap();
		let receipt = latest(&connection, "work", "thread", "turn").unwrap().unwrap();
		assert_eq!(
			receipt.edit,
			AgentLiveSettingsEdit::Reviewer { reviewer: "auto_review".into() }
		);
		assert_eq!(receipt.outcome, "reserved");
	}

	#[tokio::test]
	async fn live_reviewer_reservations_survive_crash_and_reject_competing_or_stale_edits() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("state.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store
			.create_agent_work_item(AgentWorkItem {
				id: "work".into(),
				parent_goal_id: None,
				kind: AgentWorkKind::Goal,
				title: "Fixture".into(),
				instructions: "Fixture".into(),
				codex_thread_id: None,
				status: AgentWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
				active_turn_id: None,
				dispatch_state: AgentDispatchState::Idle,
			})
			.await
			.unwrap();
		store.bind_agent_thread("work".into(), "thread".into()).await.unwrap();
		assert!(store.reserve_agent_live_settings(attempt('a', None)).await.is_err());
		store.begin_agent_dispatch("work".into()).await.unwrap();
		store.acknowledge_agent_dispatch("work".into(), "turn".into()).await.unwrap();
		let visible = store
			.record_agent_observation(crate::EnqueueAgentEvent {
				source_event_id: "visible-message".into(),
				work_item_id: "work".into(),
				event_kind: "assistant_message".into(),
				payload: json!({"item":{"text":"Visible message"}}).to_string(),
			})
			.await
			.unwrap();
		let (a, b) = tokio::join!(
			store.reserve_agent_live_settings(attempt('a', None)),
			store.reserve_agent_live_settings(attempt('b', None))
		);
		assert_ne!(a.as_ref().unwrap().is_some(), b.as_ref().unwrap().is_some());
		let id = a.unwrap().or(b.unwrap()).unwrap();
		let event = store.get_agent_inbox_event(id).await.unwrap();
		assert!(event.disposition.is_some(), "must not wake or deliver to model");
		let payload: Value = serde_json::from_str(&event.payload).unwrap();
		let key = payload["attemptId"].as_str().unwrap().to_owned();
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert!(store.reserve_agent_live_settings(attempt('c', Some(id))).await.unwrap().is_none());
		assert!(
			!store
				.finish_agent_live_settings(id, "different".into(), "applied".into())
				.await
				.unwrap()
		);
		assert!(store.finish_agent_live_settings(id, key.clone(), "unknown".into()).await.unwrap());
		assert!(!store.finish_agent_live_settings(id, key, "applied".into()).await.unwrap());
		let state = store
			.agent_live_settings_receipt("work".into(), "thread".into(), "turn".into())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(state.outcome, "unknown");
		let original = store.get_agent_inbox_event(id).await.unwrap();
		assert_eq!(original.payload, event.payload);
		assert_eq!(original.disposition_note.as_deref(), Some("reserved"));
		assert!(store.list_pending_agent_events(100).await.unwrap().is_empty());
		let mut replay = attempt('f', Some(id));
		replay.review_token = payload["reviewToken"].as_str().unwrap().into();
		replay.edit = AgentLiveSettingsEdit::Reviewer { reviewer: "auto_review".into() };
		assert!(store.reserve_agent_live_settings(replay).await.unwrap().is_none());
		assert!(store.reserve_agent_live_settings(attempt('d', None)).await.unwrap().is_none());
		let mut model = attempt('d', Some(id));
		model.edit =
			AgentLiveSettingsEdit::Model { model: "selected-model".into(), effort: "high".into() };
		let expected = model.edit.clone();
		let next = store.reserve_agent_live_settings(model).await.unwrap().unwrap();
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		let receipt = store
			.agent_live_settings_receipt("work".into(), "thread".into(), "turn".into())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(receipt.edit, expected);
		assert_eq!(receipt.outcome, "reserved");
		assert!(
			store.reserve_agent_live_settings(attempt('e', Some(next))).await.unwrap().is_none(),
			"reviewer edit must not overtake an unresolved model edit"
		);
		assert!(store.list_pending_agent_events(100).await.unwrap().is_empty());
		assert_ne!(next, id);
		let mut stale = attempt('e', Some(next));
		stale.turn_id = "old-turn".into();
		assert!(store.reserve_agent_live_settings(stale).await.is_err());
		let (transcript, _) = store.read_agent_transcript("work".into(), None, 1).await.unwrap();
		assert_eq!(transcript.len(), 1, "journal filtering must happen before the page limit");
		assert_eq!(transcript[0].id, visible.id);
		assert!(store.list_agent_wake_events("work".into(), 10).await.unwrap().is_empty());
		assert_eq!(
			store.get_agent_work_item("work".into()).await.unwrap().active_turn_id.as_deref(),
			Some("turn")
		);
	}
}
