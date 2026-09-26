use super::*;

#[tokio::test]
async fn native_turn_adoption_requires_ready_current_process_ownership() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("native-owner.sqlite3")).unwrap();
	seed(&store).await;
	store.bind_chief_thread("root".into(), "thread".into()).await.unwrap();
	for generation in [1, 2] {
		store
			.prepare_chief_bound_process_generation(
				&intent(generation, generation),
				&binding(generation),
				"root",
				&format!("admit-{generation}"),
			)
			.await
			.unwrap();
		let current = Some(generation_id(generation).as_str().to_owned());
		assert!(
			!store
				.observe_chief_native_turn(
					"thread".into(),
					"turn".into(),
					current.clone(),
					"connection".into()
				)
				.await
				.unwrap()
		);
		assert!(
			!store
				.observe_chief_native_turn(
					"thread".into(),
					"turn".into(),
					None,
					"connection".into()
				)
				.await
				.unwrap()
		);
		let identity = decodex_core::ProcessIdentity::new(
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			1234,
			decodex_core::ProcessStartIdentity::new(format!("start-{generation}")).unwrap(),
			1234,
			1234,
		)
		.unwrap();
		store
			.bind_process_generation_identity(&generation_id(generation), 1, &identity)
			.await
			.unwrap();
		store.mark_process_generation_ready(&generation_id(generation), 2).await.unwrap();
		if generation == 2 {
			assert!(
				!store
					.observe_chief_native_turn(
						"thread".into(),
						"turn".into(),
						Some(generation_id(1).as_str().into()),
						"old".into()
					)
					.await
					.unwrap()
			);
		}
		assert!(
			store
				.observe_chief_native_turn(
					"thread".into(),
					"turn".into(),
					current.clone(),
					"connection".into()
				)
				.await
				.unwrap()
		);
		store.complete_chief_turn("root".into(), "turn".into()).await.unwrap();
		assert!(
			!store
				.observe_chief_native_turn(
					"thread".into(),
					"turn".into(),
					current.clone(),
					"reconnect".into()
				)
				.await
				.unwrap()
		);
		let death = ProcessDeathEvidence::new(
			ProcessDeathEvidenceId::new(format!("50000000-0000-4000-8000-{generation:012}"))
				.unwrap(),
			generation_id(generation),
			ProcessDeathEvidenceKind::OwnedChildExit,
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			Some(identity),
			DIGEST,
		)
		.unwrap();
		store.record_process_generation_death(3, &death).await.unwrap();
		assert!(
			!store
				.observe_chief_native_turn("thread".into(), "other".into(), current, "dead".into())
				.await
				.unwrap()
		);
		assert_eq!(
			store.get_chief_work_item("root".into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Idle
		);
	}
	assert!(store.list_pending_chief_events(10).await.unwrap().is_empty());
	assert!(store.list_chief_wake_events("root".into(), 10).await.unwrap().is_empty());
	store.revalidate().await.unwrap();
}
