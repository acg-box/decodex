use crate::agent::tests::{self, AgentDisposition, AgentWorkStatus};

#[tokio::test]
async fn busy_worker_refuses_unsent_input_then_continues_once_idle() {
	let (mut coordinator, mut sent, _directory) = tests::fixture().await;
	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_worker("agent", "worker", "Prepare the cover").await.unwrap();
	let before = coordinator.store.get_agent_work_item("worker".into()).await.unwrap();
	let events_before =
		coordinator.store.read_agent_work_events("worker".into(), 100).await.unwrap();
	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let command = serde_json::json!({"tool":"agent_continue_worker","arguments":{"id":"worker","prompt":"Use Maple"}});
	while sent.try_recv().is_ok() {}

	let error = coordinator.tool(&agent, &command).await.unwrap_err().to_string();
	assert!(
		error.contains("not sent or queued")
			&& error.contains("agent_list_work")
			&& error.contains("idle"),
		"{error}"
	);
	assert!(sent.try_recv().is_err(), "Busy must not send native input");
	let busy = coordinator.store.get_agent_work_item("worker".into()).await.unwrap();
	assert_eq!(busy.active_turn_id, before.active_turn_id);
	assert_eq!(busy.dispatch_state, before.dispatch_state);
	assert_eq!(
		coordinator.store.read_agent_work_events("worker".into(), 100).await.unwrap().len(),
		events_before.len(),
		"Busy must not queue an instruction"
	);

	tests::complete(&mut coordinator, "worker").await;
	coordinator.tool(&agent, &command).await.unwrap();
	let continued = coordinator.store.get_agent_work_item("worker".into()).await.unwrap();
	assert_eq!(continued.codex_thread_id, before.codex_thread_id);
	assert_ne!(continued.active_turn_id, before.active_turn_id);
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|request| request["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["toolOutput"]["output"], "Use Maple");
}

#[tokio::test]
async fn work_tools_distinguish_agent_conversations_from_child_goals() {
	let (mut coordinator, _sent, _directory) = tests::fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_goal("agent", "goal", "Outcome").await.unwrap();
	coordinator.create_worker("goal", "worker", "Inspect").await.unwrap();
	coordinator.create_manager("agent", "manager", "Separate outcome").await.unwrap();

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let view = coordinator
		.tool(&agent, &serde_json::json!({"tool":"agent_list_work","arguments":{}}))
		.await
		.unwrap();
	let work = view["work"].as_array().unwrap();
	let kind = |id| work.iter().find(|item| item["id"] == id).unwrap()["kind"].clone();

	assert_eq!(kind("agent"), "agent");
	assert_eq!(kind("manager"), "agent");
	assert_eq!(kind("goal"), "goal");
	assert_eq!(kind("worker"), "task");
}

#[tokio::test]
async fn completion_errors_preserve_evidence_and_allow_the_correct_closure() {
	let (mut coordinator, _sent, _directory) = tests::fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_goal("agent", "goal", "Outcome").await.unwrap();
	coordinator.create_worker("goal", "worker", "Inspect").await.unwrap();
	tests::complete(&mut coordinator, "agent").await;
	tests::complete(&mut coordinator, "worker").await;

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = coordinator
		.store
		.list_agent_events_for_turn(agent.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "worker")
		.unwrap();
	let disposition = |id, ids| serde_json::json!({"tool":"agent_disposition","arguments":{"id":id,"status":"resolved","eventIds":ids,"summary":"Result accepted"}});

	let wrong = coordinator
		.tool(&agent, &disposition("agent", vec![event.id]))
		.await
		.unwrap_err()
		.to_string();
	assert!(wrong.contains("worker") && wrong.contains("belongs"), "{wrong}");
	let empty = coordinator
		.tool(&agent, &disposition("worker", Vec::<i64>::new()))
		.await
		.unwrap_err()
		.to_string();
	assert!(empty.contains("agent_list_work") && empty.contains(&event.id.to_string()), "{empty}");
	assert!(coordinator.store.get_agent_inbox_event(event.id).await.unwrap().disposition.is_none());

	coordinator.tool(&agent, &disposition("worker", vec![event.id])).await.unwrap();
	let replay = coordinator
		.tool(&agent, &disposition("worker", vec![event.id]))
		.await
		.unwrap_err()
		.to_string();
	assert!(replay.contains("not pending") && replay.contains("agent_list_work"), "{replay}");
	assert_eq!(
		coordinator.store.get_agent_inbox_event(event.id).await.unwrap().disposition,
		Some(AgentDisposition::Resolved)
	);

	// A goal judgment can cite the worker's already-assessed result from this turn.
	coordinator.tool(&agent, &serde_json::json!({"tool":"agent_resolve_goal","arguments":{"id":"goal","evidenceEventId":event.id,"summary":"Accepted worker result satisfies the goal"}})).await.unwrap();
	assert_eq!(
		coordinator.store.get_agent_work_item("worker".into()).await.unwrap().status,
		AgentWorkStatus::Resolved
	);
	assert_eq!(
		coordinator.store.get_agent_work_item("goal".into()).await.unwrap().status,
		AgentWorkStatus::Resolved
	);
	assert_eq!(
		coordinator.store.get_agent_work_item("agent".into()).await.unwrap().status,
		AgentWorkStatus::Open
	);
}

#[tokio::test]
async fn personal_agent_closure_explains_the_target_without_changing_it() {
	let (mut coordinator, _sent, _directory) = tests::fixture().await;

	coordinator.start_agent("agent", "Coordinate").await.unwrap();
	coordinator.create_worker("agent", "worker", "Inspect").await.unwrap();
	tests::complete(&mut coordinator, "agent").await;
	tests::complete(&mut coordinator, "worker").await;

	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let event = coordinator
		.store
		.list_agent_events_for_turn(agent.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.work_item_id == "worker")
		.unwrap();
	let command = |id| serde_json::json!({"tool":"agent_resolve_goal","arguments":{"id":id,"evidenceEventId":event.id,"summary":"Outcome met"}});
	let root = coordinator.tool(&agent, &command("agent")).await.unwrap_err().to_string();
	assert!(root.contains("Agent conversation") && root.contains("finish"), "{root}");
	let task = coordinator.tool(&agent, &command("worker")).await.unwrap_err().to_string();
	assert!(task.contains("child goal") && task.contains("agent_disposition"), "{task}");
	assert_eq!(
		coordinator.store.get_agent_work_item("agent".into()).await.unwrap().status,
		AgentWorkStatus::Open
	);
	assert!(coordinator.store.get_agent_inbox_event(event.id).await.unwrap().disposition.is_none());
}

#[tokio::test]
async fn user_input_finishes_without_resolving_the_agent_conversation() {
	let (mut coordinator, _sent, _directory) = tests::fixture().await;
	coordinator.start_agent("agent", "Discuss").await.unwrap();
	tests::complete(&mut coordinator, "agent").await;
	coordinator.enqueue_user_message("agent", "answer", "Keep the existing limit").await.unwrap();
	coordinator.wake_pending().await.unwrap();
	let agent = coordinator.store.get_agent_work_item("agent".into()).await.unwrap();
	let input = coordinator
		.store
		.list_agent_events_for_turn(agent.active_turn_id.clone().unwrap(), 100)
		.await
		.unwrap()
		.into_iter()
		.find(|event| event.event_kind == "user_message")
		.unwrap();
	let result = coordinator.tool(&agent, &serde_json::json!({"tool":"agent_disposition","arguments":{"id":"agent","eventIds":[input.id],"status":"resolved","summary":"User reply handled"}})).await.unwrap_err().to_string();
	assert!(result.contains("automatically") && result.contains("finish"), "{result}");
	assert!(coordinator.store.get_agent_inbox_event(input.id).await.unwrap().disposition.is_none());
	tests::complete(&mut coordinator, "agent").await;
	assert_eq!(
		coordinator.store.get_agent_inbox_event(input.id).await.unwrap().disposition,
		Some(AgentDisposition::Resolved)
	);
	assert_eq!(
		coordinator.store.get_agent_work_item("agent".into()).await.unwrap().status,
		AgentWorkStatus::Open
	);
}
