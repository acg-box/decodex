use serde_json::Value;

use crate::{
	SqliteStore,
	agent_process::tests::{self, DIGEST},
};
use decodex_core::{ProcessBootIdentity, ProcessIdentity, ProcessStartIdentity};

#[tokio::test]
async fn native_warnings_stay_with_owned_threads_and_survive_reopen_without_waking() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("native-warnings.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	tests::seed(&store).await;

	seed_warning_threads(&store).await;

	let generation = tests::generation_id(1).as_str().to_owned();

	store
		.record_agent_native_warning(
			"root".into(),
			generation.clone(),
			None,
			DIGEST.into(),
			"not ready".into(),
		)
		.await
		.unwrap();

	mark_warning_process_ready(&store).await;

	for (owner, thread) in [
		(generation.clone(), Some("other-thread")),
		(generation.clone(), Some("unknown")),
		(tests::generation_id(2).as_str().into(), Some("child-thread")),
	] {
		store
			.record_agent_native_warning(
				"root".into(),
				owner,
				thread.map(str::to_owned),
				DIGEST.into(),
				"wrong owner".into(),
			)
			.await
			.unwrap();
	}
	for _ in 0..2 {
		store
			.record_agent_native_warning(
				"root".into(),
				generation.clone(),
				Some("child-thread".into()),
				DIGEST.into(),
				"Retaining the last global instructions".into(),
			)
			.await
			.unwrap();
		store
			.record_agent_native_warning(
				"root".into(),
				generation.clone(),
				None,
				DIGEST.into(),
				"Process notice".into(),
			)
			.await
			.unwrap();
	}

	// Startup config diagnostics can be repeated as thread warnings. Collapse the
	// same root/generation text, but retain the same notice on a different task.
	store
		.record_agent_config_warning(
			"root".into(),
			generation.clone(),
			"b".repeat(64),
			"Process notice".into(),
		)
		.await
		.unwrap();
	store
		.record_agent_config_warning(
			"root".into(),
			generation.clone(),
			"c".repeat(64),
			"Retaining the last global instructions".into(),
		)
		.await
		.unwrap();
	store
		.record_agent_native_warning(
			"root".into(),
			generation.clone(),
			None,
			"d".repeat(64),
			"Retaining the last global instructions".into(),
		)
		.await
		.unwrap();

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();

	for (work, expected) in
		[("root", "Process notice"), ("child", "Retaining the last global instructions")]
	{
		let (events, _) = store.read_agent_transcript(work.into(), None, 100).await.unwrap();

		assert_eq!(events.len(), if work == "root" { 2 } else { 1 });
		assert_eq!(events[0].event_kind, "native_warning");
		assert_eq!(serde_json::from_str::<Value>(&events[0].payload).unwrap()["text"], expected);
		assert!(store.list_agent_wake_events(work.into(), 10).await.unwrap().is_empty());
	}

	assert!(
		store.read_agent_transcript("second-root".into(), None, 10).await.unwrap().0.is_empty()
	);
	assert!(store.list_pending_agent_events(10).await.unwrap().is_empty());

	store.revalidate().await.unwrap();
}

async fn mark_warning_process_ready(store: &SqliteStore) {
	let identity = ProcessIdentity::new(
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		1_234,
		ProcessStartIdentity::new("fixture-start").unwrap(),
		1_234,
		1_234,
	)
	.unwrap();

	store.bind_process_generation_identity(&tests::generation_id(1), 1, &identity).await.unwrap();
	store.mark_process_generation_ready(&tests::generation_id(1), 2).await.unwrap();
}

async fn seed_warning_threads(store: &SqliteStore) {
	store.bind_agent_thread("root".into(), "root-thread".into()).await.unwrap();
	store.bind_agent_thread("second-root".into(), "other-thread".into()).await.unwrap();

	let mut child = store.get_agent_work_item("root".into()).await.unwrap();

	child.id = "child".into();
	child.parent_goal_id = Some("root".into());
	child.codex_thread_id = None;

	store.create_agent_work_item(child).await.unwrap();
	store.bind_agent_thread("child".into(), "child-thread".into()).await.unwrap();
	store
		.prepare_agent_bound_process_generation(
			&tests::intent(1, 1),
			&tests::binding(1),
			"root",
			"warnings",
		)
		.await
		.unwrap();
}
