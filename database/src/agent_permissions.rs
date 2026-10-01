//! Durable permission selection attempts. A queued response never proves application.
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	SqliteStore, StoreError, agent_models, agent_plugins, agent_process, agent_prompt_edit, error,
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentPermissionAttempt {
	pub work: String,
	pub thread: String,
	pub generation: Option<String>,
	pub settings_event: i64,
	pub profile: String,
	pub review_token: String,
	pub attempt_id: String,
}
impl AgentPermissionAttempt {
	fn validate(&self) -> Result<(), StoreError> {
		if [&self.work, &self.thread, &self.profile, &self.attempt_id]
			.into_iter()
			.chain(self.generation.iter())
			.any(|s| s.trim().is_empty() || s.len() > 512 || s.chars().any(char::is_control))
			|| self.profile.len() > 256
			|| self.settings_event <= 0
			|| self.review_token.len() != 64
			|| !self.review_token.bytes().all(|b| b.is_ascii_hexdigit())
		{
			return Err(StoreError::InvalidInput("invalid permission selection"));
		}

		Ok(())
	}

	fn key(&self) -> String {
		// A new request ID cannot replay an already reserved review.
		let identity = serde_json::json!([self.work, self.thread, self.review_token]);
		let digest: String = Sha256::digest(identity.to_string().as_bytes())
			.iter()
			.map(|b| format!("{b:02x}"))
			.collect();

		format!("permission-selection:{digest}")
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPermissionReceipt {
	pub id: i64,
	pub attempt: AgentPermissionAttempt,
	pub state: String,
}

impl SqliteStore {
	/// Reserve one owned task edit while idle or running
	/// against the exact saved observation. The caller must also validate the native catalog and
	/// bind these facts to a current transport settings guard.
	pub async fn reserve_agent_permission_selection(
		&self,
		attempt: AgentPermissionAttempt,
	) -> Result<Option<i64>, StoreError> {
		attempt.validate()?;

		self.run(move |connection| {
			let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;

			if !agent_process::owns_work(&tx,&attempt.work,attempt.generation.as_deref())? || agent_prompt_edit::pending(&tx,&attempt.work)? || pending(&tx,&attempt.work)? || agent_models::pending(&tx,&attempt.work)? || agent_plugins::pending(&tx,&attempt.work)? { return Ok(None); }

			let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items w JOIN agent_inbox_events e ON e.work_item_id=w.id WHERE w.id=?1 AND w.codex_thread_id=?2 AND ((w.dispatch_state='idle' AND w.active_turn_id IS NULL) OR (w.dispatch_state='running' AND w.active_turn_id IS NOT NULL)) AND w.status<>'resolved' AND e.id=?4 AND e.event_kind='native_task_permissions' AND json_extract(e.payload,'$.threadId')=?2 AND json_extract(e.payload,'$.generationId') IS ?3 AND json_type(e.payload,'$.settings')='object' AND json_extract(e.payload,'$.settings.profileId') IS NOT ?5 AND e.id=(SELECT max(n.id) FROM agent_inbox_events n WHERE n.work_item_id=w.id AND n.event_kind='native_task_permissions' AND json_extract(n.payload,'$.threadId')=?2 AND json_extract(n.payload,'$.generationId') IS ?3))",rusqlite::params![attempt.work,attempt.thread,attempt.generation,attempt.settings_event,attempt.profile],|r|r.get(0)).map_err(error::sqlite_error)?;

			if !valid {return Ok(None);}

			let conflict: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_misalignment WHERE work_id=?1)",[&attempt.work],|r|r.get(0)).map_err(error::sqlite_error)?;

			if conflict {return Ok(None);}

			let key=attempt.key();
			let now=crate::unix_micros()?;
			let changed=tx.execute("INSERT OR IGNORE INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'permission_selection',?3,?4,'resolved','Permission selection reserved; not confirmed.',?4)",rusqlite::params![key,attempt.work,serde_json::json!({"attempt":attempt}).to_string(),now]).map_err(error::sqlite_error)?;

			let id=tx.last_insert_rowid();tx.commit().map_err(error::sqlite_error)?;Ok((changed==1).then_some(id))
		}).await
	}

