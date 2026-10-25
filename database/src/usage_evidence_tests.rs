use serde_json::Value;
use tempfile::TempDir;

use crate::{
	AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus, EnqueueAgentEvent,
	SqliteStore, error,
};
use decodex_core::work::{Work, WorkCommand, WorkRequirements, WorkState};

async fn fixture() -> (TempDir, SqliteStore) {
	let home = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&home.path().join("usage.db")).unwrap();

	store
		.create_agent_work_item(AgentWorkItem {
			id: "sage".into(),
			parent_goal_id: None,
			kind: AgentWorkKind::Goal,
			title: "Sage".into(),
			instructions: "Help".into(),
			codex_thread_id: None,
			dispatch_state: AgentDispatchState::Idle,
			active_turn_id: None,
			status: AgentWorkStatus::Open,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
		})
		.await
		.unwrap();

	for id in ["one", "two"] {
		store
			.apply_work_command(
				"sage".into(),
				format!("create-{id}"),
				WorkCommand::Create {
					work: Work {
						id: id.into(),
						project_id: None,
						owner_agent_id: "sage".into(),
						title: id.into(),
						requirements: WorkRequirements {
							id: "r1".into(),
							goal: "Check".into(),
							criteria: "Evidence".into(),
							source: "user".into(),
						},
						state: WorkState::Open,
						reason: None,
						resume_condition: None,
						user_hold: false,
						assessment: None,
					},
				},
				true,
			)
			.await
			.unwrap();
	}

	(home, store)
}
async fn terminal(store: &SqliteStore, thread: &str, turn: &str, usage: Value, duration: Value) {
	store.enqueue_agent_event(EnqueueAgentEvent {source_event_id:serde_json::json!(["turn/completed",thread,turn]).to_string(),work_item_id:"sage".into(),event_kind:"agent_turn_completed".into(),payload:serde_json::json!({"terminal":{"threadId":thread,"turn":{"id":turn,"status":"completed","durationMs":duration}},"usage":usage,"threadReadback":{"tokenUsage":{"total":{"inputTokens":999_999,"outputTokens":999_999}}}}).to_string()}).await.unwrap();
}
async fn link(store: &SqliteStore, id: &str, work: &str, thread: &str, turn: &str) {
	let (id, work, thread, turn) =
		(id.to_owned(), work.to_owned(), thread.to_owned(), turn.to_owned());

	store.run(move|c| {
		let record=serde_json::json!({"source":{"kind":"native_item","thread_id":thread,"turn_id":turn,"item_id":id}}).to_string();

		c.execute("INSERT INTO work_results(id,work_id,requirements_id,instruction_id,agent_id,record_json) VALUES(?1,?2,'r1',NULL,'sage',?3)",rusqlite::params![id,work,record]).map_err(error::sqlite_error)?;

		Ok(())
	}).await.unwrap();
}

#[tokio::test]
async fn usage_totals_deduplicate_shared_turns_and_preserve_missing_proof_after_reopen() {
	let (home, store) = fixture().await;

	terminal(
		&store,
		"root",
		"own",
		serde_json::json!({"input_tokens":100,"output_tokens":20}),
		serde_json::json!(3_000),
	)
	.await;
	terminal(
		&store,
		"root",
		"shared",
		serde_json::json!({"input_tokens":200,"output_tokens":40}),
		serde_json::json!(4_000),
	)
	.await;
	terminal(
		&store,
		"root",
		"unassigned",
		serde_json::json!({"input_tokens":30,"output_tokens":5}),
		Value::Null,
	)
	.await;
	terminal(&store, "old-binding", "unknown", Value::Null, serde_json::json!(1_000)).await;
	link(&store, "a", "one", "root", "own").await;
	link(&store, "b", "one", "root", "own").await;
	link(&store, "c", "one", "root", "shared").await;
	link(&store, "d", "two", "root", "shared").await;
	link(&store, "e", "one", "old-binding", "unknown").await;
	link(&store, "f", "one", "child", "no-receipt").await;
	drop(store);

	let store = SqliteStore::open_test(&home.path().join("usage.db")).unwrap();
	let agent = store.read_usage_evidence(true, "sage".into()).await.unwrap().unwrap();

	assert_eq!((agent.turns, agent.token_turns, agent.duration_turns), (5, 3, 3));
	assert_eq!((agent.input_tokens, agent.output_tokens, agent.duration_ms), (330, 65, 8_000));
	assert_eq!((agent.shared_turns, agent.unattributed_turns), (1, 1));

	let work = store.read_usage_evidence(false, "one".into()).await.unwrap().unwrap();

	assert_eq!((work.turns, work.token_turns, work.duration_turns), (4, 1, 2));
	assert_eq!((work.input_tokens, work.output_tokens, work.duration_ms), (100, 20, 4_000));
	assert_eq!(work.shared_turns, 1);
	assert!(
		work.sources
			.iter()
			.any(|s| s.thread_id == "child" && s.event_id.is_none() && s.input_tokens.is_none())
	);
	assert!(store.read_usage_evidence(false, "missing".into()).await.unwrap().is_none());
	assert!(
		store
			.list_pending_agent_events(100)
			.await
			.unwrap()
			.iter()
			.all(|e| e.event_kind == "agent_turn_completed")
	);
}

#[tokio::test]
async fn usage_preview_limit_does_not_limit_totals_or_turn_identity() {
	let (_home, store) = fixture().await;

	for index in 0..40 {
		terminal(
			&store,
			&format!("thread-{index}"),
			"same-turn",
			serde_json::json!({"input_tokens":1,"output_tokens":2}),
			serde_json::json!(5),
		)
		.await;
	}

	let result = store.read_usage_evidence(true, "sage".into()).await.unwrap().unwrap();

	assert_eq!((result.turns, result.input_tokens, result.duration_ms), (40, 40, 200));
	assert_eq!(result.sources.len(), 32);
}
