//! Durable model selection attempts. A queued response never proves application.
#[path = "chief_model_history.rs"] mod history;
#[path = "chief_model_legacy.rs"] mod legacy;
use crate::{SqliteStore, StoreError, chief_process::owns_work, error::sqlite_error, unix_micros};
pub use history::ChiefModelHistory;
pub use legacy::ChiefLegacyModelPending;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefModelAttempt {
	pub work: String,
	pub thread: String,
	pub generation: Option<String>,
	pub settings_event: i64,
	pub model: String,
	pub model_provider: String,
	pub effort: Option<String>,
	pub review_token: String,
	pub attempt_id: String,
	/// Account identity captured by the manual selection owner. Older records omit it.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub manual_source: Option<ChiefManualModelSource>,
	/// Automatic recovery evidence. Absent in existing explicit selections.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub recovery: Option<ChiefModelRecoveryContext>,
}

/// Bind an explicit model choice to the account revision reviewed before reservation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefManualModelSource {
	pub account: String,
	pub account_revision: i64,
}

/// Bind an automatic fallback to its account, banner and complete target settings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefModelRecoveryContext {
	pub account: String,
	pub account_revision: i64,
	pub banner_digest: String,
	pub from_model: String,
	/// Expected configured tier, including an explicitly observed absence.
	pub service_tier: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefModelReceipt {
	pub id: i64,
	pub attempt: ChiefModelAttempt,
	pub state: String,
}

fn valid(text: &str, limit: usize) -> bool {
	!text.trim().is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
}

fn model_facts(settings: &Value) -> Option<(String, String, Option<String>)> {
	let optional = |field| match settings.get(field)? {
		Value::Null => Some(None),
		Value::String(value) if valid(value, 128) => Some(Some(value.clone())),
		_ => None,
	};
	let model = settings.get("model")?.as_str().filter(|s| valid(s, 256))?;
	let provider = settings.get("modelProvider")?.as_str().filter(|s| valid(s, 256))?;
	optional("serviceTier")?;
	Some((model.into(), provider.into(), optional("effort")?))
}

impl ChiefModelAttempt {
	fn validate(&self) -> Result<(), StoreError> {
		if [&self.work, &self.thread, &self.attempt_id]
			.into_iter()
			.chain(self.generation.iter())
			.any(|s| s.trim().is_empty() || s.len() > 512 || s.chars().any(char::is_control))
			|| !valid(&self.model, 256)
			|| self.model == "gpt-reserve"
			|| !valid(&self.model_provider, 256)
			|| self.effort.as_deref().is_some_and(|effort| !valid(effort, 128))
			|| self.settings_event <= 0
			|| self.review_token.len() != 64
			|| !self.review_token.bytes().all(|b| b.is_ascii_hexdigit())
		{
			return Err(StoreError::InvalidInput("invalid model selection"));
		}
		if let Some(source) = &self.manual_source
			&& (!valid(&source.account, 512)
				|| source.account_revision < 1
				|| self.generation.is_none()
				|| self.recovery.is_some())
		{
			return Err(StoreError::InvalidInput("invalid manual model source"));
		}
		if let Some(recovery) = &self.recovery
			&& (!valid(&recovery.account, 512)
				|| recovery.account_revision < 1
				|| !valid(&recovery.from_model, 256)
				|| recovery.from_model == self.model
				|| self.generation.is_none()
				|| self.effort.is_none()
				|| recovery.banner_digest.len() != 64
				|| !recovery.banner_digest.bytes().all(|b| b.is_ascii_hexdigit())
				|| recovery.service_tier.as_deref().is_some_and(|tier| !valid(tier, 128)))
		{
			return Err(StoreError::InvalidInput("invalid automatic model selection"));
		}
		Ok(())
	}

	fn key(&self) -> String {
		// A new request ID cannot replay an already reserved review.
		let identity = match &self.recovery {
			// A new process, account refresh or request key cannot replay the same fallback.
			Some(recovery) => json!([
				self.work,
				self.thread,
				recovery.account,
				recovery.banner_digest,
				recovery.from_model,
				self.model,
				self.effort,
				recovery.service_tier
			]),
			None => json!([self.work, self.thread, self.review_token]),
		};
		let digest: String = Sha256::digest(identity.to_string().as_bytes())
			.iter()
			.map(|b| format!("{b:02x}"))
			.collect();
		format!("model-selection:{digest}")
	}
}

