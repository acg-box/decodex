//! Source-owned response observations. They never create an execution obligation.

use crate::{SqliteStore, StoreError, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde_json::Value;

/// Bounded display fields; opaque provider metadata stays out of timeline reads.
pub struct ChiefResponseUsageSummary {
	pub turn_id: String,
	pub response_id: String,
	pub amount: Option<String>,
	pub metadata_omitted: bool,
	pub observed_count: i64,
}

impl SqliteStore {
	/// Read at most eight response amounts per exact turn, with the full observed count.
	pub async fn read_chief_response_usage(
		&self,
		work: String,
		thread: String,
		turns: Vec<String>,
	) -> Result<Vec<ChiefResponseUsageSummary>, StoreError> {
		if work.is_empty()
			|| work.len() > 512
			|| thread.is_empty()
			|| thread.len() > 512
			|| turns.len() > 100
			|| turns.iter().any(|turn| turn.is_empty() || turn.len() > 512)
		{
			return Err(StoreError::InvalidInput("invalid response usage query"));
		}
		let turns =
			serde_json::to_string(&turns).map_err(|_| StoreError::InvalidInput("invalid turns"))?;
		self.run(move |connection| {
			let mut query = connection.prepare("WITH observations AS (
			SELECT id,json_extract(payload,'$.turnId') turn_id,json_extract(payload,'$.responseId') response_id,
			json_extract(payload,'$.usageMetadata.amount') amount,coalesce(json_extract(payload,'$.usageMetadata.metadataOmitted'),0) omitted
			FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind='response_usage'
			AND json_extract(payload,'$.threadId')=?2 AND json_extract(payload,'$.turnId') IN (SELECT value FROM json_each(?3))
			AND json_extract(source_event_id,'$[2]') IS (
			WITH RECURSIVE ancestry(id,parent,depth) AS (
			SELECT id,parent_goal_id,0 FROM chief_work_items WHERE id=?1
			UNION ALL SELECT w.id,w.parent_goal_id,a.depth+1 FROM chief_work_items w JOIN ancestry a ON w.id=a.parent)
			SELECT b.account_id FROM ancestry a JOIN chief_process_bindings b ON b.root_id=a.id ORDER BY a.depth,b.created_at_micros DESC,b.rowid DESC LIMIT 1)),
			ranked AS (SELECT *,row_number() OVER(PARTITION BY turn_id ORDER BY id DESC) rank,count(*) OVER(PARTITION BY turn_id) count FROM observations)
			SELECT turn_id,response_id,amount,omitted,count FROM ranked WHERE rank<=8 ORDER BY id")
			.map_err(sqlite_error)?;
			query.query_map(params![work,thread,turns],|row|Ok(ChiefResponseUsageSummary {
				turn_id:row.get(0)?,response_id:row.get(1)?,amount:row.get(2)?,metadata_omitted:row.get(3)?,observed_count:row.get(4)?,
			})).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(|error|sqlite_error(error).into())
		}).await
	}

	/// Record a bounded native response for the currently owned thread, including late completions.
	pub async fn record_chief_response_usage(
		&self,
		generation: Option<String>,
		payload: String,
	) -> Result<bool, StoreError> {
		if payload.len() > 48 * 1024 {
			return Err(StoreError::InvalidInput("response usage exceeds storage budget"));
		}
		let value: Value = serde_json::from_str(&payload)
			.map_err(|_| StoreError::InvalidInput("invalid response usage"))?;
		let identity = |key| {
			value
				.get(key)
				.and_then(Value::as_str)
				.filter(|id| !id.is_empty() && id.len() <= 512)
				.map(str::to_owned)
				.ok_or(StoreError::InvalidInput("invalid response identity"))
		};
		let thread = identity("threadId")?;
		let turn = identity("turnId")?;
		let response = identity("responseId")?;
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work: Option<String> = tx.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1",[&thread],|row|row.get(0)).optional().map_err(sqlite_error)?;
			let Some(work) = work else { return Ok(false); };
			if !crate::chief_process::owns_work(&tx,&work,generation.as_deref())? { return Ok(false); }
			let account: Option<String> = if let Some(generation) = generation.as_ref() {
				tx.query_row("SELECT account_id FROM chief_process_bindings WHERE generation_id=?1",[generation],|row|row.get(0)).optional().map_err(sqlite_error)?
			} else { None };
			let identity = serde_json::json!(["response_usage",work,account,thread,turn,response]).to_string();
			let previous: Option<String> = tx.query_row("SELECT payload FROM chief_inbox_events WHERE source_event_id=?1",[&identity],|row|row.get(0)).optional().map_err(sqlite_error)?;
			if let Some(previous) = previous {
				return if previous == payload { Ok(false) } else { Err(StoreError::IdempotencyConflict) };
			}
			let now = unix_micros()?;
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'response_usage',?3,?4,'resolved','Provider response usage observed.',?4)",params![identity,work,payload,now]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(true)
		}).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus};

	#[tokio::test]
	async fn response_observations_survive_restart_without_waking_or_exposing_metadata() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("usage.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		store
			.create_chief_work_item(ChiefWorkItem {
				id: "root".into(),
				parent_goal_id: None,
				kind: ChiefWorkKind::Goal,
				title: "Chief".into(),
				instructions: "Task".into(),
				codex_thread_id: None,
				dispatch_state: ChiefDispatchState::Idle,
				active_turn_id: None,
				status: ChiefWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		store.begin_chief_thread_creation("root".into()).await.unwrap();
		store.acknowledge_chief_thread_creation("root".into(), "thread".into()).await.unwrap();
		let payload = serde_json::json!({"threadId":"thread","turnId":"past-turn","responseId":"response","usageMetadata":{"amount":"0.12345678901234567890","metadata":{"private":"retained"}}}).to_string();
		assert!(store.record_chief_response_usage(None, payload.clone()).await.unwrap());
		assert!(!store.record_chief_response_usage(None, payload.clone()).await.unwrap());
		assert!(
			!store
				.record_chief_response_usage(Some("stale-generation".into()), payload.clone())
				.await
				.unwrap()
		);
		assert!(store.read_chief_work_events("root".into(), 10).await.unwrap().is_empty());
		assert!(store.list_pending_chief_events(10).await.unwrap().is_empty());
		assert!(store.list_chief_wake_events("root".into(), 10).await.unwrap().is_empty());
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert!(!store.record_chief_response_usage(None, payload.clone()).await.unwrap());
		let expected = payload.clone();
		let saved: String = store
			.run(move |connection| {
				connection
					.query_row(
						"SELECT payload FROM chief_inbox_events WHERE event_kind='response_usage'",
						[],
						|row| row.get(0),
					)
					.map_err(|error| sqlite_error(error).into())
			})
			.await
			.unwrap();
		assert_eq!(saved, expected);
		let rows = store
			.read_chief_response_usage("root".into(), "thread".into(), vec!["past-turn".into()])
			.await
			.unwrap();
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].amount.as_deref(), Some("0.12345678901234567890"));
		assert_eq!(rows[0].response_id, "response");
		assert!(
			store
				.read_chief_response_usage(
					"other-work".into(),
					"thread".into(),
					vec!["past-turn".into()]
				)
				.await
				.unwrap()
				.is_empty()
		);
		for index in 0..10 {
			let mut event: Value = serde_json::from_str(&payload).unwrap();
			event["responseId"] = serde_json::json!(format!("response-{index}"));
			store.record_chief_response_usage(None, event.to_string()).await.unwrap();
		}
		let rows = store
			.read_chief_response_usage("root".into(), "thread".into(), vec!["past-turn".into()])
			.await
			.unwrap();
		assert_eq!(rows.len(), 8);
		assert!(rows.iter().all(|row| row.observed_count == 11));
		let mut changed: Value = serde_json::from_str(&payload).unwrap();
		changed["usageMetadata"]["amount"] = serde_json::json!("1");
		assert!(matches!(
			store.record_chief_response_usage(None, changed.to_string()).await,
			Err(StoreError::IdempotencyConflict)
		));
		changed["threadId"] = serde_json::json!("foreign");
		assert!(!store.record_chief_response_usage(None, changed.to_string()).await.unwrap());
	}
}