	/// Append one immutable RPC outcome; it cannot replace a separately observed target state.
	pub async fn finish_agent_permission_selection(
		&self,
		event: i64,
		attempt: AgentPermissionAttempt,
		state: String,
	) -> Result<bool, StoreError> {
		attempt.validate()?;

		if !matches!(state.as_str(), "queued" | "rejected" | "unknown") {
			return Err(StoreError::InvalidInput("invalid permission response"));
		}

		self.run(move |connection| {
			let key=attempt.key();
			let expected=serde_json::json!({"attempt":attempt}).to_string();

			Ok(connection.execute("INSERT OR IGNORE INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT source_event_id||':result',work_item_id,'permission_selection_result',?4,?5,'resolved','Native RPC outcome; application is separate.',?5 FROM agent_inbox_events WHERE id=?1 AND source_event_id=?2 AND payload=?3 AND event_kind='permission_selection'",rusqlite::params![event,key,expected,serde_json::json!({"reservation":event,"state":state}).to_string(),crate::unix_micros()?]).map_err(error::sqlite_error)?==1)
		}).await
	}

	pub async fn agent_permission_receipt(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<AgentPermissionReceipt>, StoreError> {
		self.run(move |connection| {
			let row:Option<(i64,String,String)>=connection.query_row("SELECT e.id,json_extract(e.payload,'$.attempt'),COALESCE(json_extract(o.payload,'$.state'),json_extract(r.payload,'$.state'),'reserved') FROM agent_inbox_events e LEFT JOIN agent_inbox_events r ON r.source_event_id=e.source_event_id||':result' AND r.event_kind='permission_selection_result' LEFT JOIN agent_inbox_events o ON o.source_event_id=e.source_event_id||':observation' AND o.event_kind='permission_selection_observation' WHERE e.work_item_id=?1 AND e.event_kind='permission_selection' AND json_extract(e.payload,'$.attempt.thread')=?2 ORDER BY e.id DESC LIMIT 1",rusqlite::params![work,thread],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(error::sqlite_error)?;

			row.map(|(id,attempt,state)|Ok(AgentPermissionReceipt {id,attempt:serde_json::from_str(&attempt).map_err(|_|StoreError::InvalidInput("invalid saved permission attempt"))?,state})).transpose()
		}).await
	}
}

pub(crate) fn pending(connection: &Connection, work: &str) -> Result<bool, StoreError> {
	connection.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events e WHERE e.work_item_id=?1 AND e.event_kind='permission_selection' AND NOT EXISTS(SELECT 1 FROM agent_inbox_events r WHERE (r.source_event_id=e.source_event_id||':result' AND r.event_kind='permission_selection_result' AND json_extract(r.payload,'$.state')='rejected') OR (r.source_event_id=e.source_event_id||':observation' AND r.event_kind='permission_selection_observation')))", [work], |r|r.get(0)).map_err(|e|error::sqlite_error(e).into())
}

/// The journal owner calls this with wire-current facts in the observation transaction.
/// A new owner can reconcile an old attempt only after the old process is confirmed dead.
pub(crate) fn observe(
	connection: &Connection,
	work: &str,
	thread: &str,
	generation: Option<&str>,
	observation: i64,
	settings: Option<&Value>,
) -> Result<(), StoreError> {
	let Some(settings) = settings else {
		return Ok(());
	};
	let text =
		|key: &str| settings.get(key).and_then(Value::as_str).is_some_and(|v| !v.trim().is_empty());

	if !text("cwd")
		|| !text("approvalsReviewer")
		|| !settings.get("approvalPolicy").is_some_and(|v| v.is_string() || v.is_object())
		|| !settings
			.get("sandboxPolicy")
			.and_then(|v| v.get("type"))
			.and_then(Value::as_str)
			.is_some_and(|v| !v.trim().is_empty())
	{
		return Ok(());
	}

	let profile = match settings.get("profileId") {
		Some(Value::String(profile)) => Some(profile.as_str()),
		Some(Value::Null) => None,
		_ => return Ok(()),
	};

	if !agent_process::owns_work(connection, work, generation)? {
		return Ok(());
	}

	let row:Option<(i64,String,Option<String>,String)>=connection.query_row("SELECT e.id,e.source_event_id,json_extract(e.payload,'$.attempt.generation'),json_extract(e.payload,'$.attempt.profile') FROM agent_inbox_events e JOIN agent_work_items w ON w.id=e.work_item_id AND w.codex_thread_id=?2 WHERE e.work_item_id=?1 AND e.event_kind='permission_selection' AND e.id<?3 AND json_extract(e.payload,'$.attempt.thread')=?2 AND NOT EXISTS(SELECT 1 FROM agent_inbox_events r WHERE (r.source_event_id=e.source_event_id||':result' AND json_extract(r.payload,'$.state')='rejected') OR r.source_event_id=e.source_event_id||':observation') ORDER BY e.id DESC LIMIT 1",rusqlite::params![work,thread,observation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(error::sqlite_error)?;
	let Some((reservation, key, previous, target)) = row else {
		return Ok(());
	};
	let matches = profile == Some(target.as_str());
	let state = if previous.as_deref() == generation {
		if !matches {
			return Ok(());
		}
		"target_observed"
	} else {
		let (Some(previous), Some(_current)) = (previous.as_deref(), generation) else {
			return Ok(());
		};
		let dead:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM process_generations g JOIN process_generation_death_evidence e ON e.evidence_id=g.death_evidence_id AND e.generation_id=g.generation_id WHERE g.generation_id=?1 AND g.state='dead')",[previous],|r|r.get(0)).map_err(error::sqlite_error)?;

		if !dead {
			return Ok(());
		}

		if matches { "target_observed" } else { "superseded" }
	};
	let now = crate::unix_micros()?;

	connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'permission_selection_observation',?3,?4,'resolved','Current native permissions observed; prior request causation is not asserted.',?4)",rusqlite::params![format!("{key}:observation"),work,serde_json::json!({"reservation":reservation,"settingsEvent":observation,"state":state,"generationId":generation}).to_string(),now]).map_err(error::sqlite_error)?;

	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::{
		AgentDispatchState, AgentPermissionAttempt, AgentPermissionReceipt, EnqueueAgentEvent,
		SqliteStore, tests,
	};

	fn attempt(settings_event: i64, token: char) -> AgentPermissionAttempt {
		AgentPermissionAttempt {
			work: "work".into(),
			thread: "thread".into(),
			generation: None,
			settings_event,
			profile: "scoped".into(),
			review_token: token.to_string().repeat(64),
			attempt_id: format!("attempt-{token}"),
		}
	}

	async fn facts(store: &SqliteStore, profile: Option<&str>, digest: char) -> i64 {
		store.record_agent_task_permissions_publication("thread".into(),None,profile.map(|profile|serde_json::json!({"profileId":profile,"cwd":"/native","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"}}).to_string()),digest.to_string().repeat(64)).await.unwrap().unwrap()
	}
	async fn receipt(store: &SqliteStore) -> AgentPermissionReceipt {
		store.agent_permission_receipt("work".into(), "thread".into()).await.unwrap().unwrap()
	}

