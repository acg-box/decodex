use super::*;
use crate::ChiefReasoningSummaryChange as Change;

#[tokio::test]
async fn reasoning_parts_survive_reopen_and_completion_replaces_missing_or_late_deltas() {
	let directory = tempdir().unwrap();
	let path = directory.path().join("summary.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	store.create_chief_work_item(item("chief", None)).await.unwrap();
	store.bind_chief_thread("chief".into(), "thread".into()).await.unwrap();
	store.begin_chief_dispatch("chief".into()).await.unwrap();
	store.acknowledge_chief_dispatch("chief".into(), "turn".into()).await.unwrap();
	let (revision, _) = store.wait_chief_output("chief".into(), None).await.unwrap();
	store
		.update_chief_reasoning_summary(
			"thread".into(),
			"turn".into(),
			"item".into(),
			None,
			Change::Delta { index: 32, text: "omitted".into() },
		)
		.await
		.unwrap();
	let (next, values) = tokio::time::timeout(
		std::time::Duration::from_secs(1),
		store.wait_chief_output("chief".into(), Some(revision)),
	)
	.await
	.unwrap()
	.unwrap();
	assert_ne!(next, revision);
	assert!(values[0].truncated);
	for (index, text) in [(1, "Second."), (0, "First"), (0, ".")] {
		store
			.update_chief_reasoning_summary(
				"thread".into(),
				"turn".into(),
				"item".into(),
				None,
				Change::Delta { index, text: text.into() },
			)
			.await
			.unwrap();
	}
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let output = store.read_chief_output("chief".into()).await.unwrap();
	assert_eq!(output[0].text, "First.\n\nSecond.");
	assert_eq!(output[0].kind, "reasoningSummary");
	store
		.update_chief_reasoning_summary(
			"thread".into(),
			"turn".into(),
			"handoff".into(),
			None,
			Change::VoiceHandoff,
		)
		.await
		.unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_voice_origins(&store).await;
	assert!(
		store
			.read_chief_transcript("chief".into(), None, 1)
			.await
			.unwrap()
			.0
			.iter()
			.all(|event| event.event_kind != "reasoning_voice_handoff")
	);
	store
		.update_chief_reasoning_summary(
			"thread".into(),
			"turn".into(),
			"private-voice".into(),
			None,
			Change::Completed { parts: vec!["PRIVATE_VOICE".into()] },
		)
		.await
		.unwrap();
	assert_eq!(store.read_chief_output("chief".into()).await.unwrap().len(), 1);
	for (thread, turn, generation) in [
		("other", "turn", None),
		("thread", "old", None),
		("thread", "turn", Some("foreign".into())),
	] {
		store
			.update_chief_reasoning_summary(
				thread.into(),
				turn.into(),
				"item".into(),
				generation,
				Change::Delta { index: 0, text: "unowned".into() },
			)
			.await
			.unwrap();
	}
	assert_eq!(store.read_chief_output("chief".into()).await.unwrap()[0].text, output[0].text);
	assert_completion_and_bounds(&store).await;
}

async fn assert_completion_and_bounds(store: &SqliteStore) {
	store
		.update_chief_reasoning_summary(
			"thread".into(),
			"turn".into(),
			"item".into(),
			None,
			Change::Delta { index: 32, text: "Outside the bounded preview".into() },
		)
		.await
		.unwrap();
	assert!(store.read_chief_output("chief".into()).await.unwrap()[0].truncated);
	store
		.update_chief_reasoning_summary(
			"thread".into(),
			"turn".into(),
			"item".into(),
			None,
			Change::Completed { parts: vec!["Corrected complete summary.".into()] },
		)
		.await
		.unwrap();
	store
		.update_chief_reasoning_summary(
			"thread".into(),
			"turn".into(),
			"item".into(),
			None,
			Change::Delta { index: 0, text: "late".into() },
		)
		.await
		.unwrap();
	assert_eq!(
		store.read_chief_output("chief".into()).await.unwrap()[0].text,
		"Corrected complete summary."
	);
	assert!(store.list_chief_wake_events("chief".into(), 32).await.unwrap().is_empty());
	store
		.update_chief_reasoning_summary(
			"thread".into(),
			"turn".into(),
			"item".into(),
			None,
			Change::Completed { parts: vec!["界".repeat(30000)] },
		)
		.await
		.unwrap();
	let output = store.read_chief_output("chief".into()).await.unwrap();
	let large = output.iter().find(|row| row.item_id == "item").unwrap();
	assert!(large.truncated && large.text.len() <= 65536 && large.text.chars().all(|c| c == '界'));
}

async fn assert_voice_origins(store: &SqliteStore) {
	assert_eq!(
		store
			.read_chief_reasoning_origins("chief".into(), "thread".into(), vec!["turn".into()])
			.await
			.unwrap(),
		vec![("turn".into(), vec!["item".into()])]
	);
	assert!(
		store
			.read_chief_reasoning_origins(
				"chief".into(),
				"other-thread".into(),
				vec!["turn".into()]
			)
			.await
			.unwrap()
			.is_empty()
	);
}
