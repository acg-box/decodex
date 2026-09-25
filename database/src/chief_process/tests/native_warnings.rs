use super::*;

#[tokio::test]
async fn native_warnings_stay_with_owned_threads_and_survive_reopen_without_waking() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("native-warnings.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	seed(&store).await;
	store.bind_chief_thread("root".into(), "root-thread".into()).await.unwrap();
	store.bind_chief_thread("second-root".into(), "other-thread".into()).await.unwrap();
	let mut child = store.get_chief_work_item("root".into()).await.unwrap();
	child.id = "child".into();
	child.parent_goal_id = Some("root".into());
	child.codex_thread_id = None;
	store.create_chief_work_item(child).await.unwrap();
	store.bind_chief_thread("child".into(), "child-thread".into()).await.unwrap();
	store
		.prepare_chief_bound_process_generation(&intent(1, 1), &binding(1), "root", "warnings")
		.await
		.unwrap();
	let generation = generation_id(1).as_str().to_owned();
	store
		.record_chief_native_warning(
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
		(generation_id(2).as_str().into(), Some("child-thread")),
	] {
		store
			.record_chief_native_warning(
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
			.record_chief_native_warning(
				"root".into(),
				generation.clone(),
				Some("child-thread".into()),
				DIGEST.into(),
				"Retaining the last global instructions".into(),
			)
			.await
			.unwrap();
		store
			.record_chief_native_warning(
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
		.record_chief_config_warning(
			"root".into(),
			generation.clone(),
			"b".repeat(64),
			"Process notice".into(),
		)
		.await
		.unwrap();
	store
		.record_chief_config_warning(
			"root".into(),
			generation.clone(),
			"c".repeat(64),
			"Retaining the last global instructions".into(),
		)
		.await
		.unwrap();
	store
		.record_chief_native_warning(
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
		let (events, _) = store.read_chief_transcript(work.into(), None, 100).await.unwrap();
		assert_eq!(events.len(), if work == "root" { 2 } else { 1 });
		assert_eq!(events[0].event_kind, "native_warning");
		assert_eq!(
			serde_json::from_str::<serde_json::Value>(&events[0].payload).unwrap()["text"],
			expected
		);
		assert!(store.list_chief_wake_events(work.into(), 10).await.unwrap().is_empty());
	}
	assert!(
		store.read_chief_transcript("second-root".into(), None, 10).await.unwrap().0.is_empty()
	);
	assert!(store.list_pending_chief_events(10).await.unwrap().is_empty());
	store.revalidate().await.unwrap();
}

async fn mark_warning_process_ready(store: &SqliteStore) {
	let identity = decodex_core::ProcessIdentity::new(
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		1234,
		decodex_core::ProcessStartIdentity::new("fixture-start").unwrap(),
		1234,
		1234,
	)
	.unwrap();
	store.bind_process_generation_identity(&generation_id(1), 1, &identity).await.unwrap();
	store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
}
