use super::*;

#[tokio::test]
async fn cold_resume_preserves_task_selection_and_partial_user_changes() {
	let (mut agent, mut sent, _directory) = fixture().await;
	agent.start_agent("agent", "Start").await.unwrap();
	complete(&mut agent, "agent").await;
	agent.store.enqueue_agent_event(EnqueueAgentEvent {
		source_event_id:"changed-model".into(),work_item_id:"agent".into(),event_kind:"user_message".into(),
		payload:json!({"text":"Use the selected model","options":{"attachments":[],"execution":{"model":"chosen-model","reasoning_effort":"future-effort"}}}).to_string(),
	}).await.unwrap();
	agent.wake_pending().await.unwrap();
	complete(&mut agent, "agent").await;
	let config = AgentConfig::new("new-startup-model".into(), "low".into(), "/tmp".into());
	let mut agent =
		AgentCoordinator::new(agent.store.clone(), agent.client.clone(), config).unwrap();
	while sent.try_recv().is_ok() {}
	agent.continue_worker("agent", "Continue").await.unwrap();
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	let resume = requests.iter().find(|r| r["method"] == "thread/resume").unwrap();
	assert_eq!(
		resume["params"],
		json!({"threadId":"opaque thread/1","excludeTurns":true,"experimentalRawEvents":true,"initialTurnsPage":{"limit":1,"sortDirection":"desc","itemsView":"summary"}})
	);
	let turn = requests.iter().find(|r| r["method"] == "turn/start").unwrap();
	assert_eq!(turn["params"]["model"], "chosen-model");
	assert_eq!(turn["params"]["effort"], "future-effort");
	complete(&mut agent, "agent").await;
	agent.store.enqueue_agent_event(EnqueueAgentEvent {
		source_event_id:"effort-only".into(),work_item_id:"agent".into(),event_kind:"user_message".into(),
		payload:json!({"text":"Only change effort","options":{"attachments":[],"execution":{"reasoning_effort":"high"}}}).to_string(),
	}).await.unwrap();
	agent.wake_pending().await.unwrap();
	let turn =
		std::iter::from_fn(|| sent.try_recv().ok()).find(|r| r["method"] == "turn/start").unwrap();
	assert_eq!(turn["params"]["model"], "chosen-model");
	assert_eq!(turn["params"]["effort"], "high");
}

#[tokio::test]
async fn a_changed_native_selection_cancels_capacity_retry_without_a_new_turn() {
	let failure = json!({"id":"opaque turn/1","status":"failed","error":{"message":"At capacity","codexErrorInfo":"serverOverloaded"},"items":[]});
	let (mut agent, mut sent, _directory) = fixture_with_history(
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[failure]}}}),
	)
	.await;
	agent.start_agent("agent", "Start").await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":failure}),
		})
		.await
		.unwrap();
	assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_some());
	while sent.try_recv().is_ok() {}
	for (thread, model) in [("foreign", "different"), ("opaque thread/1", "selected-model")] {
		agent
			.handle_event(ServerEvent::Notification {
				method: "thread/settings/updated".into(),
				params: json!({"threadId":thread,"threadSettings":{"model":model,"effort":"high"}}),
			})
			.await
			.unwrap();
		assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_some());
	}
	agent.handle_event(ServerEvent::Notification {method:"thread/settings/updated".into(),params:json!({"threadId":"opaque thread/1","threadSettings":{"model":"new-model","effort":"high"}})}).await.unwrap();
	assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());
	agent.check_due_followups(i64::MAX).await.unwrap();
	assert!(std::iter::from_fn(|| sent.try_recv().ok()).all(|r| r["method"] != "turn/start"));
}

#[tokio::test]
async fn known_settings_refusal_preserves_unsent_input_for_user_decision() {
	let (mut agent, mut sent, _directory) = fixture().await;
	agent.start_agent("agent", "Start").await.unwrap();
	complete(&mut agent, "agent").await;
	let previous = agent.store.get_agent_work_item("agent".into()).await.unwrap();
	agent.enqueue_user_message("agent", "pending", "Keep this input").await.unwrap();
	let event = agent
		.store
		.list_pending_agent_events(100)
		.await
		.unwrap()
		.into_iter()
		.find(|e| e.event_kind == "user_message")
		.unwrap();
	agent
		.store
		.begin_agent_dispatch_with_input("agent".into(), vec![event.id], None)
		.await
		.unwrap();
	while sent.try_recv().is_ok() {}
	let result = agent
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
		Err(AgentError::InputNotSent(decodex_database::AgentDispatchRefusal::SettingsChanged))
	));
	let event = agent.store.get_agent_inbox_event(event.id).await.unwrap();
	assert_eq!(event.disposition, Some(AgentDisposition::UserDecision));
	assert!(event.payload.contains("Keep this input"));
	assert!(event.delivered_turn_id.is_none());
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn capacity_retry_keeps_the_acknowledged_selection_after_store_reopen() {
	let failure = json!({"id":"opaque turn/1","status":"failed","error":{"message":"At capacity","codexErrorInfo":"serverOverloaded"},"items":[]});
	let (mut agent, mut sent, directory) = fixture_with_history(
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[failure]}}}),
	)
	.await;
	agent.start_agent("agent", "Start").await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":failure}),
		})
		.await
		.unwrap();
	let retry = agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().unwrap();
	let client = agent.client.clone();
	drop(agent);
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	let config = AgentConfig::new("different-default-model".into(), "low".into(), "/tmp".into());
	let mut agent = AgentCoordinator::new(reopened, client, config).unwrap();
	while sent.try_recv().is_ok() {}
	agent.check_due_followups(retry.due_at_micros).await.unwrap();
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|r| r["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["model"], "selected-model");
	assert_eq!(starts[0]["params"]["effort"], "high");
	assert_eq!(starts[0]["params"]["toolOutput"]["name"], "capacity_retry");
}

#[tokio::test]
async fn resume_bootstrap_avoids_a_second_history_read_and_does_not_replay_input() {
	let (mut agent, mut sent, _home) = fixture_with_history(
		json!({"thread":{"thread":{"id":"thread","turns":[{"id":"latest","items":[]}]}}}),
	)
	.await;
	for page in [
		json!({"data":[{"id":"latest","items":[],"itemsView":"summary"}],"nextCursor":"older"}),
		json!({"data":[],"nextCursor":null}),
	] {
		let response = json!({"thread":{"id":"thread","turns":[]},"initialTurnsPage":page});
		let latest =
			agent.expect_usage_replay("thread", &response, agent.client.history_revision()).await;
		assert_eq!(latest.as_deref(), page["data"][0]["id"].as_str());
		assert!(sent.try_recv().is_err(), "bootstrap data must not cause another RPC or input");
	}
	let legacy = json!({"thread":{"id":"thread","turns":[]}});
	assert_eq!(
		agent
			.expect_usage_replay("thread", &legacy, agent.client.history_revision())
			.await
			.as_deref(),
		Some("latest")
	);
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	assert!(!requests.is_empty());
	assert!(requests.iter().all(|request| request["method"] == "thread/read"));
	let stale = json!({"thread":{"id":"thread"},"initialTurnsPage":{"data":[{"id":"stale"}]}});
	assert!(
		agent
			.expect_usage_replay("thread", &stale, agent.client.history_revision() + 1)
			.await
			.is_none()
	);
	assert!(!agent.usage_replays.contains_key("thread"));
	assert!(sent.try_recv().is_err());
}