pub(crate) fn pending(connection: &rusqlite::Connection, work: &str) -> Result<bool, StoreError> {
	if legacy::pending(connection, work)? {
		return Ok(true);
	}
	connection.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events e WHERE e.work_item_id=?1 AND e.event_kind='model_selection' AND NOT EXISTS(SELECT 1 FROM chief_inbox_events r WHERE (r.source_event_id=e.source_event_id||':result' AND r.event_kind='model_selection_result' AND json_extract(r.payload,'$.state')='rejected') OR (r.source_event_id=e.source_event_id||':observation' AND r.event_kind='model_selection_observation')))", [work], |r|r.get(0)).map_err(|e|sqlite_error(e).into())
}

fn recovery_source_is_current(
	connection: &rusqlite::Connection,
	attempt: &ChiefModelAttempt,
) -> Result<bool, StoreError> {
	let Some(recovery) = &attempt.recovery else { return Ok(true) };
	let current: bool = connection.query_row(
		"SELECT EXISTS(SELECT 1 FROM chief_work_items w JOIN chief_process_bindings b ON b.generation_id=?3 AND b.account_id=?4 JOIN accounts a ON a.account_id=b.account_id JOIN chief_inbox_events e ON e.id=?6 AND e.work_item_id=w.id WHERE w.id=?1 AND w.codex_thread_id=?2 AND w.dispatch_state='idle' AND w.active_turn_id IS NULL AND a.revision=?5 AND a.enabled=1 AND a.tombstoned_at_micros IS NULL AND e.event_kind='native_task_models' AND json_extract(e.payload,'$.settings.model')=?7)",
		params![attempt.work, attempt.thread, attempt.generation, recovery.account, recovery.account_revision, attempt.settings_event, recovery.from_model],
		|row| row.get(0),
	).map_err(sqlite_error)?;
	Ok(current)
}

fn queued_execution_input(
	connection: &rusqlite::Connection,
	work: &str,
) -> Result<bool, StoreError> {
	let explicit_input: bool = connection.query_row(
		"SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind='user_message' AND disposition IS NULL AND (delivered_turn_id IS NULL OR delivered_turn_id='') AND json_type(payload,'$.options.execution')='object' AND json_extract(payload,'$.options.execution')<>'{}')",
		[work], |row| row.get(0),
	).map_err(sqlite_error)?;
	Ok(explicit_input)
}

fn manual_source_is_current(
	connection: &rusqlite::Connection,
	generation: Option<&str>,
	source: Option<&ChiefManualModelSource>,
) -> Result<bool, StoreError> {
	let Some(source) = source else { return Ok(true) };
	connection.query_row(
		"SELECT EXISTS(SELECT 1 FROM chief_process_bindings b JOIN accounts a ON a.account_id=b.account_id WHERE b.generation_id=?1 AND b.account_id=?2 AND a.revision=?3 AND a.enabled=1 AND a.tombstoned_at_micros IS NULL)",
		params![generation, source.account, source.account_revision],
		|row| row.get(0),
	).map_err(|error| sqlite_error(error).into())
}

