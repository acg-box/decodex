//! One row per independent work item, with worker progress folded into its owner.

use crate::{SqliteStore, StoreError, error::sqlite_error};

// One entry per conversation with real input or progress. Native archive visibility
// supplies the same active work set used by the rest of the application. Child
// workers contribute state to their owner, never another feed entry. Tool calls,
// usage accounting and readback refreshes do not advance the timeline.
const DELIVERIES: &str = r#"WITH RECURSIVE owners AS (
 SELECT w.rowid AS record_id,w.* FROM agent_work_items w
 WHERE w.kind='goal'
 AND (?5 IS NULL OR w.id IN (SELECT value FROM json_each(?5)))
 AND EXISTS(SELECT 1 FROM agent_inbox_events e WHERE e.work_item_id=w.id
            AND e.event_kind IN ('user_message','work_instruction','agent_turn_completed','worker_turn_completed','plan_updated'))
), owned(owner,id) AS (
 SELECT id,id FROM owners UNION SELECT o.owner,w.id FROM owned o
 JOIN agent_work_items w ON w.parent_goal_id=o.id
 WHERE NOT EXISTS(SELECT 1 FROM owners g WHERE g.id=w.id)
 AND (?5 IS NULL OR w.id IN (SELECT value FROM json_each(?5)))
), progress AS (
 SELECT o.owner,max(w.dispatch_state IN ('running','dispatching')) AS running,
 max(w.dispatch_state='unknown') AS uncertain,
 max(o.id=o.owner AND EXISTS(SELECT 1 FROM agent_inbox_events e WHERE e.work_item_id=o.owner
     AND e.event_kind='async_question_answer' AND e.delivered_turn_id=w.active_turn_id)) AS resuming
 FROM owned o JOIN agent_work_items w ON w.id=o.id GROUP BY o.owner
), questions AS (
 SELECT o.owner,q.work_id,q.question_json,q.created_at_micros FROM owned o
 JOIN agent_async_questions q ON q.work_id=o.id
 JOIN agent_work_items w ON w.id=q.work_id AND w.codex_thread_id=q.thread_id
 WHERE NOT EXISTS(SELECT 1 FROM agent_async_recovery r WHERE r.work_id=q.work_id AND r.thread_id=q.thread_id)
 AND NOT EXISTS(SELECT 1 FROM agent_async_answers a WHERE a.work_id=q.work_id AND a.thread_id=q.thread_id AND a.question_id IN(q.question_id,q.item_id))
 AND NOT EXISTS(SELECT 1 FROM agent_async_skips s WHERE s.work_id=q.work_id AND s.thread_id=q.thread_id AND s.question_id=q.question_id)
), events AS (
 SELECT o.owner,e.* FROM owned o JOIN agent_inbox_events e ON e.work_item_id=o.id
 WHERE e.event_kind IN ('user_message','work_instruction','plan_updated','agent_turn_completed','worker_turn_completed','goal_resolved',
 'user_input_pending','permission_pending','server_request_pending','thread_in_use_needs_attention','user_decision_resolved','async_question_answer','voice_user','voice_assistant')
), deliveries AS (
 SELECT w.record_id,
 coalesce((SELECT q.work_id FROM questions q WHERE q.owner=w.id ORDER BY q.created_at_micros LIMIT 1),
          (SELECT e.work_item_id FROM events e WHERE e.owner=w.id AND e.disposition IS NULL AND e.event_kind IN ('user_input_pending','permission_pending','server_request_pending','thread_in_use_needs_attention') ORDER BY e.id LIMIT 1),
          w.id) AS id,
 w.id AS owner_id,
 coalesce((SELECT json_extract(input.payload,'$.text') FROM events input
  WHERE input.work_item_id=w.id AND input.event_kind IN('user_message','work_instruction')
  AND input.delivered_turn_id IS NOT NULL AND coalesce(json_extract(input.payload,'$.asyncQuestionReply'),0)<>1
  ORDER BY input.id DESC LIMIT 1),w.title) AS title,
 w.status,coalesce(s.slot,ps.slot) AS slot,
 max(w.created_at_micros,
     coalesce((SELECT max(max(e.created_at_micros,coalesce(e.disposed_at_micros,0))) FROM events e WHERE e.owner=w.id),0),
     coalesce((SELECT max(q.created_at_micros) FROM questions q WHERE q.owner=w.id),0)) AS changed,
 a.running,a.uncertain,coalesce(a.resuming,0) AS resuming,
 (EXISTS(SELECT 1 FROM questions q WHERE q.owner=w.id)
  OR EXISTS(SELECT 1 FROM events e WHERE e.owner=w.id AND e.disposition IS NULL AND e.event_kind IN ('user_input_pending','permission_pending','server_request_pending'))) AS needs_input,
 EXISTS(SELECT 1 FROM events e WHERE e.owner=w.id AND e.disposition IS NULL AND e.event_kind='thread_in_use_needs_attention') AS blocked,
 EXISTS(SELECT 1 FROM agent_inbox_events e JOIN owned o ON o.id=e.work_item_id WHERE o.owner=w.id AND e.disposition IS NULL AND e.delivered_turn_id IS NULL AND e.event_kind IN ('user_message','work_instruction')) AS queued,
 (SELECT e.payload FROM events e WHERE e.work_item_id=w.id AND e.event_kind IN ('agent_turn_completed','worker_turn_completed') ORDER BY e.id DESC LIMIT 1) AS payload,
 (SELECT e.payload FROM events e WHERE e.owner=w.id AND e.event_kind='plan_updated' AND json_type(e.payload,'$.progress')='object'
  AND e.id>=coalesce((SELECT max(input.id) FROM events input WHERE input.work_item_id=w.id AND input.event_kind IN ('user_message','work_instruction')),0)
  ORDER BY e.id DESC LIMIT 1) AS milestone,
 (SELECT q.question_json FROM questions q WHERE q.owner=w.id ORDER BY q.created_at_micros LIMIT 1) AS decision,
 (SELECT json_extract(e.payload,'$.summary') FROM events e WHERE e.work_item_id=w.id AND e.event_kind='goal_resolved' ORDER BY e.id DESC LIMIT 1) AS resolution,
 (SELECT json_group_array(json_object('id',recent.id,'kind',recent.event_kind,'payload',json(recent.payload),'at',recent.created_at_micros))
  FROM (SELECT e.id,e.event_kind,e.payload,e.created_at_micros FROM events e
   WHERE e.owner=w.id AND (e.event_kind IN ('plan_updated','async_question_answer','goal_resolved','agent_turn_completed','worker_turn_completed')
    OR (e.event_kind='user_message' AND json_extract(e.payload,'$.asyncQuestionReply')=1))
   AND NOT(e.event_kind IN ('agent_turn_completed','worker_turn_completed') AND EXISTS(
    SELECT 1 FROM agent_async_questions q WHERE q.work_id=e.work_item_id AND q.turn_id=json_extract(e.payload,'$.terminal.turn.id')
    AND NOT EXISTS(SELECT 1 FROM events reply WHERE reply.work_item_id=q.work_id
     AND reply.event_kind IN('user_message','async_question_answer')
     AND reply.delivered_turn_id=q.turn_id AND json_extract(reply.payload,'$.asyncQuestionId') IN(q.question_id,q.item_id))
    AND NOT EXISTS(SELECT 1 FROM agent_async_skips s WHERE s.work_id=q.work_id AND s.thread_id=q.thread_id
     AND s.question_id=q.question_id AND s.created_at_micros<=e.created_at_micros)))
   ORDER BY e.id DESC LIMIT 6) recent) AS updates
 FROM owners w JOIN progress a ON a.owner=w.id
 LEFT JOIN agent_slots s ON s.agent_key=w.id
 LEFT JOIN agent_slots ps ON ps.agent_key=w.parent_goal_id
 WHERE (?1 IS NULL OR w.parent_goal_id=?1 OR EXISTS(SELECT 1 FROM owned o WHERE o.owner=w.id AND o.id=?1))
)"#;

