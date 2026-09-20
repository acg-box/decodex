//! One durable installation attempt per pending native suggestion; never a model wake.
use crate::{SqliteStore, StoreError, chief_process::owns_work, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// Exact pending-event identity and the account process that owns it.
pub struct ChiefInstallAttempt {
	pub event_id: i64,
	pub work_id: String,
	pub thread_id: String,
	pub generation_id: Option<String>,
	pub attempt_id: String,
	pub plugin_id: String,
}

/// Non-secret authorization requirements returned by a confirmed native installation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefInstallRequirements {
	pub auth_policy: String,
	pub connector_ids: Vec<String>,
}

impl SqliteStore {
	/// Attach facts only to the exact reserved attempt. Repeating the same receipt is harmless;
	/// conflicting receipts cannot replace the required authorization set.
	pub async fn record_chief_install_requirements(
		&self,
		event: i64,
		attempt: String,
		requirements: ChiefInstallRequirements,
	) -> Result<(), StoreError> {
		let mut ids = std::collections::HashSet::new();
		if !matches!(requirements.auth_policy.as_str(), "ON_INSTALL" | "ON_USE")
			|| requirements.connector_ids.len() > 128
			|| requirements.connector_ids.iter().any(|id| {
				id.trim().is_empty()
					|| id.len() > 1024
					|| id.chars().any(char::is_control)
					|| !ids.insert(id)
			}) {
			return Err(StoreError::InvalidInput("invalid installation requirements"));
		}
		let receipt = serde_json::to_string(&requirements)
			.map_err(|_| StoreError::InvalidInput("invalid installation requirements"))?;
		if receipt.len() > 32768 {
			return Err(StoreError::InvalidInput("installation requirements too large"));
		}
		self.run(move|connection| {
			let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let payload:String=tx.query_row("SELECT payload FROM chief_inbox_events WHERE id=?1",[event],|r|r.get(0)).map_err(sqlite_error)?;
			let source=install_source(&payload)?;
			let work:Option<String>=tx.query_row("SELECT work_item_id FROM chief_inbox_events WHERE source_event_id=?1 AND event_kind='plugin_install_attempt' AND json_extract(payload,'$.attemptId')=?2",params![source,attempt],|r|r.get(0)).optional().map_err(sqlite_error)?;
			let work=work.ok_or(StoreError::OwnershipLost("installation attempt"))?;
			let receipt_source=format!("{source}:receipt");
			let saved:Option<String>=tx.query_row("SELECT json_extract(payload,'$.requirements') FROM chief_inbox_events WHERE source_event_id=?1 AND event_kind='plugin_install_receipt'",[&receipt_source],|r|r.get(0)).optional().map_err(sqlite_error)?;
			if let Some(saved)=saved {return if saved==receipt {Ok(())} else {Err(StoreError::IdempotencyConflict)};}
			let now=unix_micros()?;
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'plugin_install_receipt',json_object('requestEventId',?3,'attemptId',?4,'requirements',json(?5)),?6,'resolved','Native installation requirements recorded; access must be verified separately.',?6)",params![receipt_source,work,event,attempt,receipt,now]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;Ok(())
		}).await
	}

	/// Missing receipt remains unknown; it is not an empty authorization requirement set.
	pub async fn chief_install_requirements(
		&self,
		event: i64,
	) -> Result<Option<ChiefInstallRequirements>, StoreError> {
		self.run(move|connection| {
			let payload:String=connection.query_row("SELECT payload FROM chief_inbox_events WHERE id=?1",[event],|r|r.get(0)).map_err(sqlite_error)?;
			let source=format!("{}:receipt",install_source(&payload)?);
			let receipt:Option<String>=connection.query_row("SELECT json_extract(payload,'$.requirements') FROM chief_inbox_events WHERE source_event_id=?1 AND event_kind='plugin_install_receipt'",[source],|r|r.get(0)).optional().map_err(sqlite_error)?;
			receipt.map(|s|serde_json::from_str(&s).map_err(|_|StoreError::InvalidInput("invalid saved installation requirements"))).transpose()
		}).await
	}

	/// Reserve before native mutation. False means a prior attempt already exists,
	/// including a crash after reservation; it never authorizes replay.
	pub async fn reserve_chief_install_attempt(
		&self,
		attempt: ChiefInstallAttempt,
	) -> Result<bool, StoreError> {
		let a = attempt;
		if a.event_id <= 0
			|| [&a.work_id, &a.thread_id, &a.attempt_id, &a.plugin_id]
				.iter()
				.any(|s| s.trim().is_empty() || s.len() > 4096 || s.chars().any(char::is_control))
		{
			return Err(StoreError::InvalidInput("invalid plugin installation attempt"));
		}
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let payload:Option<String>=tx.query_row("SELECT e.payload FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id WHERE e.id=?1 AND e.work_item_id=?2 AND w.codex_thread_id=?3 AND e.event_kind='server_request_pending' AND e.disposition IS NULL AND ((json_extract(e.payload,'$.ownerThreadId')=?3 AND json_extract(e.payload,'$.params.threadId')!=?3) OR json_extract(e.payload,'$.params.turnId') IS NULL OR (w.dispatch_state='running' AND json_extract(e.payload,'$.params.turnId')=w.active_turn_id))",params![a.event_id,a.work_id,a.thread_id],|r|r.get(0)).optional().map_err(sqlite_error)?;
			let valid=payload.as_deref().and_then(|s|serde_json::from_str::<Value>(s).ok()).is_some_and(|v| {
				v["method"]=="mcpServer/elicitation/request" && (v["params"]["threadId"]==a.thread_id || (v["ownerThreadId"]==a.thread_id && v["params"]["threadId"].as_str().is_some_and(|thread| !thread.is_empty())))
					&& v["params"]["serverName"]=="codex_apps"
					&& v["params"]["_meta"]["codex_approval_kind"]=="tool_suggestion"
					&& v["params"]["_meta"]["suggest_type"]=="install"
					&& v["params"]["_meta"]["tool_type"]=="plugin"
					&& v["params"]["_meta"]["tool_id"]==a.plugin_id
			});
			if !valid || !owns_work(&tx,&a.work_id,a.generation_id.as_deref())?
				|| tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_misalignment WHERE work_id=?1 AND thread_id=?2)",params![a.work_id,a.thread_id],|r|r.get::<_,bool>(0)).map_err(sqlite_error)? {
				return Err(StoreError::OwnershipLost("plugin installation suggestion"));
			}
			let source=install_source(payload.as_deref().ok_or(StoreError::OwnershipLost("installation request"))?)?;
			let prior:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE source_event_id=?1)",[&source],|r|r.get(0)).map_err(sqlite_error)?;
			if prior { return Ok(false); }
			let now=unix_micros()?;
			let payload=json!({"requestEventId":a.event_id,"threadId":a.thread_id,"generationId":a.generation_id,"attemptId":a.attempt_id,"pluginId":a.plugin_id}).to_string();
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'plugin_install_attempt',?3,?4,'resolved','Installation attempt reserved; inspect native state before any further action.',?4)",params![source,a.work_id,payload,now]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(true)
		}).await
	}

	/// Read only the attempt identity, never infer that installation succeeded.
	pub async fn chief_install_attempt_id(
		&self,
		event_id: i64,
	) -> Result<Option<String>, StoreError> {
		self.run(move|connection| {
			let payload:Option<String>=connection.query_row("SELECT payload FROM chief_inbox_events WHERE id=?1",[event_id],|r|r.get(0)).optional().map_err(sqlite_error)?;
			let Some(payload)=payload else {return Ok(None);};
			let source=install_source(&payload)?;
			connection.query_row("SELECT json_extract(payload,'$.attemptId') FROM chief_inbox_events WHERE source_event_id=?1 AND event_kind='plugin_install_attempt'",[source],|r|r.get(0)).optional().map_err(|e|sqlite_error(e).into())
		}).await
	}
}