	#[tokio::test]
	async fn running_permission_reservation_accepts_named_profiles_and_does_not_replay() {
		let dir = tempfile::tempdir().expect("fixture");
		let path = dir.path().join("permissions.sqlite3");
		let store = tests::bound_agent_store(&path).await;
		let observed = facts(&store, Some(":read-only"), 'a').await;

		store.begin_agent_dispatch("work".into()).await.expect("dispatch");

		let selected = attempt(observed, 'a');

		assert!(
			store
				.reserve_agent_permission_selection(selected.clone())
				.await
				.expect("reserve")
				.is_none()
		);

		store.acknowledge_agent_dispatch("work".into(), "active".into()).await.expect("running");

		let id = store
			.reserve_agent_permission_selection(selected.clone())
			.await
			.expect("named profile")
			.expect("reserved");

		store
			.finish_agent_permission_selection(id, selected.clone(), "unknown".into())
			.await
			.expect("uncertain");

		drop(store);

		let reopened = SqliteStore::open_test(&path).expect("reopen");

		assert_eq!(
			reopened
				.get_agent_work_item("work".into())
				.await
				.expect("work")
				.active_turn_id
				.as_deref(),
			Some("active")
		);
		assert!(
			reopened.reserve_agent_permission_selection(selected).await.expect("replay").is_none()
		);

		facts(&reopened, Some("scoped"), 'c').await;

		assert_eq!(receipt(&reopened).await.state, "target_observed");
		assert_eq!(
			reopened.get_agent_work_item("work".into()).await.expect("work").dispatch_state,
			AgentDispatchState::Running
		);
	}

	#[tokio::test]
	async fn permission_reservation_survives_crash_and_blocks_dispatch_until_native_confirmation() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("state.sqlite3");
		let store = tests::bound_agent_store(&path).await;
		let observed = facts(&store, Some("readonly"), 'a').await;
		let a = attempt(observed, 'a');
		let b = attempt(observed, 'b');
		let (one, two) = tokio::join!(
			store.reserve_agent_permission_selection(a.clone()),
			store.reserve_agent_permission_selection(b.clone())
		);

		assert_ne!(one.as_ref().unwrap().is_some(), two.as_ref().unwrap().is_some());

		let (id, winning) =
			if let Some(id) = one.unwrap() { (id, a) } else { (two.unwrap().unwrap(), b) };

