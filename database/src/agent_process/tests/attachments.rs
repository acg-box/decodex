use super::{SqliteStore, binding, generation_id, identity, intent, seed};

#[tokio::test]
async fn independent_native_attachment_requires_exact_thread_and_live_uncontested_process() {
	let temporary = tempfile::tempdir().unwrap();
	let path = temporary.path().join("attachment.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	seed(&store).await;
	store.bind_agent_thread("root".into(), "first-thread".into()).await.unwrap();
	store.bind_agent_thread("second-root".into(), "second-thread".into()).await.unwrap();
	store
		.prepare_agent_bound_process_generation(&intent(1, 1), &binding(1), "root", "host")
		.await
		.unwrap();
	let generation = generation_id(1).as_str().to_owned();
	assert!(
		!store
			.attach_independent_agent_process(
				"second-root".into(),
				"second-thread".into(),
				generation.clone()
			)
			.await
			.unwrap()
	);
	store.bind_process_generation_identity(&generation_id(1), 1, &identity(123)).await.unwrap();
	store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
	assert!(
		!store
			.agent_thread_is_owned(
				"second-root".into(),
				"second-thread".into(),
				Some(generation.clone())
			)
			.await
			.unwrap()
	);
	assert!(
		!store
			.attach_independent_agent_process(
				"second-root".into(),
				"wrong-thread".into(),
				generation.clone()
			)
			.await
			.unwrap()
	);
	for _ in 0..2 {
		assert!(
			store
				.attach_independent_agent_process(
					"second-root".into(),
					"second-thread".into(),
					generation.clone()
				)
				.await
				.unwrap()
		);
	}
	assert!(
		store
			.agent_thread_is_owned(
				"second-root".into(),
				"second-thread".into(),
				Some(generation.clone())
			)
			.await
			.unwrap()
	);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert!(
		store
			.agent_thread_is_owned(
				"second-root".into(),
				"second-thread".into(),
				Some(generation.clone())
			)
			.await
			.unwrap()
	);
	assert_eq!(
		store.read_agent_process_binding("second-root").await.unwrap().unwrap().generation_id,
		generation_id(1)
	);
	assert!(
		!store
			.attach_independent_agent_process(
				"second-root".into(),
				"second-thread".into(),
				generation_id(2).as_str().into()
			)
			.await
			.unwrap()
	);
	store
		.mark_process_generation_death_unknown(
			&generation_id(1),
			3,
			decodex_core::ProcessAuthorityLossReason::SupervisorRestarted,
		)
		.await
		.unwrap();
	assert!(
		!store
			.attach_independent_agent_process(
				"second-root".into(),
				"second-thread".into(),
				generation
			)
			.await
			.unwrap()
	);
}
