//! Review notices require a ready current process, including after a restart.
use serde_json::Value;

use crate::{
	SqliteStore,
	agent_process::tests::{self, DIGEST},
};
use decodex_core::{
	ProcessBootIdentity, ProcessDeathEvidence, ProcessDeathEvidenceId, ProcessDeathEvidenceKind,
	ProcessIdentity, ProcessStartIdentity,
};

#[tokio::test]
async fn strict_review_notices_reject_old_processes_and_survive_reopen() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("guardian-notices.sqlite3");
	let mut store = SqliteStore::open_test(&path).unwrap();

	tests::seed(&store).await;

	store.bind_agent_thread("root".into(), "thread".into()).await.unwrap();

	for generation in [1, 2] {
		store
			.prepare_agent_bound_process_generation(
				&tests::intent(1, generation),
				&tests::binding(1),
				"root",
				&format!("admit-{generation}"),
			)
			.await
			.unwrap();
		store.begin_agent_dispatch("root".into()).await.unwrap();

		let turn = format!("turn-{generation}");

		store.acknowledge_agent_dispatch("root".into(), turn.clone()).await.unwrap();

		let id = tests::generation_id(generation).as_str().to_owned();
		let identity = ProcessIdentity::new(
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			1_234,
			ProcessStartIdentity::new(format!("start-{generation}")).unwrap(),
			1_234,
			1_234,
		)
		.unwrap();

		store
			.record_agent_strict_review("thread".into(), turn.clone(), 10, Some(id.clone()))
			.await
			.unwrap();
		store
			.bind_process_generation_identity(&tests::generation_id(generation), 1, &identity)
			.await
			.unwrap();
		store.mark_process_generation_ready(&tests::generation_id(generation), 2).await.unwrap();

		for owner in [None, Some(tests::generation_id(3 - generation).as_str().into())] {
			store
				.record_agent_strict_review("thread".into(), turn.clone(), 20, owner)
				.await
				.unwrap();
		}

		assert_notice_count(&store, usize::from(generation - 1)).await;

		for started in [30, 40] {
			store
				.record_agent_strict_review(
					"thread".into(),
					turn.clone(),
					started,
					Some(id.clone()),
				)
				.await
				.unwrap();
		}

		assert_notice_count(&store, usize::from(generation)).await;

		store.complete_agent_turn("root".into(), turn).await.unwrap();

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

		drop(store);

		store = SqliteStore::open_test(&path).unwrap();

		assert_notice_count(&store, usize::from(generation)).await;
	}

	assert!(store.list_pending_agent_events(10).await.unwrap().is_empty());
	assert!(store.list_agent_wake_events("root".into(), 10).await.unwrap().is_empty());

	store.revalidate().await.unwrap();
}

async fn assert_notice_count(store: &SqliteStore, count: usize) {
	let (events, _) = store.read_agent_transcript("root".into(), None, 32).await.unwrap();
	let notices: Vec<_> =
		events.iter().filter(|e| e.event_kind == "strict_review_notice").collect();

	assert_eq!(notices.len(), count);

	for notice in notices {
		let payload: Value = serde_json::from_str(&notice.payload).unwrap();

		assert_eq!(payload["startedAtMs"], 30);
	}
}
