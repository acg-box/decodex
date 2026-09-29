//! Read and reconcile historical plugin selections after local plugin writes were retired.
use crate::{SqliteStore, StoreError, agent_process::owns_work, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentPluginAttempt {
	pub work: String,
	pub thread: String,
	pub generation: Option<String>,
	pub settings_event: i64,
	pub disabled_plugin_ids: Vec<String>,
	pub review_token: String,
	pub attempt_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPluginReceipt {
	pub id: i64,
	pub attempt: AgentPluginAttempt,
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

pub(crate) fn pending(connection: &rusqlite::Connection, work: &str) -> Result<bool, StoreError> {
	connection.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events e WHERE e.work_item_id=?1 AND e.event_kind='plugin_selection' AND NOT EXISTS(SELECT 1 FROM agent_inbox_events r WHERE (r.source_event_id=e.source_event_id||':result' AND r.event_kind='plugin_selection_result' AND json_extract(r.payload,'$.state')='rejected') OR (r.source_event_id=e.source_event_id||':observation' AND r.event_kind='plugin_selection_observation')))", [work], |r|r.get(0)).map_err(|e|sqlite_error(e).into())
}

impl SqliteStore {
	pub async fn agent_plugin_receipt(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<AgentPluginReceipt>, StoreError> {
		self.run(move |connection| {
			let row:Option<(i64,String,String)>=connection.query_row("SELECT e.id,json_extract(e.payload,'$.attempt'),COALESCE(json_extract(o.payload,'$.state'),json_extract(r.payload,'$.state'),'reserved') FROM agent_inbox_events e LEFT JOIN agent_inbox_events r ON r.source_event_id=e.source_event_id||':result' AND r.event_kind='plugin_selection_result' LEFT JOIN agent_inbox_events o ON o.source_event_id=e.source_event_id||':observation' AND o.event_kind='plugin_selection_observation' WHERE e.work_item_id=?1 AND e.event_kind='plugin_selection' AND json_extract(e.payload,'$.attempt.thread')=?2 ORDER BY e.id DESC LIMIT 1",params![work,thread],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(sqlite_error)?;
			row.map(|(id,attempt,state)|Ok(AgentPluginReceipt {id,attempt:serde_json::from_str(&attempt).map_err(|_|StoreError::InvalidInput("invalid saved plugin attempt"))?,state})).transpose()
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
	let row:Option<(i64,String,Option<String>,String)>=connection.query_row("SELECT e.id,e.source_event_id,json_extract(e.payload,'$.attempt.generation'),json_extract(e.payload,'$.attempt.disabled_plugin_ids') FROM agent_inbox_events e JOIN agent_work_items w ON w.id=e.work_item_id AND w.codex_thread_id=?2 WHERE e.work_item_id=?1 AND e.event_kind='plugin_selection' AND e.id<?3 AND json_extract(e.payload,'$.attempt.thread')=?2 AND NOT EXISTS(SELECT 1 FROM agent_inbox_events r WHERE (r.source_event_id=e.source_event_id||':result' AND json_extract(r.payload,'$.state')='rejected') OR r.source_event_id=e.source_event_id||':observation') ORDER BY e.id DESC LIMIT 1",params![work,thread,observation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sqlite_error)?;
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
	connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'plugin_selection_observation',?3,?4,'resolved','Current native plugins observed; prior request causation is not asserted.',?4)",params![format!("{key}:observation"),work,json!({"reservation":reservation,"settingsEvent":observation,"state":state,"generationId":generation}).to_string(),now]).map_err(sqlite_error)?;
	Ok(())
}

#[cfg(test)]
pub(crate) async fn seed_legacy_selection(
	store: &SqliteStore,
	attempt: AgentPluginAttempt,
	state: Option<&str>,
) {
	let state = state.map(str::to_owned);
	store.run(move |connection| {
		// Historical journal rows, not a replacement implementation of the retired writer.
		let key = format!("plugin-selection:{}", attempt.review_token);
		let payload = json!({"attempt":{"work":attempt.work,"thread":attempt.thread,"generation":attempt.generation,"settings_event":attempt.settings_event,"disabled_plugin_ids":attempt.disabled_plugin_ids,"review_token":attempt.review_token,"attempt_id":attempt.attempt_id}});
		connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'plugin_selection',?3,1,'resolved','Historical fixture',1)", params![key,attempt.work,payload.to_string()]).map_err(sqlite_error)?;
		let id=connection.last_insert_rowid();
		if let Some(state)=state {
			connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'plugin_selection_result',?3,2,'resolved','Historical fixture',2)",params![format!("{key}:result"),attempt.work,json!({"reservation":id,"state":state}).to_string()]).map_err(sqlite_error)?;
		}
		Ok(())
	}).await.expect("load historical plugin selection");
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::tests::bound_agent_store as setup;

	async fn facts(store: &SqliteStore, profile: Option<&str>, digest: char) -> i64 {
		store
			.record_agent_task_plugins_publication(
				"thread".into(),
				None,
				profile.map(|profile| json!({"disabledPluginIds":[profile]}).to_string()),
				digest.to_string().repeat(64),
			)
			.await
			.unwrap()
			.unwrap()
	}
	fn attempt(settings_event: i64) -> AgentPluginAttempt {
		AgentPluginAttempt {
			work: "work".into(),
			thread: "thread".into(),
			generation: None,
			settings_event,
			disabled_plugin_ids: vec!["scoped".into()],
			review_token: "a".repeat(64),
			attempt_id: "legacy".into(),
		}
	}
	async fn receipt(store: &SqliteStore) -> AgentPluginReceipt {
		store.agent_plugin_receipt("work".into(), "thread".into()).await.unwrap().unwrap()
	}

	#[tokio::test]
	async fn historical_pending_plugin_edits_block_dispatch_until_current_confirmation() {
		for state in [None, Some("queued"), Some("unknown")] {
			let dir = tempfile::tempdir().unwrap();
			let path = dir.path().join("state.sqlite3");
			let store = setup(&path).await;
			let observed = facts(&store, Some("readonly"), 'a').await;
			let original = attempt(observed);
			seed_legacy_selection(&store, original.clone(), state).await;
			drop(store);
			let store = SqliteStore::open_test(&path).unwrap();
			assert_eq!(receipt(&store).await.attempt, original);
			assert_eq!(receipt(&store).await.state, state.unwrap_or("reserved"));
			assert!(store.begin_agent_dispatch("work".into()).await.is_err());
			facts(&store, None, 'b').await;
			facts(&store, Some("other"), 'c').await;
			store
				.record_agent_task_plugins(
					"thread".into(),
					None,
					Some(json!({"disabledPluginIds":["scoped"]}).to_string()),
					"d".repeat(64),
				)
				.await
				.unwrap();
			assert_eq!(
				receipt(&store).await.state,
				state.unwrap_or("reserved"),
				"historical or unmatched facts cannot settle a write"
			);
			facts(&store, Some("scoped"), 'e').await;
			assert_eq!(receipt(&store).await.state, "target_observed");
			assert!(store.begin_agent_dispatch("work".into()).await.is_ok());
			assert!(store.list_pending_agent_events(100).await.unwrap().is_empty());
		}
	}

	#[tokio::test]
	async fn historical_rejection_is_not_replaced_by_later_plugin_observation() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("state.sqlite3");
		let store = setup(&path).await;
		let observed = facts(&store, Some("readonly"), 'a').await;
		seed_legacy_selection(&store, attempt(observed), Some("rejected")).await;
		facts(&store, Some("scoped"), 'b').await;
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert_eq!(receipt(&store).await.state, "rejected");
		assert!(store.begin_agent_dispatch("work".into()).await.is_ok());
	}

	#[tokio::test]
	async fn historical_plugin_edit_blocks_running_permission_selection() {
		let dir = tempfile::tempdir().unwrap();
		let store = setup(&dir.path().join("mixed.sqlite3")).await;
		let observed = facts(&store, Some("original"), 'a').await;
		let event=store.record_agent_task_permissions_publication("thread".into(),None,Some(json!({"profileId":":read-only","cwd":"/native","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"}}).to_string()),"a".repeat(64)).await.unwrap().unwrap();
		store.begin_agent_dispatch("work".into()).await.unwrap();
		store.acknowledge_agent_dispatch("work".into(), "active".into()).await.unwrap();
		seed_legacy_selection(&store, attempt(observed), Some("unknown")).await;
		let permission = crate::AgentPermissionAttempt {
			work: "work".into(),
			thread: "thread".into(),
			generation: None,
			settings_event: event,
			profile: "scoped".into(),
			review_token: "b".repeat(64),
			attempt_id: "permission".into(),
		};
		assert!(
			store.reserve_agent_permission_selection(permission.clone()).await.unwrap().is_none()
		);
		facts(&store, Some("scoped"), 'c').await;
		assert!(store.reserve_agent_permission_selection(permission).await.unwrap().is_some());
		assert_eq!(
			store.get_agent_work_item("work".into()).await.unwrap().active_turn_id.as_deref(),
			Some("active")
		);
	}

	#[tokio::test]
	async fn observation_revisions_preserve_reversions_without_consuming_transcript_pages() {
		let dir = tempfile::tempdir().unwrap();
		let store = setup(&dir.path().join("observations.sqlite3")).await;
		let message = store
			.record_agent_observation(crate::EnqueueAgentEvent {
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
				.agent_task_plugins("work".into(), "thread".into(), None)
				.await
				.unwrap()
				.unwrap()
				.id,
			reverted
		);
		assert!(
			store
				.agent_task_plugins("foreign".into(), "thread".into(), None)
				.await
				.unwrap()
				.is_none()
		);
		seed_legacy_selection(&store, attempt(reverted), Some("queued")).await;
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
