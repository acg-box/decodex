//! Journal qualification with native settings; process-death qualification lives in database tests.
use decodex_codex::app_server_client::AppServerClient;
use decodex_database::{
	AgentDispatchState, AgentModelAttempt, AgentWorkItem, AgentWorkKind, AgentWorkStatus,
	SqliteStore,
};

pub(super) async fn reserve(
	home: &std::path::Path,
	client: &AppServerClient,
	thread: &str,
	active_turn: Option<&str>,
	effort: Option<&str>,
) -> (SqliteStore, AgentModelAttempt, i64) {
	let root = decodex_core::DecodexRoot::new(home.canonicalize().expect("home").join("product"))
		.expect("product root");

	root.paths().ensure_layout().expect("layout");

	let store = SqliteStore::open(&root.paths()).expect("store");

	store
		.create_agent_work_item(AgentWorkItem {
			id: "model-task".into(),
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
		.expect("work");
	store.bind_agent_thread("model-task".into(), thread.into()).await.expect("binding");

	if let Some(turn) = active_turn {
		store.begin_agent_dispatch("model-task".into()).await.expect("dispatch");
		store
			.acknowledge_agent_dispatch("model-task".into(), turn.into())
			.await
			.expect("active turn");
	}

	crate::agent_models::persist_current(&store, client, thread, None).await.expect("native facts");

	let observed = store
		.agent_task_models("model-task".into(), thread.into(), None)
		.await
		.expect("read")
		.expect("settings");
	let attempt = AgentModelAttempt {
		work: "model-task".into(),
		thread: thread.into(),
		generation: None,
		settings_event: observed.id,
		model: "fixture-b".into(),
		model_provider: "fixture".into(),
		effort: effort
			.map(str::to_owned)
			.or_else(|| client.configured_task_models(thread).expect("current model").0.effort),
		review_token: "a".repeat(64),
		attempt_id: "model-selection".into(),
		manual_source: None,
		recovery: None,
	};
	let id = store
		.reserve_agent_model_selection(attempt.clone())
		.await
		.expect("reserve")
		.expect("reservation");

	assert!(store.begin_agent_dispatch("model-task".into()).await.is_err());

	(store, attempt, id)
}

pub(super) async fn observe(
	store: &SqliteStore,
	client: &AppServerClient,
	attempt: AgentModelAttempt,
	id: i64,
) {
	// Simulate the caller losing the acknowledgement while native publishes the saved settings.
	store
		.finish_agent_model_selection(id, attempt.clone(), "unknown".into())
		.await
		.expect("lost response");

	assert_eq!(
		store
			.agent_model_receipt(attempt.work.clone(), attempt.thread.clone())
			.await
			.expect("receipt")
			.expect("reserved")
			.state,
		"unknown"
	);

	crate::agent_models::persist_current(store, client, &attempt.thread, None)
		.await
		.expect("native publication");

	assert_eq!(
		store
			.agent_model_receipt(attempt.work, attempt.thread)
			.await
			.expect("receipt")
			.expect("observed")
			.state,
		"target_observed"
	);
	assert!(store.list_pending_agent_events(100).await.expect("pending events").is_empty());
}