fn install_source(payload: &str) -> Result<String, StoreError> {
	let value: Value = serde_json::from_str(payload)
		.map_err(|_| StoreError::InvalidInput("invalid installation request"))?;
	if !(value["id"].is_i64()
		|| value["id"].as_str().is_some_and(|s| !s.is_empty() && s.len() <= 4096))
		|| !value["params"]["threadId"].is_string()
		|| !value["params"]["_meta"]["tool_id"].is_string()
	{
		return Err(StoreError::InvalidInput("missing installation identity"));
	}
	// Native suggestion correlation survives transport request-ID changes. It is
	// distinct from the JSON-RPC ID used to answer the live request.
	let correlation = match &value["params"]["_meta"]["suggestion_id"] {
		Value::Null => json!({"requestId":value["id"]}),
		Value::String(id)
			if !id.is_empty() && id.len() <= 1024 && !id.chars().any(char::is_control) =>
			json!({"suggestionId":id}),
		_ => return Err(StoreError::InvalidInput("invalid installation suggestion identity")),
	};
	let identity = json!([
		value["params"]["threadId"],
		value["params"]["turnId"],
		correlation,
		value["params"]["_meta"]["tool_id"]
	]);
	let digest: String = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	Ok(format!("plugin-install-attempt:{digest}"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus, EnqueueChiefEvent,
	};
	async fn setup(store: &SqliteStore) -> i64 {
		store
			.create_chief_work_item(ChiefWorkItem {
				id: "work".into(),
				parent_goal_id: None,
				kind: ChiefWorkKind::Goal,
				title: "Install".into(),
				instructions: "Install the requested integration".into(),
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
		store.enqueue_chief_event(EnqueueChiefEvent{source_event_id:"request".into(),work_item_id:"work".into(),event_kind:"server_request_pending".into(),payload:json!({"id":"native-suggestion","method":"mcpServer/elicitation/request","params":{"threadId":"thread","serverName":"codex_apps","_meta":{"codex_approval_kind":"tool_suggestion","suggest_type":"install","tool_type":"plugin","tool_id":"sample@market","suggestion_id":"stable-tool-call"}}}).to_string()}).await.unwrap().id
	}
	fn attempt(event_id: i64, key: &str) -> ChiefInstallAttempt {
		ChiefInstallAttempt {
			event_id,
			work_id: "work".into(),
			thread_id: "thread".into(),
			generation_id: None,
			attempt_id: key.into(),
			plugin_id: "sample@market".into(),
		}
	}
	#[tokio::test]
	async fn install_attempt_is_single_even_across_concurrent_clients_and_restart() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("install.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		let event = setup(&store).await;
		let (one, two) = tokio::join!(
			store.reserve_chief_install_attempt(attempt(event, "one")),
			store.reserve_chief_install_attempt(attempt(event, "two"))
		);
		assert_ne!(one.unwrap(), two.unwrap());
		let saved = store.chief_install_attempt_id(event).await.unwrap().unwrap();
		assert!(store.list_chief_wake_events("work".into(), 32).await.unwrap().is_empty());
		assert!(store.get_chief_inbox_event(event).await.unwrap().disposition.is_none());
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert!(
			!store.reserve_chief_install_attempt(attempt(event, "new-client-key")).await.unwrap()
		);
		assert_eq!(store.chief_install_attempt_id(event).await.unwrap(), Some(saved));
		let original = store.get_chief_inbox_event(event).await.unwrap();
		let reconnected = store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "request-new-connection".into(),
				work_item_id: "work".into(),
				event_kind: "server_request_pending".into(),
				payload: original.payload,
			})
			.await
			.unwrap();
		assert!(
			!store
				.reserve_chief_install_attempt(attempt(reconnected.id, "new-event-key"))
				.await
				.unwrap()
		);
	}
	#[tokio::test]
	async fn receipt_only_requirements_survive_reconnect_and_cannot_be_replaced() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("receipts.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		let event = setup(&store).await;
		store.reserve_chief_install_attempt(attempt(event, "original-attempt")).await.unwrap();
		assert!(store.chief_install_requirements(event).await.unwrap().is_none());
		let requirements = ChiefInstallRequirements {
			auth_policy: "ON_INSTALL".into(),
			connector_ids: vec!["receipt-only".into()],
		};
		assert!(
			store
				.record_chief_install_requirements(
					event,
					"wrong-attempt".into(),
					requirements.clone()
				)
				.await
				.is_err()
		);
		for _ in 0..2 {
			store
				.record_chief_install_requirements(
					event,
					"original-attempt".into(),
					requirements.clone(),
				)
				.await
				.unwrap();
		}
		assert!(
			store
				.record_chief_install_requirements(
					event,
					"original-attempt".into(),
					ChiefInstallRequirements {
						auth_policy: "ON_USE".into(),
						connector_ids: vec![]
					}
				)
				.await
				.is_err()
		);
		let original = store.get_chief_inbox_event(event).await.unwrap();
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		let mut changed_transport: Value = serde_json::from_str(&original.payload).unwrap();
		changed_transport["id"] = json!(42);
		let reconnected = store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: "new-connection".into(),
				work_item_id: "work".into(),
				event_kind: "server_request_pending".into(),
				payload: changed_transport.to_string(),
			})
			.await
			.unwrap();
		assert_eq!(
			store.chief_install_requirements(reconnected.id).await.unwrap(),
			Some(requirements)
		);
		assert!(
			!store
				.reserve_chief_install_attempt(attempt(reconnected.id, "changed-rpc-id"))
				.await
				.unwrap(),
			"a new transport request ID must not replay the same native suggestion"
		);
		assert!(store.list_chief_wake_events("work".into(), 32).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn stale_foreign_and_resolved_suggestions_do_not_reserve() {
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("install.sqlite3")).unwrap();
		let event = setup(&store).await;
		for variant in 0..4 {
			let mut request = attempt(event, "attempt");
			match variant {
				0 => request.work_id = "other".into(),
				1 => request.thread_id = "other".into(),
				2 => request.plugin_id = "other".into(),
				_ => request.generation_id = Some("foreign-generation".into()),
			}
			assert!(store.reserve_chief_install_attempt(request).await.is_err());
		}
		assert!(store.chief_install_attempt_id(event).await.unwrap().is_none());
		store.acknowledge_chief_request_event(event).await.unwrap();
		assert!(store.reserve_chief_install_attempt(attempt(event, "attempt")).await.is_err());
	}
}