impl SqliteStore {
	/// Reserve one idle or running owned task edit against the exact saved observation. The caller
	/// must bind these facts to a current transport
	/// settings guard. Effort is the expected configured value, including preserved effort.
	pub async fn reserve_chief_model_selection(
		&self,
		attempt: ChiefModelAttempt,
	) -> Result<Option<i64>, StoreError> {
		attempt.validate()?;
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			if !owns_work(&tx,&attempt.work,attempt.generation.as_deref())? || crate::chief_prompt_edit::pending(&tx,&attempt.work)? || pending(&tx,&attempt.work)? || crate::chief_permissions::pending(&tx,&attempt.work)? || crate::chief_plugins::pending(&tx,&attempt.work)? { return Ok(None); }
			if !recovery_source_is_current(&tx, &attempt)? || !manual_source_is_current(&tx, attempt.generation.as_deref(), attempt.manual_source.as_ref())? || queued_execution_input(&tx, &attempt.work)? { return Ok(None); }
			let saved: Option<String> = tx.query_row("SELECT json_extract(e.payload,'$.settings') FROM chief_work_items w JOIN chief_inbox_events e ON e.work_item_id=w.id WHERE w.id=?1 AND w.codex_thread_id=?2 AND ((w.dispatch_state='idle' AND w.active_turn_id IS NULL) OR (w.dispatch_state='running' AND w.active_turn_id IS NOT NULL)) AND w.status<>'resolved' AND e.id=?4 AND e.event_kind='native_task_models' AND json_extract(e.payload,'$.threadId')=?2 AND json_extract(e.payload,'$.generationId') IS ?3 AND json_type(e.payload,'$.settings')='object' AND e.id=(SELECT max(n.id) FROM chief_inbox_events n WHERE n.work_item_id=w.id AND n.event_kind='native_task_models' AND json_extract(n.payload,'$.threadId')=?2 AND json_extract(n.payload,'$.generationId') IS ?3)", params![attempt.work,attempt.thread,attempt.generation,attempt.settings_event], |r|r.get(0)).optional().map_err(sqlite_error)?;
			let current = saved.and_then(|value| serde_json::from_str::<Value>(&value).ok()).and_then(|value| model_facts(&value));
			if current.is_none_or(|current| current.1 != attempt.model_provider || current == (attempt.model.clone(), attempt.model_provider.clone(), attempt.effort.clone())) { return Ok(None); }

			let conflict: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_misalignment WHERE work_id=?1)",[&attempt.work],|r|r.get(0)).map_err(sqlite_error)?;
			if conflict {return Ok(None);}
			let key=attempt.key();
            if attempt.recovery.is_some() {
                // The legacy journal used the same stable digest with a different prefix.
                // A terminal receipt still consumes that occurrence across upgrades.
                let legacy_key = key.replacen("model-selection:", "model-recovery:", 1);
                let used: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE source_event_id=?1 AND event_kind='model_recovery')", [&legacy_key], |row| row.get(0)).map_err(sqlite_error)?;
                if used { return Ok(None); }
            }
			let now=unix_micros()?;
			let changed=tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'model_selection',?3,?4,'resolved','Model selection reserved; not confirmed.',?4)",params![key,attempt.work,json!({"attempt":attempt}).to_string(),now]).map_err(sqlite_error)?;
			let id=tx.last_insert_rowid();tx.commit().map_err(sqlite_error)?;Ok((changed==1).then_some(id))
		}).await
	}

	/// Append one immutable RPC outcome; it cannot replace a separately observed target state.
	pub async fn finish_chief_model_selection(
		&self,
		event: i64,
		attempt: ChiefModelAttempt,
		state: String,
	) -> Result<bool, StoreError> {
		attempt.validate()?;
		if !matches!(state.as_str(), "queued" | "rejected" | "unknown") {
			return Err(StoreError::InvalidInput("invalid model response"));
		}
		self.run(move |connection| {
			let key=attempt.key();
			let expected=json!({"attempt":attempt}).to_string();
			Ok(connection.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT source_event_id||':result',work_item_id,'model_selection_result',?4,?5,'resolved','Native RPC outcome; application is separate.',?5 FROM chief_inbox_events WHERE id=?1 AND source_event_id=?2 AND payload=?3 AND event_kind='model_selection'",params![event,key,expected,json!({"reservation":event,"state":state}).to_string(),unix_micros()?]).map_err(sqlite_error)?==1)
		}).await
	}

	pub async fn chief_model_receipt(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<ChiefModelReceipt>, StoreError> {
		self.run(move |connection| {
			let row:Option<(i64,String,String)>=connection.query_row("SELECT e.id,json_extract(e.payload,'$.attempt'),COALESCE(json_extract(o.payload,'$.state'),json_extract(r.payload,'$.state'),'reserved') FROM chief_inbox_events e LEFT JOIN chief_inbox_events r ON r.source_event_id=e.source_event_id||':result' AND r.event_kind='model_selection_result' LEFT JOIN chief_inbox_events o ON o.source_event_id=e.source_event_id||':observation' AND o.event_kind='model_selection_observation' WHERE e.work_item_id=?1 AND e.event_kind='model_selection' AND json_extract(e.payload,'$.attempt.thread')=?2 ORDER BY e.id DESC LIMIT 1",params![work,thread],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(sqlite_error)?;
			row.map(|(id,attempt,state)|Ok(ChiefModelReceipt {id,attempt:serde_json::from_str(&attempt).map_err(|_|StoreError::InvalidInput("invalid saved model attempt"))?,state})).transpose()
		}).await
	}
}

/// The journal owner calls this with wire-current facts in the observation transaction.
/// A new owner can reconcile an old attempt only after the old process is confirmed dead.
pub(crate) fn observe(
	connection: &rusqlite::Connection,
	work: &str,
	thread: &str,
	generation: Option<&str>,
	observation: i64,
	settings: Option<&Value>,
) -> Result<(), StoreError> {
	let Some(settings) = settings else {
		return Ok(());
	};
	let Some(selected) = model_facts(settings) else {
		return Ok(());
	};
	if !owns_work(connection, work, generation)? {
		return Ok(());
	}
	if let Some(generation) = generation {
		let active_account: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM chief_process_bindings b JOIN accounts a ON a.account_id=b.account_id WHERE b.generation_id=?1 AND a.enabled=1 AND a.tombstoned_at_micros IS NULL)", [generation], |row| row.get(0)).map_err(sqlite_error)?;
		if !active_account {
			return Ok(());
		}
	}
	legacy::observe(connection, work, thread, generation, observation, settings)?;
	let row:Option<(i64,String,Option<String>,String)>=connection.query_row("SELECT e.id,e.source_event_id,json_extract(e.payload,'$.attempt.generation'),json_extract(e.payload,'$.attempt') FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id AND w.codex_thread_id=?2 WHERE e.work_item_id=?1 AND e.event_kind='model_selection' AND e.id<?3 AND json_extract(e.payload,'$.attempt.thread')=?2 AND NOT EXISTS(SELECT 1 FROM chief_inbox_events r WHERE (r.source_event_id=e.source_event_id||':result' AND json_extract(r.payload,'$.state')='rejected') OR r.source_event_id=e.source_event_id||':observation') ORDER BY e.id DESC LIMIT 1",params![work,thread,observation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sqlite_error)?;
	let Some((reservation, key, previous, target)) = row else {
		return Ok(());
	};
	let target: ChiefModelAttempt = serde_json::from_str(&target)
		.map_err(|_| StoreError::InvalidInput("invalid saved model target"))?;
	let matches = selected == (target.model, target.model_provider, target.effort)
		&& target.recovery.as_ref().is_none_or(|recovery| {
			settings.get("serviceTier").and_then(Value::as_str) == recovery.service_tier.as_deref()
		});
	let state = if previous.as_deref() == generation {
		if !manual_source_is_current(connection, generation, target.manual_source.as_ref())? {
			return Ok(());
		}
		if let Some(recovery) = &target.recovery {
			let current: bool = connection.query_row(
				"SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND revision=?2 AND enabled=1 AND tombstoned_at_micros IS NULL)",
				params![recovery.account, recovery.account_revision], |row| row.get(0),
			).map_err(sqlite_error)?;
			if !current {
				return Ok(());
			}
		}
		if !matches {
			return Ok(());
		}
		"target_observed"
	} else {
		let (Some(previous), Some(_current)) = (previous.as_deref(), generation) else {
			return Ok(());
		};
		let dead:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM process_generations g JOIN process_generation_death_evidence e ON e.evidence_id=g.death_evidence_id AND e.generation_id=g.generation_id WHERE g.generation_id=?1 AND g.state='dead')",[previous],|r|r.get(0)).map_err(sqlite_error)?;
		if !dead {
			return Ok(());
		}
		if matches && target.recovery.is_none() && target.manual_source.is_none() {
			"target_observed"
		} else {
			"superseded"
		}
	};
	let now = unix_micros()?;
	connection.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'model_selection_observation',?3,?4,'resolved','Current native models observed; prior request causation is not asserted.',?4)",params![format!("{key}:observation"),work,json!({"reservation":reservation,"settingsEvent":observation,"state":state,"generationId":generation}).to_string(),now]).map_err(sqlite_error)?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus};

	fn settings(model: &str) -> Value {
		json!({"model":model,"modelProvider":"fixture","effort":"high","serviceTier":null})
	}

	async fn setup(path: &std::path::Path) -> SqliteStore {
		let store = SqliteStore::open_test(path).unwrap();
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
		store
	}
	async fn facts(store: &SqliteStore, profile: Option<&str>, digest: char) -> i64 {
		store
			.record_chief_task_models_publication(
				"thread".into(),
				None,
				profile.map(|profile| settings(profile).to_string()),
				digest.to_string().repeat(64),
			)
			.await
			.unwrap()
			.unwrap()
	}
	fn attempt(settings_event: i64, token: char) -> ChiefModelAttempt {
		ChiefModelAttempt {
			work: "work".into(),
			thread: "thread".into(),
			generation: None,
			settings_event,
			model: "scoped".into(),
			model_provider: "fixture".into(),
			effort: Some("high".into()),
			review_token: token.to_string().repeat(64),
			attempt_id: format!("attempt-{token}"),
			manual_source: None,
			recovery: None,
		}
	}
	async fn receipt(store: &SqliteStore) -> ChiefModelReceipt {
		store.chief_model_receipt("work".into(), "thread".into()).await.unwrap().unwrap()
	}

	#[tokio::test]
	async fn legacy_model_recovery_blocks_dispatch_after_reopen() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("legacy.sqlite3");
		let store = setup(&path).await;
		store.run(|connection| {
			connection.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('model-recovery:legacy','work','model_recovery',?1,1,'resolved','Legacy fixture',1)", [json!({"attempt":{"work":"work","thread":"thread","generation":"old","account":"account","account_revision":1,"settings_event":1,"banner_digest":"b".repeat(64),"from_model":"original","model":"scoped","effort":"high","service_tier":"priority"},"state":"claimed"}).to_string()]).map_err(sqlite_error)?;
			Ok(())
		}).await.unwrap();
		drop(store);
		let reopened = SqliteStore::open_test(&path).unwrap();
		assert!(
			reopened.begin_chief_dispatch("work".into()).await.is_err(),
			"an unresolved legacy request must not be ignored after upgrade"
		);
	}

	#[tokio::test]
	async fn model_reservation_survives_crash_and_blocks_dispatch_until_native_confirmation() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("state.sqlite3");
		let store = setup(&path).await;
		let observed = facts(&store, Some("readonly"), 'a').await;
		let a = attempt(observed, 'a');
		let b = attempt(observed, 'b');
		let second = SqliteStore::open_test(&path).unwrap();
		let (one, two) = tokio::join!(
			store.reserve_chief_model_selection(a.clone()),
			second.reserve_chief_model_selection(b.clone())
		);
		assert_ne!(one.as_ref().unwrap().is_some(), two.as_ref().unwrap().is_some());
		let (id, winning) =
			if let Some(id) = one.unwrap() { (id, a) } else { (two.unwrap().unwrap(), b) };
		assert!(store.begin_chief_dispatch("work".into()).await.is_err());
		assert!(store.list_pending_chief_events(100).await.unwrap().is_empty());
		drop(second);
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert_eq!(receipt(&store).await.state, "reserved");
		assert!(store.begin_chief_tool_upgrade("work".into(), "thread".into()).await.is_err());
		assert!(
			store.reserve_chief_model_selection(attempt(observed, 'c')).await.unwrap().is_none()
		);
		assert!(
			store
				.finish_chief_model_selection(id, winning.clone(), "unknown".into())
				.await
				.unwrap()
		);
		assert!(!store.finish_chief_model_selection(id, winning, "queued".into()).await.unwrap());
		assert_eq!(receipt(&store).await.state, "unknown");
		facts(&store, None, 'b').await;
		assert_eq!(receipt(&store).await.state, "unknown");
		facts(&store, Some("other"), 'c').await;
		assert_eq!(receipt(&store).await.state, "unknown");
		assert!(store.begin_chief_dispatch("work".into()).await.is_err());
		store
			.record_chief_task_models(
				"thread".into(),
				None,
				Some(settings("scoped").to_string()),
				"d".repeat(64),
			)
			.await
			.unwrap();
		assert_eq!(
			receipt(&store).await.state,
			"unknown",
			"historical facts cannot settle a write"
		);
		facts(&store, Some("scoped"), 'd').await;
		assert_eq!(receipt(&store).await.state, "target_observed");
		assert!(store.begin_chief_dispatch("work".into()).await.is_ok());
		assert!(store.list_pending_chief_events(100).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn model_publication_before_ack_wins_and_rejected_review_cannot_replay() {
		let dir = tempfile::tempdir().unwrap();
		let store = setup(&dir.path().join("state.sqlite3")).await;
		let first = facts(&store, Some("readonly"), 'a').await;
		let newer = facts(&store, Some("other"), 'b').await;
		assert!(store.reserve_chief_model_selection(attempt(first, 'a')).await.unwrap().is_none());
		let a = attempt(newer, 'a');
		let id = store.reserve_chief_model_selection(a.clone()).await.unwrap().unwrap();
		let mut forged = a.clone();
		forged.model = "different".into();
		assert!(!store.finish_chief_model_selection(id, forged, "rejected".into()).await.unwrap());
		facts(&store, Some("scoped"), 'c').await;
		assert!(store.finish_chief_model_selection(id, a.clone(), "queued".into()).await.unwrap());
		assert_eq!(receipt(&store).await.state, "target_observed");
		let reverted = facts(&store, Some("readonly"), 'd').await;
		let mut replay = a;
		replay.settings_event = reverted;
		replay.attempt_id = "new-client-key".into();
		assert!(store.reserve_chief_model_selection(replay).await.unwrap().is_none());
		let next = attempt(reverted, 'b');
		let next_id = store.reserve_chief_model_selection(next.clone()).await.unwrap().unwrap();
		assert!(
			store
				.finish_chief_model_selection(next_id, next.clone(), "rejected".into())
				.await
				.unwrap()
		);
		facts(&store, Some("scoped"), 'e').await;
		assert_eq!(receipt(&store).await.state, "rejected");
		assert!(store.reserve_chief_model_selection(next).await.unwrap().is_none());
		assert!(store.begin_chief_dispatch("work".into()).await.is_ok());
	}
	#[tokio::test]
	async fn observation_revisions_preserve_reversions_without_consuming_transcript_pages() {
		let dir = tempfile::tempdir().unwrap();
		let store = setup(&dir.path().join("observations.sqlite3")).await;
		let message = store
			.record_chief_observation(crate::EnqueueChiefEvent {
				source_event_id: "answer".into(),
				work_item_id: "work".into(),
				event_kind: "assistant_message".into(),
				payload: json!({"text":"Visible answer"}).to_string(),
			})
			.await
			.unwrap();
		let first = facts(&store, Some("readonly"), 'a').await;
		assert_eq!(facts(&store, Some("readonly"), 'a').await, first);
		let second = facts(&store, Some("scoped"), 'b').await;
		let reverted = facts(&store, Some("readonly"), 'a').await;
		assert!(first < second && second < reverted);
		assert_eq!(
			store
				.chief_task_models("work".into(), "thread".into(), None)
				.await
				.unwrap()
				.unwrap()
				.id,
			reverted
		);
		assert!(
			store
				.chief_task_models("foreign".into(), "thread".into(), None)
				.await
				.unwrap()
				.is_none()
		);
		let attempt = attempt(reverted, 'a');
		let reserved = store.reserve_chief_model_selection(attempt.clone()).await.unwrap().unwrap();
		store.finish_chief_model_selection(reserved, attempt, "queued".into()).await.unwrap();
		facts(&store, Some("scoped"), 'b').await;
		assert_eq!(
			store.read_chief_work_events("work".into(), 1).await.unwrap(),
			vec![message.clone()]
		);
		assert_eq!(
			store.read_chief_transcript("work".into(), None, 1).await.unwrap().0,
			vec![message]
		);
	}
	#[tokio::test]
	async fn incomplete_model_facts_cannot_settle_and_null_effort_is_known() {
		let dir = tempfile::tempdir().unwrap();
		let store = setup(&dir.path().join("fields.sqlite3")).await;
		let event = facts(&store, Some("original"), 'a').await;
		let mut selected = attempt(event, 'a');
		selected.effort = None;
		let mut foreign_provider = selected.clone();
		foreign_provider.model_provider = "other-provider".into();
		assert!(store.reserve_chief_model_selection(foreign_provider).await.unwrap().is_none());
		let id = store.reserve_chief_model_selection(selected.clone()).await.unwrap().unwrap();
		for field in ["model", "modelProvider", "effort", "serviceTier"] {
			let mut partial = settings("scoped");
			partial["effort"] = Value::Null;
			partial.as_object_mut().unwrap().remove(field);
			store
				.record_chief_task_models_publication(
					"thread".into(),
					None,
					Some(partial.to_string()),
					"b".repeat(64),
				)
				.await
				.unwrap();
			assert_eq!(receipt(&store).await.state, "reserved");
		}
		let mut target = settings("scoped");
		target["effort"] = Value::Null;
		target["modelProvider"] = json!("other-provider");
		store
			.record_chief_task_models_publication(
				"thread".into(),
				None,
				Some(target.to_string()),
				"c".repeat(64),
			)
			.await
			.unwrap();
		assert_eq!(receipt(&store).await.state, "reserved");
		target["modelProvider"] = json!("fixture");
		store
			.record_chief_task_models_publication(
				"thread".into(),
				None,
				Some(target.to_string()),
				"c".repeat(64),
			)
			.await
			.unwrap();
		assert_eq!(receipt(&store).await.state, "target_observed");
		store.finish_chief_model_selection(id, selected, "unknown".into()).await.unwrap();
		assert_eq!(receipt(&store).await.state, "target_observed");
	}

	#[tokio::test]
	async fn running_model_edits_serialize_with_permission_and_plugin_edits() {
		let dir = tempfile::tempdir().unwrap();
		let store = setup(&dir.path().join("mixed.sqlite3")).await;
		let observed = facts(&store, Some("original"), 'a').await;
		let plugin_event = store
			.record_chief_task_plugins_publication(
				"thread".into(),
				None,
				Some(json!({"disabledPluginIds":[]}).to_string()),
				"a".repeat(64),
			)
			.await
			.unwrap()
			.unwrap();
		let permission_event = store
			.record_chief_task_permissions_publication(
				"thread".into(),
				None,
				Some(json!({"profileId":":read-only"}).to_string()),
				"a".repeat(64),
			)
			.await
			.unwrap()
			.unwrap();
		store.begin_chief_dispatch("work".into()).await.unwrap();
		assert!(
			store.reserve_chief_model_selection(attempt(observed, 'a')).await.unwrap().is_none()
		);
		store.acknowledge_chief_dispatch("work".into(), "active".into()).await.unwrap();
		let model = attempt(observed, 'a');
		let id = store.reserve_chief_model_selection(model.clone()).await.unwrap().unwrap();
		let plugin = crate::ChiefPluginAttempt {
			work: "work".into(),
			thread: "thread".into(),
			generation: None,
			settings_event: plugin_event,
			disabled_plugin_ids: vec!["scoped".into()],
			review_token: "b".repeat(64),
			attempt_id: "plugin".into(),
		};
		let permission = crate::ChiefPermissionAttempt {
			work: "work".into(),
			thread: "thread".into(),
			generation: None,
			settings_event: permission_event,
			profile: "scoped".into(),
			review_token: "c".repeat(64),
			attempt_id: "permission".into(),
		};
		assert!(store.reserve_chief_plugin_selection(plugin.clone()).await.unwrap().is_none());
		assert!(
			store.reserve_chief_permission_selection(permission.clone()).await.unwrap().is_none()
		);
		store.finish_chief_model_selection(id, model, "rejected".into()).await.unwrap();
		let id = store.reserve_chief_plugin_selection(plugin.clone()).await.unwrap().unwrap();
		assert!(
			store.reserve_chief_model_selection(attempt(observed, 'b')).await.unwrap().is_none()
		);
		store.finish_chief_plugin_selection(id, plugin, "rejected".into()).await.unwrap();
		let id =
			store.reserve_chief_permission_selection(permission.clone()).await.unwrap().unwrap();
		assert!(
			store.reserve_chief_model_selection(attempt(observed, 'b')).await.unwrap().is_none()
		);
		store.finish_chief_permission_selection(id, permission, "rejected".into()).await.unwrap();
		assert!(
			store.reserve_chief_model_selection(attempt(observed, 'b')).await.unwrap().is_some()
		);
		assert_eq!(
			store.get_chief_work_item("work".into()).await.unwrap().active_turn_id.as_deref(),
			Some("active")
		);
	}
}
