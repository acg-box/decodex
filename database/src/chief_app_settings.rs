//! Durable, non-waking reservation for a reviewed connected-account setting edit.
use crate::{SqliteStore, StoreError, chief_process::owns_work, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

pub struct ChiefAppSettingsAttempt {
	pub event_id: i64,
	pub work_id: String,
	pub thread_id: String,
	pub generation_id: Option<String>,
	pub connector_id: String,
	pub link_id: String,
	pub review_token: String,
	pub field: String,
	pub value: Option<String>,
	pub attempt_id: String,
}

impl SqliteStore {
	/// Reserve the exact reviewed edit before native dispatch. False includes a prior
	/// crash after reservation and never authorizes replay with another client key.
	pub async fn reserve_chief_app_settings_attempt(
		&self,
		a: ChiefAppSettingsAttempt,
	) -> Result<bool, StoreError> {
		let allowed: &[&str] = match a.field.as_str() {
			"default_tools_approval_mode" => &["auto", "prompt", "writes", "approve"],
			"approvals_reviewer" => &["user", "auto_review"],
			_ => return Err(StoreError::InvalidInput("invalid account setting field")),
		};
		if a.event_id <= 0
			|| a.review_token.len() != 64
			|| !a.review_token.bytes().all(|b| b.is_ascii_hexdigit())
			|| a.value.as_deref().is_some_and(|v| !allowed.contains(&v))
			|| [&a.work_id, &a.thread_id, &a.connector_id, &a.link_id, &a.attempt_id]
				.iter()
				.any(|v| v.is_empty() || v.len() > 4096 || v.chars().any(char::is_control))
		{
			return Err(StoreError::InvalidInput("invalid account setting attempt"));
		}
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let payload: Option<String> = tx.query_row(
				"SELECT e.payload FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id WHERE e.id=?1 AND e.work_item_id=?2 AND w.codex_thread_id=?3 AND e.event_kind='server_request_pending' AND e.disposition IS NULL AND ((json_extract(e.payload,'$.ownerThreadId')=?3 AND json_extract(e.payload,'$.params.threadId')!=?3) OR json_extract(e.payload,'$.params.turnId') IS NULL OR (w.dispatch_state='running' AND json_extract(e.payload,'$.params.turnId')=w.active_turn_id))",
				params![a.event_id,a.work_id,a.thread_id], |r| r.get(0)).optional().map_err(sqlite_error)?;
			let valid = payload.as_deref().and_then(|p|serde_json::from_str::<Value>(p).ok()).is_some_and(|v| {
				v["method"]=="mcpServer/elicitation/request" && v["params"]["serverName"]=="codex_apps"
					&& (v["params"]["threadId"]==a.thread_id || (v["ownerThreadId"]==a.thread_id && v["params"]["threadId"].as_str().is_some_and(|s|!s.is_empty())))
					&& v["params"]["_meta"]["connector_id"]==a.connector_id && v["params"]["_meta"]["link_id"]==a.link_id
			});
			if !valid || !owns_work(&tx,&a.work_id,a.generation_id.as_deref())?
				|| tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_misalignment WHERE work_id=?1 AND thread_id=?2)",params![a.work_id,a.thread_id],|r|r.get::<_,bool>(0)).map_err(sqlite_error)? {
				return Err(StoreError::OwnershipLost("account settings request"));
			}
			let identity = json!([a.work_id,a.thread_id,a.connector_id,a.link_id,a.review_token,a.field,a.value]);
			let digest: String = Sha256::digest(identity.to_string().as_bytes()).iter().map(|b|format!("{b:02x}")).collect();
			let source = format!("app-setting-attempt:{digest}");
			let prior: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE source_event_id=?1)",[&source],|r|r.get(0)).map_err(sqlite_error)?;
			if prior { return Ok(false); }
			let now=unix_micros()?;
			let payload=json!({"requestEventId":a.event_id,"threadId":a.thread_id,"generationId":a.generation_id,"attemptId":a.attempt_id,"connectorId":a.connector_id,"linkId":a.link_id,"reviewToken":a.review_token,"field":a.field,"value":a.value}).to_string();
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'app_setting_attempt',?3,?4,'resolved','Setting write reserved; inspect native configuration before further action.',?4)",params![source,a.work_id,payload,now]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(true)
		}).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus, EnqueueChiefEvent,
	};

	fn attempt(event_id: i64, key: &str) -> ChiefAppSettingsAttempt {
		ChiefAppSettingsAttempt {
			event_id,
			work_id: "work".into(),
			thread_id: "thread".into(),
			generation_id: None,
			connector_id: "calendar".into(),
			link_id: " work.link ".into(),
			review_token: "a".repeat(64),
			field: "default_tools_approval_mode".into(),
			value: Some("prompt".into()),
			attempt_id: key.into(),
		}
	}

	#[tokio::test]
	async fn account_setting_attempt_is_durable_exact_and_single_across_clients() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("settings.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store
			.create_chief_work_item(ChiefWorkItem {
				id: "work".into(),
				parent_goal_id: None,
				kind: ChiefWorkKind::Goal,
				title: "Work".into(),
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
		let event=store.enqueue_chief_event(EnqueueChiefEvent { source_event_id:"request".into(),work_item_id:"work".into(),
			event_kind:"server_request_pending".into(),payload:json!({"id":"approval","method":"mcpServer/elicitation/request","params":{"threadId":"thread","serverName":"codex_apps","_meta":{"connector_id":"calendar","link_id":" work.link "},"tool_params":{"link_id":"unrelated"}}}).to_string() }).await.unwrap();
		let mut wrong = attempt(event.id, "wrong");
		wrong.link_id = "unrelated".into();
		assert!(store.reserve_chief_app_settings_attempt(wrong).await.is_err());
		let (a, b) = tokio::join!(
			store.reserve_chief_app_settings_attempt(attempt(event.id, "one")),
			store.reserve_chief_app_settings_attempt(attempt(event.id, "two"))
		);
		assert_ne!(a.unwrap(), b.unwrap());
		assert!(
			store.get_chief_inbox_event(event.id).await.unwrap().disposition.is_none(),
			"setting edits never answer the approval"
		);
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert!(
			!store
				.reserve_chief_app_settings_attempt(attempt(event.id, "after-crash"))
				.await
				.unwrap()
		);
		let mut invalid = attempt(event.id, "invalid");
		invalid.field = "model".into();
		assert!(store.reserve_chief_app_settings_attempt(invalid).await.is_err());
			let mut changed = attempt(event.id, "changed-owner");
			changed.thread_id = "replacement".into();
		changed.review_token = "b".repeat(64);
		assert!(store.reserve_chief_app_settings_attempt(changed).await.is_err());
	}
}
