//! Journal qualification with native settings; process-death qualification lives in database tests.
use decodex_codex::app_server_client::AppServerClient;
use decodex_database::{
	ChiefDispatchState, ChiefModelAttempt, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus,
	SqliteStore,
};

pub(super) async fn reserve(
	home: &std::path::Path,
	client: &AppServerClient,
	thread: &str,
	active_turn: Option<&str>,
	effort: &str,
) -> (SqliteStore, ChiefModelAttempt, i64) {
	let root = decodex_core::DecodexRoot::new(home.canonicalize().expect("home").join("product"))
		.expect("product root");
	root.paths().ensure_layout().expect("layout");
	let store = SqliteStore::open(&root.paths()).expect("store");
	store
		.create_chief_work_item(ChiefWorkItem {
			id: "model-task".into(),
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
		.expect("work");
	store.bind_chief_thread("model-task".into(), thread.into()).await.expect("binding");
	if let Some(turn) = active_turn {
		store.begin_chief_dispatch("model-task".into()).await.expect("dispatch");
		store
			.acknowledge_chief_dispatch("model-task".into(), turn.into())
			.await
			.expect("active turn");
	}
	crate::chief_models::persist_current(&store, client, thread, None).await.expect("native facts");
	let observed = store
		.chief_task_models("model-task".into(), thread.into(), None)
		.await
		.expect("read")
		.expect("settings");
	let attempt = ChiefModelAttempt {
		work: "model-task".into(),
		thread: thread.into(),
		generation: None,
		settings_event: observed.id,
		model: "fixture-b".into(),
		model_provider: "fixture".into(),
		effort: Some(effort.into()),
		review_token: "a".repeat(64),
		attempt_id: "model-selection".into(),
	};
	let id = store
		.reserve_chief_model_selection(attempt.clone())
		.await
		.expect("reserve")
		.expect("reservation");
	assert!(store.begin_chief_dispatch("model-task".into()).await.is_err());
	(store, attempt, id)
}

pub(super) async fn observe(
	store: &SqliteStore,
	client: &AppServerClient,
	attempt: ChiefModelAttempt,
	id: i64,
) {
	// Simulate the caller losing the acknowledgement while native publishes the saved settings.
	store
		.finish_chief_model_selection(id, attempt.clone(), "unknown".into())
		.await
		.expect("lost response");
	assert_eq!(
		store
			.chief_model_receipt(attempt.work.clone(), attempt.thread.clone())
			.await
			.expect("receipt")
			.expect("reserved")
			.state,
		"unknown"
	);
	crate::chief_models::persist_current(store, client, &attempt.thread, None)
		.await
		.expect("native publication");
	assert_eq!(
		store
			.chief_model_receipt(attempt.work, attempt.thread)
			.await
			.expect("receipt")
			.expect("observed")
			.state,
		"target_observed"
	);
	assert!(store.list_pending_chief_events(100).await.expect("pending events").is_empty());
}
