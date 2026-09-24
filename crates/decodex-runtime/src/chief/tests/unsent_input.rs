use super::*;

#[tokio::test]
async fn only_local_refusals_without_prior_effects_release_input_after_restart() {
	for (error, no_prior_effects, unsent) in [
		(ClientError::RequestTooLarge, true, true),
		(ClientError::RequestQueueFull, true, true),
		(ClientError::RequestTooLarge, false, false),
		(ClientError::RequestQueueFull, false, false),
		(ClientError::FrameTooLarge, true, false),
		(ClientError::CapacityExceeded, true, false),
		(ClientError::Closed, true, false),
		(ClientError::Io, true, false),
	] {
		let (mut chief, mut sent, directory) = fixture().await;
		chief.start_chief("chief", "Start").await.unwrap();
		complete(&mut chief, "chief").await;
		let previous = chief.store.get_chief_work_item("chief".into()).await.unwrap();
		chief.enqueue_user_message("chief", "pending", "Keep this exact input").await.unwrap();
		let event = chief
			.store
			.list_pending_chief_events(100)
			.await
			.unwrap()
			.into_iter()
			.find(|e| e.event_kind == "user_message")
			.unwrap();
		chief
			.store
			.begin_chief_dispatch_with_input("chief".into(), vec![event.id], None)
			.await
			.unwrap();
		while sent.try_recv().is_ok() {}
		assert!(
			chief
				.finish_dispatch_attempt(
					&previous,
					None,
					Err(error.into()),
					None,
					no_prior_effects,
					None
				)
				.await
				.is_err()
		);
		let root =
			decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
				.unwrap();
		let reopened = SqliteStore::open(&root.paths()).unwrap();
		let saved = reopened.get_chief_inbox_event(event.id).await.unwrap();
		assert!(saved.payload.contains("Keep this exact input"));
		let work = reopened.get_chief_work_item("chief".into()).await.unwrap();
		if unsent {
			assert_eq!(saved.disposition, Some(ChiefDisposition::UserDecision));
			assert!(saved.delivered_turn_id.is_none());
			assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Idle);
			assert_eq!(work.status, ChiefWorkStatus::UserDecision);
			chief.check_due_followups(i64::MAX).await.unwrap();
		} else {
			assert!(saved.disposition.is_none());
			assert_eq!(saved.delivered_turn_id.as_deref(), Some(""));
			assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Unknown);
		}
		assert!(
			std::iter::from_fn(|| sent.try_recv().ok()).all(|r| r["method"] != "turn/start"),
			"a refusal is not retry authority"
		);
	}
}

#[tokio::test]
async fn local_question_refusal_releases_only_the_exact_pending_answer() {
	let (mut chief, _sent, _directory) = fixture().await;
	chief.start_chief("chief", "Start").await.unwrap();
	complete(&mut chief, "chief").await;
	let previous = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	let event = chief
		.store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "reply".into(),
			work_item_id: "chief".into(),
			event_kind: "async_question_answer".into(),
			payload: json!({"text":"My answer","asyncQuestionId":"question"}).to_string(),
		})
		.await
		.unwrap();
	chief
		.store
		.begin_chief_dispatch_with_input("chief".into(), vec![event.id], None)
		.await
		.unwrap();
	assert!(
		chief
			.finish_dispatch_attempt(
				&previous,
				Some(event.id),
				Err(ClientError::RequestQueueFull.into()),
				None,
				true,
				None
			)
			.await
			.is_err()
	);
	let saved = chief.store.get_chief_inbox_event(event.id).await.unwrap();
	assert_eq!(saved.disposition, Some(ChiefDisposition::Resolved));
	assert!(saved.disposition_note.unwrap().contains("local connection queue"));
	assert!(
		!chief.store.chief_async_answer_pending("chief".into(), "question".into()).await.unwrap()
	);
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Idle
	);
}