pub struct DeliveryRecord {
	pub id: i64,
	pub work_id: String,
	pub title: String,
	pub slot: Option<u32>,
	pub updated_at_micros: i64,
	pub status: String,
	pub running: bool,
	pub resuming: bool,
	pub needs_input: bool,
	pub blocked: bool,
	pub queued: bool,
	pub uncertain: bool,
	pub payload: Option<String>,
	pub milestone: Option<String>,
	pub decision: Option<String>,
	pub resolution: Option<String>,
	pub updates: String,
}

pub struct DeliveryPage {
	pub entries: Vec<DeliveryRecord>,
	pub extent: Option<(i64, i64)>,
}

impl SqliteStore {
	pub async fn agent_deliveries(
		&self,
		visible: Option<Vec<String>>,
		agent: Option<String>,
		after: Option<i64>,
		before: Option<i64>,
		limit: u16,
	) -> Result<DeliveryPage, StoreError> {
		if limit == 0 || limit > 100 || after.zip(before).is_some_and(|(a, b)| a >= b) {
			return Err(StoreError::InvalidInput("invalid delivery query"));
		}

		let visible = visible.map(|ids| serde_json::to_string(&ids).expect("work IDs serialize"));

		self.run(move |c| {
			let tx = c.transaction().map_err(sqlite_error)?;
			let extent = tx
				.query_row(
					&format!("{DELIVERIES} SELECT min(changed),max(changed) FROM deliveries"),
					rusqlite::params![agent, None::<i64>, None::<i64>, limit, visible],
					|r| Ok(r.get::<_, Option<i64>>(0)?.zip(r.get::<_, Option<i64>>(1)?)),
				)
				.map_err(sqlite_error)?;
			let entries = tx
				.prepare(&format!(
					"{DELIVERIES} SELECT * FROM deliveries
                WHERE (?2 IS NULL OR changed>=?2) AND (?3 IS NULL OR changed<?3)
                ORDER BY changed DESC,record_id DESC LIMIT ?4"
				))
				.map_err(sqlite_error)?
				.query_map(rusqlite::params![agent, after, before, limit, visible], |r| {
					Ok(DeliveryRecord {
						id: r.get("record_id")?,
						work_id: r.get("id")?,
						title: r.get("title")?,
						status: r.get("status")?,
						slot: r.get("slot")?,
						updated_at_micros: r.get("changed")?,
						running: r.get("running")?,
						resuming: r.get("resuming")?,
						needs_input: r.get("needs_input")?,
						blocked: r.get("blocked")?,
						queued: r.get("queued")?,
						uncertain: r.get("uncertain")?,
						payload: r.get("payload")?,
						milestone: r.get("milestone")?,
						decision: r.get("decision")?,
						resolution: r.get("resolution")?,
						updates: r.get("updates")?,
					})
				})
				.map_err(sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(sqlite_error)?;

			tx.commit().map_err(sqlite_error)?;

			Ok(DeliveryPage { entries, extent })
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use crate::agent_deliveries::{SqliteStore, sqlite_error};
	#[tokio::test]
	async fn conversations_include_plain_replies_fold_workers_and_ignore_tool_churn() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("conversations.sqlite");
		let store = SqliteStore::open_test(&path).unwrap();

		store.with_connection(|c| {
   c.execute("INSERT INTO agent_work_items(id,kind,title,instructions,status,created_at_micros,updated_at_micros) VALUES('main','goal','Main','Coordinate','open',1,9999)",[]).map_err(sqlite_error)?;

   for id in ["chat","task","archived"] {
    c.execute("INSERT INTO agent_work_items(id,parent_goal_id,kind,title,instructions,status,codex_thread_id,created_at_micros,updated_at_micros) VALUES(?1,'main','goal',?1,'Work','open',?1,10,9999)",[id]).map_err(sqlite_error)?;
   }

   c.execute("INSERT INTO agent_work_items(id,parent_goal_id,kind,title,instructions,status,dispatch_state,active_turn_id,codex_thread_id,created_at_micros,updated_at_micros) VALUES('worker','task','task','Review','Check','open','running','turn','worker-thread',10,9999)",[]).map_err(sqlite_error)?;

   for (id,work,kind,at) in [("reply","chat","agent_turn_completed",100),("tool","chat","activity_completed",900),("plan","task","plan_updated",200),("old","archived","agent_turn_completed",800)] {
    c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,?2,?3,'{}',?4)",rusqlite::params![id,work,kind,at]).map_err(sqlite_error)?;
   }

   c.execute("INSERT INTO agent_async_questions(work_id,thread_id,turn_id,item_id,question_id,question_json,created_at_micros) VALUES('worker','worker-thread','turn','item','question','{}',400)",[]).map_err(sqlite_error)?;

   Ok(())
  }).unwrap();

		let visible = Some(vec!["chat".into(), "task".into(), "worker".into()]);
		let page = store.agent_deliveries(visible.clone(), None, None, None, 20).await.unwrap();

		assert_eq!(page.entries.len(), 2);
		assert_eq!(page.extent, Some((100, 400)));
		assert_eq!(page.entries[0].title, "task");
		assert_eq!(page.entries[0].work_id, "worker");
		assert!(page.entries[0].running && page.entries[0].needs_input);
		assert_eq!(page.entries[1].work_id, "chat");

		let changes: serde_json::Value = serde_json::from_str(&page.entries[1].updates).unwrap();

		assert_eq!(changes[0]["kind"], "agent_turn_completed");
		assert_eq!(changes[0]["at"], 100, "tool activity must not become a work update");
		assert_eq!(
			store
				.agent_deliveries(visible.clone(), Some("worker".into()), None, None, 20)
				.await
				.unwrap()
				.entries
				.len(),
			1
		);
		assert_eq!(
			store
				.agent_deliveries(visible.clone(), None, Some(401), None, 20)
				.await
				.unwrap()
				.extent,
			Some((100, 400))
		);
		assert!(
			store
				.agent_deliveries(Some(vec![]), None, None, None, 20)
				.await
				.unwrap()
				.entries
				.is_empty()
		);

		store.with_connection(|c| {
			c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES('writer-lock','chat','thread_in_use_needs_attention','{}',450)", []).map_err(sqlite_error)?;

			Ok(())
		}).unwrap();

		let page = store.agent_deliveries(visible.clone(), None, None, None, 20).await.unwrap();

		assert!(page.entries.iter().find(|r| r.work_id == "chat").unwrap().blocked);

		store.with_connection(|c| {
			c.execute("UPDATE agent_inbox_events SET disposition='resolved',disposition_note='Writer released',disposed_at_micros=500 WHERE source_event_id='writer-lock'", []).map_err(sqlite_error)?;

			Ok(())
		}).unwrap();

		let page = store.agent_deliveries(visible.clone(), None, None, None, 20).await.unwrap();

		assert!(!page.entries.iter().find(|r| r.work_id == "chat").unwrap().blocked);

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert_eq!(
			store.agent_deliveries(visible, None, None, None, 20).await.unwrap().entries.len(),
			2
		);
	}
	#[tokio::test]
	async fn later_turns_update_the_same_conversation_without_a_plan() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("turns.sqlite")).unwrap();
		let request = "Help me make a quiet reading plan, with every part of this question retained beyond the old title limit.";

		store.with_connection(|c| {
   c.execute("INSERT INTO agent_work_items(id,kind,title,instructions,status,codex_thread_id,created_at_micros,updated_at_micros) VALUES('chat','goal','Chat','Talk','open','thread',1,9999)",[]).map_err(sqlite_error)?;
   c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,delivery_work_item_id,delivered_turn_id) VALUES('input','chat','user_message',?1,2,'chat','first')",[serde_json::json!({"text":request}).to_string()]).map_err(sqlite_error)?;
   c.execute("INSERT INTO agent_async_questions(work_id,thread_id,turn_id,item_id,question_id,question_json,created_at_micros) VALUES('chat','thread','first','item','q','{}',3)",[]).map_err(sqlite_error)?;

   Ok(())
  }).unwrap();

