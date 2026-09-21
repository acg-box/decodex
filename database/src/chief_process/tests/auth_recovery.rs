use super::*;

fn observation(generation: Option<u8>) -> crate::ChiefAuthRecoveryObservation {
	crate::ChiefAuthRecoveryObservation {
		thread_id: "thread".into(),
		turn_id: "turn".into(),
		provider: "AWS".into(),
		message: "Signed in with AWS.".into(),
		completed: true,
		connection_id: "connection".into(),
		generation_id: generation.map(|id| generation_id(id).as_str().to_owned()),
	}
}

#[tokio::test]
async fn auth_recovery_requires_ready_owner_across_rotation_and_restart() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("auth-ownership.sqlite3");
	let mut store = SqliteStore::open_test(&path).unwrap();
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
		store.begin_chief_dispatch("root".into()).await.unwrap();
		store.acknowledge_chief_dispatch("root".into(), "turn".into()).await.unwrap();
		assert!(!store.record_chief_auth_recovery(observation(Some(generation))).await.unwrap());
		assert!(!store.record_chief_auth_recovery(observation(None)).await.unwrap());
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
			assert!(!store.record_chief_auth_recovery(observation(Some(1))).await.unwrap());
		}
		assert!(store.record_chief_auth_recovery(observation(Some(generation))).await.unwrap());
		let rows = store.read_chief_work_events("root".into(), 10).await.unwrap();
		assert_eq!(rows.len(), usize::from(generation));
		let payload: serde_json::Value =
			serde_json::from_str(&rows.last().unwrap().payload).unwrap();
		assert_eq!(payload["accountId"], account_id(generation).as_str());
		assert_eq!(payload["generationId"], generation_id(generation).as_str());
		assert_eq!(payload["connectionId"], "connection");
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
		assert!(!store.record_chief_auth_recovery(observation(Some(generation))).await.unwrap());
		store.complete_chief_turn("root".into(), "turn".into()).await.unwrap();
		drop(store);
		store = SqliteStore::open_test(&path).unwrap();
		assert_eq!(
			store.read_chief_work_events("root".into(), 10).await.unwrap().len(),
			usize::from(generation)
		);
	}
	assert!(store.list_pending_chief_events(10).await.unwrap().is_empty());
	assert!(store.list_chief_wake_events("root".into(), 10).await.unwrap().is_empty());
	store.revalidate().await.unwrap();
}
