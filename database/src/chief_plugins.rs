//! Durable plugin selection attempts. A queued response never proves application.
use crate::{SqliteStore, StoreError, chief_process::owns_work, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefPluginAttempt {
	pub work: String,
	pub thread: String,
	pub generation: Option<String>,
	pub settings_event: i64,
	pub disabled_plugin_ids: Vec<String>,
	pub review_token: String,
	pub attempt_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefPluginReceipt {
	pub id: i64,
	pub attempt: ChiefPluginAttempt,
	pub state: String,
}

fn valid_ids(ids: &[String]) -> bool {
	ids.len() <= 128
		&& ids.iter().map(String::len).sum::<usize>() <= 32768
		&& ids
			.iter()
			.all(|s| !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
		&& ids.iter().collect::<std::collections::HashSet<_>>().len() == ids.len()
}

impl ChiefPluginAttempt {
	fn validate(&self) -> Result<(), StoreError> {
		if [&self.work, &self.thread, &self.attempt_id]
			.into_iter()
			.chain(self.generation.iter())
			.any(|s| s.trim().is_empty() || s.len() > 512 || s.chars().any(char::is_control))
			|| !valid_ids(&self.disabled_plugin_ids)
			|| self.settings_event <= 0
			|| self.review_token.len() != 64
			|| !self.review_token.bytes().all(|b| b.is_ascii_hexdigit())
		{
			return Err(StoreError::InvalidInput("invalid plugin selection"));
		}
		Ok(())
	}

	fn key(&self) -> String {
		// A new request ID cannot replay an already reserved review.
		let identity = json!([self.work, self.thread, self.review_token]);
		let digest: String = Sha256::digest(identity.to_string().as_bytes())
			.iter()
			.map(|b| format!("{b:02x}"))
			.collect();
		format!("plugin-selection:{digest}")
	}
}

pub(crate) fn pending(connection: &rusqlite::Connection, work: &str) -> Result<bool, StoreError> {
	connection.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events e WHERE e.work_item_id=?1 AND e.event_kind='plugin_selection' AND NOT EXISTS(SELECT 1 FROM chief_inbox_events r WHERE (r.source_event_id=e.source_event_id||':result' AND r.event_kind='plugin_selection_result' AND json_extract(r.payload,'$.state')='rejected') OR (r.source_event_id=e.source_event_id||':observation' AND r.event_kind='plugin_selection_observation')))", [work], |r|r.get(0)).map_err(|e|sqlite_error(e).into())
}

impl SqliteStore {
	/// Reserve one idle or running owned task edit against the exact saved observation. The caller
	/// must also preserve unrelated exclusions and bind these facts to a current transport
	/// settings guard.
	pub async fn reserve_chief_plugin_selection(
		&self,
		attempt: ChiefPluginAttempt,
	) -> Result<Option<i64>, StoreError> {
		attempt.validate()?;
		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			if !owns_work(&tx,&attempt.work,attempt.generation.as_deref())? || pending(&tx,&attempt.work)? || crate::chief_permissions::pending(&tx,&attempt.work)? { return Ok(None); }
			let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items w JOIN chief_inbox_events e ON e.work_item_id=w.id WHERE w.id=?1 AND w.codex_thread_id=?2 AND ((w.dispatch_state='idle' AND w.active_turn_id IS NULL) OR (w.dispatch_state='running' AND w.active_turn_id IS NOT NULL)) AND w.status<>'resolved' AND e.id=?4 AND e.event_kind='native_task_plugins' AND json_extract(e.payload,'$.threadId')=?2 AND json_extract(e.payload,'$.generationId') IS ?3 AND json_type(e.payload,'$.settings.disabledPluginIds')='array' AND json_extract(e.payload,'$.settings.disabledPluginIds') IS NOT ?5 AND e.id=(SELECT max(n.id) FROM chief_inbox_events n WHERE n.work_item_id=w.id AND n.event_kind='native_task_plugins' AND json_extract(n.payload,'$.threadId')=?2 AND json_extract(n.payload,'$.generationId') IS ?3))",params![attempt.work,attempt.thread,attempt.generation,attempt.settings_event,serde_json::to_string(&attempt.disabled_plugin_ids).expect("serializable selection")],|r|r.get(0)).map_err(sqlite_error)?;
			if !valid {return Ok(None);}
			let conflict: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_misalignment WHERE work_id=?1)",[&attempt.work],|r|r.get(0)).map_err(sqlite_error)?;
			if conflict {return Ok(None);}
			let key=attempt.key();
			let now=unix_micros()?;
			let changed=tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'plugin_selection',?3,?4,'resolved','Plugin selection reserved; not confirmed.',?4)",params![key,attempt.work,json!({"attempt":attempt}).to_string(),now]).map_err(sqlite_error)?;
			let id=tx.last_insert_rowid();tx.commit().map_err(sqlite_error)?;Ok((changed==1).then_some(id))
		}).await
	}

	/// Append one immutable RPC outcome; it cannot replace a separately observed target state.
	pub async fn finish_chief_plugin_selection(
		&self,
		event: i64,
		attempt: ChiefPluginAttempt,
		state: String,
	) -> Result<bool, StoreError> {
		attempt.validate()?;
		if !matches!(state.as_str(), "queued" | "rejected" | "unknown") {
			return Err(StoreError::InvalidInput("invalid plugin response"));
		}
		self.run(move |connection| {
			let key=attempt.key();
			let expected=json!({"attempt":attempt}).to_string();
			Ok(connection.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT source_event_id||':result',work_item_id,'plugin_selection_result',?4,?5,'resolved','Native RPC outcome; application is separate.',?5 FROM chief_inbox_events WHERE id=?1 AND source_event_id=?2 AND payload=?3 AND event_kind='plugin_selection'",params![event,key,expected,json!({"reservation":event,"state":state}).to_string(),unix_micros()?]).map_err(sqlite_error)?==1)
		}).await
	}

	pub async fn chief_plugin_receipt(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<ChiefPluginReceipt>, StoreError> {
		self.run(move |connection| {
			let row:Option<(i64,String,String)>=connection.query_row("SELECT e.id,json_extract(e.payload,'$.attempt'),COALESCE(json_extract(o.payload,'$.state'),json_extract(r.payload,'$.state'),'reserved') FROM chief_inbox_events e LEFT JOIN chief_inbox_events r ON r.source_event_id=e.source_event_id||':result' AND r.event_kind='plugin_selection_result' LEFT JOIN chief_inbox_events o ON o.source_event_id=e.source_event_id||':observation' AND o.event_kind='plugin_selection_observation' WHERE e.work_item_id=?1 AND e.event_kind='plugin_selection' AND json_extract(e.payload,'$.attempt.thread')=?2 ORDER BY e.id DESC LIMIT 1",params![work,thread],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(sqlite_error)?;
			row.map(|(id,attempt,state)|Ok(ChiefPluginReceipt {id,attempt:serde_json::from_str(&attempt).map_err(|_|StoreError::InvalidInput("invalid saved plugin attempt"))?,state})).transpose()
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
	let Some(mut selected) = settings
		.get("disabledPluginIds")
		.and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
		.filter(|ids| valid_ids(ids))
	else {
		return Ok(());
	};
	selected.sort_unstable();
	if !owns_work(connection, work, generation)? {
		return Ok(());
	}
	let row:Option<(i64,String,Option<String>,String)>=connection.query_row("SELECT e.id,e.source_event_id,json_extract(e.payload,'$.attempt.generation'),json_extract(e.payload,'$.attempt.disabled_plugin_ids') FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id AND w.codex_thread_id=?2 WHERE e.work_item_id=?1 AND e.event_kind='plugin_selection' AND e.id<?3 AND json_extract(e.payload,'$.attempt.thread')=?2 AND NOT EXISTS(SELECT 1 FROM chief_inbox_events r WHERE (r.source_event_id=e.source_event_id||':result' AND json_extract(r.payload,'$.state')='rejected') OR r.source_event_id=e.source_event_id||':observation') ORDER BY e.id DESC LIMIT 1",params![work,thread,observation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sqlite_error)?;
	let Some((reservation, key, previous, target)) = row else {
		return Ok(());
	};
	let mut target: Vec<String> = serde_json::from_str(&target)
		.map_err(|_| StoreError::InvalidInput("invalid saved plugin target"))?;
	target.sort_unstable();
	let matches = selected == target;
	let state = if previous.as_deref() == generation {
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
		if matches { "target_observed" } else { "superseded" }
	};
	let now = unix_micros()?;
	connection.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'plugin_selection_observation',?3,?4,'resolved','Current native plugins observed; prior request causation is not asserted.',?4)",params![format!("{key}:observation"),work,json!({"reservation":reservation,"settingsEvent":observation,"state":state,"generationId":generation}).to_string(),now]).map_err(sqlite_error)?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus};

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
			.record_chief_task_plugins_publication(
				"thread".into(),
				None,
				profile.map(|profile| json!({"disabledPluginIds":[profile]}).to_string()),
				digest.to_string().repeat(64),
			)
			.await
			.unwrap()
			.unwrap()
	}
	fn attempt(settings_event: i64, token: char) -> ChiefPluginAttempt {
		ChiefPluginAttempt {
			work: "work".into(),
			thread: "thread".into(),
			generation: None,
			settings_event,
			disabled_plugin_ids: vec!["scoped".into()],
			review_token: token.to_string().repeat(64),
			attempt_id: format!("attempt-{token}"),
		}
	}
	async fn receipt(store: &SqliteStore) -> ChiefPluginReceipt {
		store.chief_plugin_receipt("work".into(), "thread".into()).await.unwrap().unwrap()
	}

	#[tokio::test]
	async fn plugin_reservation_survives_crash_and_blocks_dispatch_until_native_confirmation() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("state.sqlite3");
		let store = setup(&path).await;
		let observed = facts(&store, Some("readonly"), 'a').await;
		let a = attempt(observed, 'a');
		let b = attempt(observed, 'b');
		let (one, two) = tokio::join!(
			store.reserve_chief_plugin_selection(a.clone()),
			store.reserve_chief_plugin_selection(b.clone())
		);
		assert_ne!(one.as_ref().unwrap().is_some(), two.as_ref().unwrap().is_some());
		let (id, winning) =
			if let Some(id) = one.unwrap() { (id, a) } else { (two.unwrap().unwrap(), b) };
		assert!(store.begin_chief_dispatch("work".into()).await.is_err());
		assert!(store.list_pending_chief_events(100).await.unwrap().is_empty());
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert_eq!(receipt(&store).await.state, "reserved");
		assert!(store.begin_chief_tool_upgrade("work".into(), "thread".into()).await.is_err());
		assert!(
			store.reserve_chief_plugin_selection(attempt(observed, 'c')).await.unwrap().is_none()
		);
		assert!(
			store
				.finish_chief_plugin_selection(id, winning.clone(), "unknown".into())
				.await
				.unwrap()
		);
		assert!(!store.finish_chief_plugin_selection(id, winning, "queued".into()).await.unwrap());
		assert_eq!(receipt(&store).await.state, "unknown");
		facts(&store, None, 'b').await;
		assert_eq!(receipt(&store).await.state, "unknown");
		facts(&store, Some("other"), 'c').await;
		assert_eq!(receipt(&store).await.state, "unknown");
		assert!(store.begin_chief_dispatch("work".into()).await.is_err());
		store
			.record_chief_task_plugins(
				"thread".into(),
				None,
				Some(json!({"disabledPluginIds":["scoped"]}).to_string()),
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
	async fn plugin_publication_before_ack_wins_and_rejected_review_cannot_replay() {
		let dir = tempfile::tempdir().unwrap();
		let store = setup(&dir.path().join("state.sqlite3")).await;
		let first = facts(&store, Some("readonly"), 'a').await;
		let newer = facts(&store, Some("other"), 'b').await;
		assert!(store.reserve_chief_plugin_selection(attempt(first, 'a')).await.unwrap().is_none());
		let a = attempt(newer, 'a');
		let id = store.reserve_chief_plugin_selection(a.clone()).await.unwrap().unwrap();
		let mut forged = a.clone();
		forged.disabled_plugin_ids = vec!["different".into()];
		assert!(!store.finish_chief_plugin_selection(id, forged, "rejected".into()).await.unwrap());
		facts(&store, Some("scoped"), 'c').await;
		assert!(store.finish_chief_plugin_selection(id, a.clone(), "queued".into()).await.unwrap());
		assert_eq!(receipt(&store).await.state, "target_observed");
		let reverted = facts(&store, Some("readonly"), 'd').await;
		let mut replay = a;
		replay.settings_event = reverted;
		replay.attempt_id = "new-client-key".into();
		assert!(store.reserve_chief_plugin_selection(replay).await.unwrap().is_none());
		let next = attempt(reverted, 'b');
		let next_id = store.reserve_chief_plugin_selection(next.clone()).await.unwrap().unwrap();
		assert!(
			store
				.finish_chief_plugin_selection(next_id, next.clone(), "rejected".into())
				.await
				.unwrap()
		);
		facts(&store, Some("scoped"), 'e').await;
		assert_eq!(receipt(&store).await.state, "rejected");
		assert!(store.reserve_chief_plugin_selection(next).await.unwrap().is_none());
		assert!(store.begin_chief_dispatch("work".into()).await.is_ok());
	}
	#[tokio::test]
	async fn running_plugin_and_permission_edits_cannot_race() {
		let dir = tempfile::tempdir().unwrap();
		let store = setup(&dir.path().join("mixed.sqlite3")).await;
		let plugin_event = facts(&store, Some("original"), 'a').await;
		let permission_event=store.record_chief_task_permissions_publication("thread".into(),None,Some(json!({"profileId":":read-only","cwd":"/native","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"}}).to_string()),"a".repeat(64)).await.unwrap().unwrap();
		store.begin_chief_dispatch("work".into()).await.unwrap();
		assert!(
			store
				.reserve_chief_plugin_selection(attempt(plugin_event, 'a'))
				.await
				.unwrap()
				.is_none()
		);
		store.acknowledge_chief_dispatch("work".into(), "active".into()).await.unwrap();
		let plugin = attempt(plugin_event, 'a');
		let id = store.reserve_chief_plugin_selection(plugin.clone()).await.unwrap().unwrap();
		let permission = crate::ChiefPermissionAttempt {
			work: "work".into(),
			thread: "thread".into(),
			generation: None,
			settings_event: permission_event,
			profile: "scoped".into(),
			review_token: "b".repeat(64),
			attempt_id: "permission".into(),
		};
		assert!(
			store.reserve_chief_permission_selection(permission.clone()).await.unwrap().is_none()
		);
		store.finish_chief_plugin_selection(id, plugin, "rejected".into()).await.unwrap();
		let id =
			store.reserve_chief_permission_selection(permission.clone()).await.unwrap().unwrap();
		assert!(
			store
				.reserve_chief_plugin_selection(attempt(plugin_event, 'b'))
				.await
				.unwrap()
				.is_none()
		);
		store.finish_chief_permission_selection(id, permission, "rejected".into()).await.unwrap();
		assert!(
			store
				.reserve_chief_plugin_selection(attempt(plugin_event, 'b'))
				.await
				.unwrap()
				.is_some()
		);
		assert_eq!(
			store.get_chief_work_item("work".into()).await.unwrap().active_turn_id.as_deref(),
			Some("active")
		);
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
				.chief_task_plugins("work".into(), "thread".into(), None)
				.await
				.unwrap()
				.unwrap()
				.id,
			reverted
		);
		assert!(
			store
				.chief_task_plugins("foreign".into(), "thread".into(), None)
				.await
				.unwrap()
				.is_none()
		);
		let attempt = attempt(reverted, 'a');
		let reserved =
			store.reserve_chief_plugin_selection(attempt.clone()).await.unwrap().unwrap();
		store.finish_chief_plugin_selection(reserved, attempt, "queued".into()).await.unwrap();
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
}