		assert_eq!(
			store.agent_deliveries(None, None, None, None, 20).await.unwrap().entries.len(),
			1
		);

		for (id, at) in [("first", 3), ("second", 9)] {
			store.with_connection(|c| {
    let payload=serde_json::json!({"terminal":{"turn":{"id":id,"status":"completed"}}}).to_string();

    c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,'chat','agent_turn_completed',?2,?3)",rusqlite::params![id,payload,at]).map_err(sqlite_error)?;

    Ok(())
   }).unwrap();

			let page = store.agent_deliveries(None, None, None, None, 20).await.unwrap();

			assert_eq!(page.entries.len(), 1);
			assert_eq!(page.entries[0].updated_at_micros, at);
			assert!(page.entries[0].payload.as_ref().unwrap().contains(id));

			let changes: Vec<serde_json::Value> =
				serde_json::from_str(&page.entries[0].updates).unwrap();

			if id == "first" {
				assert!(changes.is_empty(), "a question turn is not another delivered result");
			} else {
				assert_eq!(changes[0]["at"], at, "changes must follow the latest native turn");
			}
		}

		store.with_connection(|c| {
			c.execute("INSERT INTO agent_async_questions(work_id,thread_id,turn_id,item_id,question_id,question_json,created_at_micros) VALUES('chat','thread','native','native-item','native-question','{}',11)",[]).map_err(sqlite_error)?;
			c.execute("INSERT INTO agent_async_answers(work_id,thread_id,question_id,created_at_micros) VALUES('chat','thread','native-question',999)",[]).map_err(sqlite_error)?;

			for (id,kind,payload,at) in [
				("native-answer","user_message",serde_json::json!({"text":"Reading","asyncQuestionId":"native-question","asyncQuestionReply":true}),12),
				("native-result","agent_turn_completed",serde_json::json!({"terminal":{"turn":{"id":"native","status":"completed"}}}),13),
			] {
				c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,'chat',?2,?3,?4,'chat','native')",rusqlite::params![id,kind,payload.to_string(),at]).map_err(sqlite_error)?;
			}

			Ok(())
		}).unwrap();

		let page = store.agent_deliveries(None, None, None, None, 20).await.unwrap();

		assert_eq!(
			page.entries[0].title, request,
			"native question replies must not replace the delivered request"
		);

		let changes: Vec<serde_json::Value> =
			serde_json::from_str(&page.entries[0].updates).unwrap();

		assert_eq!(changes[0]["at"], 13, "a same-turn answer must retain the actual final result");
		assert_eq!(
			changes[1]["kind"], "user_message",
			"native question answers are work decisions"
		);
		assert!(store.agent_deliveries(None, None, Some(1), Some(1), 20).await.is_err());
	}
}
