use super::*;

#[tokio::test]
async fn cold_resume_preserves_task_selection_and_partial_user_changes() {
	let (mut chief, mut sent, _directory) = fixture().await;
	chief.start_chief("chief", "Start").await.unwrap();
	complete(&mut chief, "chief").await;
	chief.store.enqueue_chief_event(EnqueueChiefEvent {
		source_event_id:"changed-model".into(),work_item_id:"chief".into(),event_kind:"user_message".into(),
		payload:json!({"text":"Use the selected model","options":{"attachments":[],"execution":{"model":"chosen-model","reasoning_effort":"future-effort"}}}).to_string(),
	}).await.unwrap();
	chief.wake_pending().await.unwrap();
	complete(&mut chief, "chief").await;
	let config = ChiefConfig::new("new-startup-model".into(), "low".into(), "/tmp".into());
	let mut chief =
		ChiefCoordinator::new(chief.store.clone(), chief.client.clone(), config).unwrap();
	while sent.try_recv().is_ok() {}
	chief.continue_worker("chief", "Continue").await.unwrap();
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	let resume = requests.iter().find(|r| r["method"] == "thread/resume").unwrap();
	assert_eq!(
		resume["params"],
		json!({"threadId":"opaque thread/1","excludeTurns":true,"experimentalRawEvents":true})
	);
	let turn = requests.iter().find(|r| r["method"] == "turn/start").unwrap();
	assert_eq!(turn["params"]["model"], "chosen-model");
	assert_eq!(turn["params"]["effort"], "future-effort");
	complete(&mut chief, "chief").await;
	chief.store.enqueue_chief_event(EnqueueChiefEvent {
		source_event_id:"effort-only".into(),work_item_id:"chief".into(),event_kind:"user_message".into(),
		payload:json!({"text":"Only change effort","options":{"attachments":[],"execution":{"reasoning_effort":"high"}}}).to_string(),
	}).await.unwrap();
	chief.wake_pending().await.unwrap();
	let turn =
		std::iter::from_fn(|| sent.try_recv().ok()).find(|r| r["method"] == "turn/start").unwrap();
	assert_eq!(turn["params"]["model"], "chosen-model");
	assert_eq!(turn["params"]["effort"], "high");
}

#[tokio::test]
async fn a_changed_native_selection_cancels_capacity_retry_without_a_new_turn() {
	let failure = json!({"id":"opaque turn/1","status":"failed","error":{"message":"At capacity","codexErrorInfo":"serverOverloaded"},"items":[]});
	let (mut chief, mut sent, _directory) = fixture_with_history(
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[failure]}}}),
	)
	.await;
	chief.start_chief("chief", "Start").await.unwrap();
	chief
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":failure}),
		})
		.await
		.unwrap();
	assert!(chief.store.pending_chief_capacity_retry("chief".into()).await.unwrap().is_some());
	while sent.try_recv().is_ok() {}
	for (thread, model) in [("foreign", "different"), ("opaque thread/1", "selected-model")] {
		chief
			.handle_event(ServerEvent::Notification {
				method: "thread/settings/updated".into(),
				params: json!({"threadId":thread,"threadSettings":{"model":model,"effort":"high"}}),
			})
			.await
			.unwrap();
		assert!(chief.store.pending_chief_capacity_retry("chief".into()).await.unwrap().is_some());
	}
	chief.handle_event(ServerEvent::Notification {method:"thread/settings/updated".into(),params:json!({"threadId":"opaque thread/1","threadSettings":{"model":"new-model","effort":"high"}})}).await.unwrap();
	assert!(chief.store.pending_chief_capacity_retry("chief".into()).await.unwrap().is_none());
	chief.check_due_followups(i64::MAX).await.unwrap();
	assert!(std::iter::from_fn(|| sent.try_recv().ok()).all(|r| r["method"] != "turn/start"));
}

#[tokio::test]
async fn known_settings_refusal_preserves_unsent_input_for_user_decision() {
	let (mut chief, mut sent, _directory) = fixture().await;
	chief.start_chief("chief", "Start").await.unwrap();
	complete(&mut chief, "chief").await;
	let previous = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	chief.enqueue_user_message("chief", "pending", "Keep this input").await.unwrap();
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
	let result = chief
		.finish_dispatch_attempt(
			&previous,
			None,
			Err(ClientError::StaleHistory.into()),
			None,
			true,
			None,
		)
		.await;
	assert!(matches!(
		result,
		Err(ChiefError::InputNotSent(decodex_database::ChiefDispatchRefusal::SettingsChanged))
	));
	let event = chief.store.get_chief_inbox_event(event.id).await.unwrap();
	assert_eq!(event.disposition, Some(ChiefDisposition::UserDecision));
	assert!(event.payload.contains("Keep this input"));
	assert!(event.delivered_turn_id.is_none());
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Idle
	);
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn capacity_retry_keeps_the_acknowledged_selection_after_store_reopen() {
	let failure = json!({"id":"opaque turn/1","status":"failed","error":{"message":"At capacity","codexErrorInfo":"serverOverloaded"},"items":[]});
	let (mut chief, mut sent, directory) = fixture_with_history(
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[failure]}}}),
	)
	.await;
	chief.start_chief("chief", "Start").await.unwrap();
	chief
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":failure}),
		})
		.await
		.unwrap();
	let retry = chief.store.pending_chief_capacity_retry("chief".into()).await.unwrap().unwrap();
	let client = chief.client.clone();
	drop(chief);
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	let config = ChiefConfig::new("different-default-model".into(), "low".into(), "/tmp".into());
	let mut chief = ChiefCoordinator::new(reopened, client, config).unwrap();
	while sent.try_recv().is_ok() {}
	chief.check_due_followups(retry.due_at_micros).await.unwrap();
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|r| r["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["model"], "selected-model");
	assert_eq!(starts[0]["params"]["effort"], "high");
	assert_eq!(starts[0]["params"]["toolOutput"]["name"], "capacity_retry");
}