		assert!(store.begin_agent_dispatch("work".into()).await.is_err());
		assert!(store.list_pending_agent_events(100).await.unwrap().is_empty());

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert_eq!(receipt(&store).await.state, "reserved");
		assert!(
			store
				.reserve_agent_permission_selection(attempt(observed, 'c'))
				.await
				.unwrap()
				.is_none()
		);
		assert!(
			store
				.finish_agent_permission_selection(id, winning.clone(), "unknown".into())
				.await
				.unwrap()
		);
		assert!(
			!store.finish_agent_permission_selection(id, winning, "queued".into()).await.unwrap()
		);
		assert_eq!(receipt(&store).await.state, "unknown");

		facts(&store, None, 'b').await;

		assert_eq!(receipt(&store).await.state, "unknown");

		facts(&store, Some("other"), 'c').await;

		assert_eq!(receipt(&store).await.state, "unknown");
		assert!(store.begin_agent_dispatch("work".into()).await.is_err());

		store
			.record_agent_task_permissions(
				"thread".into(),
				None,
				Some(serde_json::json!({"profileId":"scoped"}).to_string()),
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
		assert!(store.begin_agent_dispatch("work".into()).await.is_ok());
		assert!(store.list_pending_agent_events(100).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn permission_publication_before_ack_wins_and_rejected_review_cannot_replay() {
		let dir = tempfile::tempdir().unwrap();
		let store = tests::bound_agent_store(&dir.path().join("state.sqlite3")).await;
		let first = facts(&store, Some("readonly"), 'a').await;
		let newer = facts(&store, Some("other"), 'b').await;

		assert!(
			store.reserve_agent_permission_selection(attempt(first, 'a')).await.unwrap().is_none()
		);

		let a = attempt(newer, 'a');
		let id = store.reserve_agent_permission_selection(a.clone()).await.unwrap().unwrap();
		let mut forged = a.clone();

		forged.profile = "different".into();

		assert!(
			!store.finish_agent_permission_selection(id, forged, "rejected".into()).await.unwrap()
		);

		facts(&store, Some("scoped"), 'c').await;

		assert!(
			store.finish_agent_permission_selection(id, a.clone(), "queued".into()).await.unwrap()
		);
		assert_eq!(receipt(&store).await.state, "target_observed");

		let reverted = facts(&store, Some("readonly"), 'd').await;
		let mut replay = a;

		replay.settings_event = reverted;
		replay.attempt_id = "new-client-key".into();

		assert!(store.reserve_agent_permission_selection(replay).await.unwrap().is_none());

		let next = attempt(reverted, 'b');
		let next_id =
			store.reserve_agent_permission_selection(next.clone()).await.unwrap().unwrap();

		assert!(
			store
				.finish_agent_permission_selection(next_id, next.clone(), "rejected".into())
				.await
				.unwrap()
		);

		facts(&store, Some("scoped"), 'e').await;

		assert_eq!(receipt(&store).await.state, "rejected");
		assert!(store.reserve_agent_permission_selection(next).await.unwrap().is_none());
		assert!(store.begin_agent_dispatch("work".into()).await.is_ok());
	}
	#[tokio::test]
	async fn observation_revisions_preserve_reversions_without_consuming_transcript_pages() {
		let dir = tempfile::tempdir().unwrap();
		let store = tests::bound_agent_store(&dir.path().join("observations.sqlite3")).await;
		let message = store
			.record_agent_observation(EnqueueAgentEvent {
				source_event_id: "answer".into(),
				work_item_id: "work".into(),
				event_kind: "assistant_message".into(),
				payload: serde_json::json!({"text":"Visible answer"}).to_string(),
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
				.agent_task_permissions("work".into(), "thread".into(), None)
				.await
				.unwrap()
				.unwrap()
				.id,
			reverted
		);
		assert!(
			store
				.agent_task_permissions("foreign".into(), "thread".into(), None)
				.await
				.unwrap()
				.is_none()
		);

		let attempt = attempt(reverted, 'a');
		let reserved =
			store.reserve_agent_permission_selection(attempt.clone()).await.unwrap().unwrap();

		store.finish_agent_permission_selection(reserved, attempt, "queued".into()).await.unwrap();

		facts(&store, Some("scoped"), 'b').await;

		assert_eq!(
			store.read_agent_work_events("work".into(), 1).await.unwrap(),
			vec![message.clone()]
		);
		assert_eq!(
			store.read_agent_transcript("work".into(), None, 1).await.unwrap().0,
			vec![message]
		);
	}
}
