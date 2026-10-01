use crate::{
	SqliteStore,
	agent_process::tests::{self, DIGEST},
};
use decodex_core::{
	ProcessBootIdentity, ProcessDeathEvidence, ProcessDeathEvidenceId, ProcessDeathEvidenceKind,
	ProcessIdentity, ProcessStartIdentity,
};

#[tokio::test]
async fn usage_requires_ready_owner_and_separates_rotated_accounts_after_reopen() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("response-ownership.sqlite3");
	let mut store = SqliteStore::open_test(&path).unwrap();

	tests::seed(&store).await;

	store.bind_agent_thread("root".into(), "thread".into()).await.unwrap();

	for (account, generation) in [(1, 1), (1, 2), (2, 3)] {
		let payload = serde_json::json!({"threadId":"thread","turnId":"turn","responseId":"response","usageMetadata":{"amount":account.to_string()}}).to_string();

		store
			.prepare_agent_bound_process_generation(
				&tests::intent(account, generation),
				&tests::binding(account),
				"root",
				&format!("admit-{generation}"),
			)
			.await
			.unwrap();

		let id = tests::generation_id(generation).as_str().to_owned();

		assert!(
			!store.record_agent_response_usage(Some(id.clone()), payload.clone()).await.unwrap()
		);
		assert!(!store.record_agent_response_usage(None, payload.clone()).await.unwrap());

		let identity = ProcessIdentity::new(
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			1_234,
			ProcessStartIdentity::new(format!("start-{generation}")).unwrap(),
			1_234,
			1_234,
		)
		.unwrap();

		store
			.bind_process_generation_identity(&tests::generation_id(generation), 1, &identity)
			.await
			.unwrap();
		store.mark_process_generation_ready(&tests::generation_id(generation), 2).await.unwrap();

		if generation > 1 {
			assert!(
				!store
					.record_agent_response_usage(
						Some(tests::generation_id(generation - 1).as_str().into()),
						payload.clone()
					)
					.await
					.unwrap()
			);
		}

		let before = store
			.read_agent_response_usage("root".into(), "thread".into(), vec!["turn".into()])
			.await
			.unwrap();

		assert_eq!(before.len(), usize::from(generation == 2));
		assert_eq!(
			store.record_agent_response_usage(Some(id.clone()), payload.clone()).await.unwrap(),
			generation != 2
		);

		let rows = store
			.read_agent_response_usage("root".into(), "thread".into(), vec!["turn".into()])
			.await
			.unwrap();

		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].amount, Some(account.to_string()));
		assert_eq!(rows[0].observed_count, 1);

		let death = ProcessDeathEvidence::new(
			ProcessDeathEvidenceId::new(format!("50000000-0000-4000-8000-{generation:012}"))
				.unwrap(),
			tests::generation_id(generation),
			ProcessDeathEvidenceKind::OwnedChildExit,
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			Some(identity),
			DIGEST,
		)
		.unwrap();

		store.record_process_generation_death(3, &death).await.unwrap();

		assert!(!store.record_agent_response_usage(Some(id), payload).await.unwrap());

		drop(store);

		store = SqliteStore::open_test(&path).unwrap();
	}

	assert!(store.list_pending_agent_events(10).await.unwrap().is_empty());

	store.revalidate().await.unwrap();
}
