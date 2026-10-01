use std::iter;

use tokio::io;

use crate::agent::tests::{
	self, AgentCoordinator, AgentDisposition, AgentError, AppServerClient, AsyncBufReadExt as _,
	AsyncWriteExt as _, BufReader, EnqueueAgentEvent, ServerEvent, SqliteStore, Value,
};
use decodex_core::DecodexRoot;

fn failed(turn: &str, code: &str) -> Value {
	serde_json::json!({"id":turn,"status":"failed","error":{"message":"Selected model is at capacity.","codexErrorInfo":code},"items":[{"id":"input","type":"userMessage","content":[{"type":"text","text":"original request"}]}]})
}

#[tokio::test]
async fn fresh_user_input_supersedes_a_due_capacity_retry() {
	let first = failed("opaque turn/1", "serverOverloaded");
	let (mut agent, mut sent, _dir) = tests::fixture_with_history(
		serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first]}}}),
	)
	.await;

	agent.start_agent("agent", "original request").await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":first}),
		})
		.await
		.unwrap();
	agent.enqueue_user_message("agent", "new-command", "changed request").await.unwrap();

	while sent.try_recv().is_ok() {}

	agent.check_due_followups(i64::MAX).await.unwrap();

	let starts: Vec<_> = iter::from_fn(|| sent.try_recv().ok())
		.filter(|frame| frame["method"] == "turn/start")
		.collect();

	assert_eq!(starts.len(), 1);
	assert!(starts[0]["params"]["input"][0]["text"].as_str().unwrap().contains("changed request"));
	assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());
}

#[tokio::test]
async fn successful_capacity_continuation_handles_the_original_input_once() {
	let first = failed("opaque turn/1", "serverOverloaded");
	let second = serde_json::json!({"id":"opaque turn/2","status":"completed","items":[]});
	let (mut agent, mut sent, _dir) = tests::fixture_with_history(
		serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first,second]}}}),
	)
	.await;

	AgentCoordinator::reserve_root(&agent.store, "agent", "original request").await.unwrap();

	agent.enqueue_user_message("agent", "command", "original request").await.unwrap();
	agent.wake_pending().await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":first}),
		})
		.await
		.unwrap();

	let retry = agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().unwrap();

	while sent.try_recv().is_ok() {}

	agent.check_due_followups(retry.due_at_micros).await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":second}),
		})
		.await
		.unwrap();

	assert!(agent.store.list_pending_agent_events(100).await.unwrap().is_empty());

	let history = agent.store.read_agent_work_events("agent".into(), 100).await.unwrap();
	let inputs: Vec<_> =
		history.iter().filter(|event| event.event_kind == "user_message").collect();

	assert_eq!(inputs.len(), 1);
	assert_eq!(inputs[0].delivered_turn_id.as_deref(), Some("opaque turn/2"));
	assert_eq!(inputs[0].disposition, Some(AgentDisposition::Resolved));

	let starts: Vec<_> = iter::from_fn(|| sent.try_recv().ok())
		.filter(|frame| frame["method"] == "turn/start")
		.collect();

	assert_eq!(starts.len(), 1);
	assert!(
		!starts[0]["params"]["toolOutput"]["output"].as_str().unwrap().contains("original request")
	);
}

#[tokio::test]
async fn worker_capacity_wait_does_not_wake_agent_and_cancel_publishes_failure() {
	for draining in [false, true] {
		let first = serde_json::json!({"id":"opaque turn/1","status":"completed","items":[]});
		let failure = failed("opaque turn/2", "serverOverloaded");
		let (mut agent, mut sent, _dir) = tests::fixture_with_history(serde_json::json!({
			"_capacity_draining":draining,
			"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first]}},
			"opaque thread/2":{"thread":{"id":"opaque thread/2","turns":[failure]}}
		}))
		.await;

		agent.start_agent("agent", "coordinate").await.unwrap();
		agent
			.handle_event(ServerEvent::Notification {
				method: "turn/completed".into(),
				params: serde_json::json!({"threadId":"opaque thread/1","turn":first}),
			})
			.await
			.unwrap();
		agent.create_worker("agent", "worker", "work").await.unwrap();

		while sent.try_recv().is_ok() {}

		agent
			.handle_event(ServerEvent::Notification {
				method: "turn/completed".into(),
				params: serde_json::json!({"threadId":"opaque thread/2","turn":failure}),
			})
			.await
			.unwrap();

		assert!(
			!iter::from_fn(|| sent.try_recv().ok()).any(|frame| frame["method"] == "turn/start")
		);

		let retry =
			agent.store.pending_agent_capacity_retry("worker".into()).await.unwrap().unwrap();

		if draining {
			assert!(matches!(
				agent.check_due_followups(retry.due_at_micros).await,
				Err(AgentError::InputNotSent(
					decodex_database::AgentDispatchRefusal::ServerDraining
				))
			));

			while sent.try_recv().is_ok() {}
		} else {
			agent.store.cancel_agent_capacity_retry("worker".into(), retry.event_id).await.unwrap();
		}

		agent.wake_pending().await.unwrap();

		let starts: Vec<_> = iter::from_fn(|| sent.try_recv().ok())
			.filter(|frame| frame["method"] == "turn/start")
			.collect();

		assert_eq!(starts.len(), 1);
		assert_eq!(starts[0]["params"]["threadId"], "opaque thread/1");
		assert!(agent.store.pending_agent_capacity_retry("worker".into()).await.unwrap().is_none());
	}
}

#[tokio::test]
async fn capacity_retry_keeps_model_thread_and_context_and_stops_after_three_attempts() {
	let turns: Vec<_> =
		(1..=4).map(|n| failed(&format!("opaque turn/{n}"), "serverOverloaded")).collect();
	let (mut agent, mut sent, _dir) = tests::fixture_with_history(
		serde_json::json!({"_started_turns_only":true,"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":turns}}}),
	)
	.await;

	agent.start_agent("agent", "original request").await.unwrap();

	while sent.try_recv().is_ok() {}

	for n in 1..=4 {
		agent.handle_event(ServerEvent::Notification { method:"turn/completed".into(),params:serde_json::json!({"threadId":"opaque thread/1","turn":failed(&format!("opaque turn/{n}"),"serverOverloaded")}) }).await.unwrap();

		if n == 4 {
			break;
		}

		let retry =
			agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().unwrap();

		assert_eq!(retry.attempt, n);

		while sent.try_recv().is_ok() {}

		agent.check_due_followups(retry.due_at_micros - 1).await.unwrap();

		assert!(sent.try_recv().is_err());

		if n == 1 {
			agent.loaded_threads.clear();
			agent.recover_persisted().await.unwrap();
		}

		agent.check_due_followups(retry.due_at_micros).await.unwrap();

		let mut starts = 0;

		while let Ok(request) = sent.try_recv() {
			if request["method"] == "turn/start" {
				starts += 1;

				assert_eq!(request["params"]["threadId"], "opaque thread/1");
				assert_eq!(request["params"]["model"], "selected-model");
				assert_eq!(request["params"]["effort"], "high");
				assert_eq!(request["params"]["input"], serde_json::json!([]));
				assert_eq!(request["params"]["toolOutput"]["name"], "capacity_retry");
				assert_eq!(request["params"]["turnTrigger"], "retry");
				assert_eq!(request["params"]["toolOutput"]["namespace"], "decodex");

				let text = request["params"]["toolOutput"]["output"].as_str().unwrap();

				assert!(text.contains("saved thread context"));
				assert!(!text.contains("original request"));
			}

			assert_ne!(request["method"], "thread/start");
		}

		assert_eq!(starts, 1);
	}

	assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());

	while sent.try_recv().is_ok() {}

	agent.check_due_followups(i64::MAX).await.unwrap();

	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn quota_other_errors_and_missing_history_do_not_schedule_capacity_retries() {
	for (code, history_present) in [
		("usageLimitExceeded", true),
		("rateLimitExceeded", true),
		("contextWindowExceeded", true),
		("other", true),
		("serverOverloaded", false),
	] {
		let turn = failed("opaque turn/1", code);
		let history = if history_present {
			serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[turn]}}})
		} else {
			serde_json::json!({})
		};
		let (mut agent, mut sent, _dir) = tests::fixture_with_history(history).await;

		agent.start_agent("agent", "request").await.unwrap();
		agent
			.handle_event(ServerEvent::Notification {
				method: "turn/completed".into(),
				params: serde_json::json!({"threadId":"opaque thread/1","turn":turn}),
			})
			.await
			.unwrap();

		assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());

		while sent.try_recv().is_ok() {}

		agent.check_due_followups(i64::MAX).await.unwrap();

		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn terminal_quota_failure_preserves_accepted_input_without_replay_after_recovery() {
	let turn = failed("opaque turn/1", "usageLimitExceeded");
	let (mut agent, mut sent, _dir) = tests::fixture_with_history(
		serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[turn]}}}),
	)
	.await;

	AgentCoordinator::reserve_root(&agent.store, "agent", "original request").await.unwrap();

	agent.enqueue_user_message("agent", "command", "original request").await.unwrap();
	agent.wake_pending().await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":turn}),
		})
		.await
		.unwrap();

	while sent.try_recv().is_ok() {}

	agent.loaded_threads.clear();
	agent.recover_persisted().await.unwrap();
	agent.wake_pending().await.unwrap();
	agent.check_due_followups(i64::MAX).await.unwrap();

	assert!(!iter::from_fn(|| sent.try_recv().ok()).any(|frame| frame["method"] == "turn/start"));

	let history = agent.store.read_agent_work_events("agent".into(), 100).await.unwrap();
	let inputs: Vec<_> =
		history.iter().filter(|event| event.event_kind == "user_message").collect();

	assert_eq!(inputs.len(), 1);
	assert_eq!(inputs[0].delivered_turn_id.as_deref(), Some("opaque turn/1"));
	assert_eq!(inputs[0].disposition, None);

	agent.enqueue_user_message("agent", "retry", "Try again now").await.unwrap();
	agent.wake_pending().await.unwrap();

	let starts: Vec<_> = iter::from_fn(|| sent.try_recv().ok())
		.filter(|frame| frame["method"] == "turn/start")
		.collect();

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["input"][0]["text"], "Try again now");
}

#[tokio::test]
async fn lost_retry_submission_is_unknown_and_never_replayed() {
	let turn = failed("opaque turn/1", "serverOverloaded");
	let (mut agent, _sent, _dir) = tests::fixture_with_history(
		serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[turn]}}}),
	)
	.await;

	agent.start_agent("agent", "request").await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":turn}),
		})
		.await
		.unwrap();

	let (io, remote) = io::duplex(8_192);
	let (reader, writer) = io::split(io);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let server = tokio::spawn(async move {
		let (reader, mut writer) = io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

		assert_eq!(request["method"], "thread/read");

		writer.write_all(format!("{}\n",serde_json::json!({"id":request["id"],"result":{"thread":{"id":"opaque thread/1","model":"selected-model","reasoningEffort":"high"}}})).as_bytes()).await.unwrap();

		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

		assert_eq!(request["method"], "turn/start");
		// The model request was written; discard its response, not the preceding settings read.
	});

	agent.client = client;

	assert!(agent.check_due_followups(i64::MAX).await.is_err());

	server.await.unwrap();

	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Unknown
	);

	agent.recover_persisted().await.unwrap();

	assert!(agent.store.due_agent_capacity_retries(i64::MAX).await.unwrap().is_empty());
}

#[tokio::test]
async fn refused_capacity_continuation_retains_original_delivery_and_cannot_replay() {
	for (managed, message) in [
		(false, "Server is draining; retry after reconnecting"),
		(
			true,
			"failed to load configuration: Your organization's required model provider settings changed. Restart Codex to apply them; this request was not sent",
		),
	] {
		let first = failed("opaque turn/1", "serverOverloaded");
		let (mut agent,mut sent,directory) = tests::fixture_with_history(serde_json::json!({"_capacity_draining":true,"_refusal_message":message,"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first]}}})).await;

		AgentCoordinator::reserve_root(&agent.store, "agent", "original request").await.unwrap();

		agent.enqueue_user_message("agent", "command", "original request").await.unwrap();
		agent.wake_pending().await.unwrap();
		agent
			.handle_event(ServerEvent::Notification {
				method: "turn/completed".into(),
				params: serde_json::json!({"threadId":"opaque thread/1","turn":first}),
			})
			.await
			.unwrap();

		let retry =
			agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().unwrap();

		while sent.try_recv().is_ok() {}

		let result = agent.check_due_followups(retry.due_at_micros).await;

		assert!(
			if managed {
				matches!(
					&result,
					Err(AgentError::InputNotSent(
						decodex_database::AgentDispatchRefusal::ManagedProviderChanged
					))
				)
			} else {
				matches!(
					&result,
					Err(AgentError::InputNotSent(
						decodex_database::AgentDispatchRefusal::ServerDraining
					))
				)
			},
			"managed={managed}, result={result:?}"
		);

		let requests = iter::from_fn(|| sent.try_recv().ok()).collect::<Vec<_>>();

		assert_eq!(requests.iter().filter(|request| request["method"] == "turn/start").count(), 1);
		assert!(requests.iter().all(|request| request["method"] != "thread/inject_items"));

		agent.check_due_followups(i64::MAX).await.unwrap();

		assert!(
			iter::from_fn(|| sent.try_recv().ok()).all(|request| request["method"] != "turn/start")
		);

		drop(agent);

		let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		let work = store.get_agent_work_item("agent".into()).await.unwrap();

		assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Idle);
		assert_eq!(work.status, decodex_database::AgentWorkStatus::UserDecision);
		assert!(store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());
		assert!(
			store
				.begin_agent_capacity_retry("agent".into(), retry.event_id, i64::MAX)
				.await
				.is_err()
		);

		let history = store.read_agent_work_events("agent".into(), 100).await.unwrap();
		let input = history.iter().find(|event| event.event_kind == "user_message").unwrap();

		assert_eq!(input.delivered_turn_id.as_deref(), Some("opaque turn/1"));
		assert!(input.payload.contains("original request"));

		let refused =
			history.iter().find(|event| event.event_kind == "capacity_retry_rejected").unwrap();
		let payload: Value = serde_json::from_str(&refused.payload).unwrap();

		assert_eq!(
			payload["reason"],
			if managed { "managedProviderChanged" } else { "serverDraining" }
		);
	}
}

#[tokio::test]
async fn native_revert_cancels_only_unclaimed_capacity_intent_durably() {
	for claimed in [false, true] {
		let failure = failed("opaque turn/1", "serverOverloaded");
		let (mut agent, mut sent, directory) = tests::fixture_with_history(
			serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[failure]}}}),
		)
		.await;

		agent.start_agent("agent", "original request").await.unwrap();
		agent
			.handle_event(ServerEvent::Notification {
				method: "turn/completed".into(),
				params: serde_json::json!({"threadId":"opaque thread/1","turn":failure}),
			})
			.await
			.unwrap();

		let retry =
			agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().unwrap();

		agent
			.store
			.cancel_reverted_agent_capacity_retries(
				"opaque thread/1".into(),
				Some("foreign-generation".into()),
			)
			.await
			.unwrap();
		agent
			.handle_event(ServerEvent::Notification {
				method: "thread/reverted".into(),
				params: serde_json::json!({"threadId":"other"}),
			})
			.await
			.unwrap();

		assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_some());

		if claimed {
			agent
				.store
				.begin_agent_capacity_retry("agent".into(), retry.event_id, i64::MAX)
				.await
				.unwrap();
		}

		let before = agent.store.get_agent_inbox_event(retry.event_id).await.unwrap();
		let work_before = agent.store.get_agent_work_item("agent".into()).await.unwrap();

		while sent.try_recv().is_ok() {}

		for _ in 0..2 {
			agent
				.handle_event(ServerEvent::Notification {
					method: "thread/reverted".into(),
					params: serde_json::json!({"threadId":"opaque thread/1"}),
				})
				.await
				.unwrap();
		}

		agent.check_due_followups(i64::MAX).await.unwrap();

		assert!(iter::from_fn(|| sent.try_recv().ok()).all(|r| r["method"] != "turn/start"));

		drop(agent);

		let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();

		assert!(store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());
		assert!(
			store
				.begin_agent_capacity_retry("agent".into(), retry.event_id, i64::MAX)
				.await
				.is_err()
		);

		let after = store.get_agent_inbox_event(retry.event_id).await.unwrap();

		assert_eq!(after.payload, before.payload);
		assert_eq!(after.delivered_turn_id, before.delivered_turn_id);
		assert_eq!(
			store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
			work_before.dispatch_state
		);

		if claimed {
			assert_eq!(after.disposition_note, before.disposition_note);
		} else {
			assert_eq!(after.disposition, Some(AgentDisposition::Resolved));
			assert_eq!(
				after.disposition_note.as_deref(),
				Some("Automatic retry cancelled because native history was reverted.")
			);
		}
	}
}

#[tokio::test]
async fn reverted_worker_capacity_wait_does_not_publish_a_completion_or_wake_agent() {
	let first = serde_json::json!({"id":"opaque turn/1","status":"completed","items":[]});
	let failure = failed("opaque turn/2", "serverOverloaded");
	let (mut agent, mut sent, _dir) = tests::fixture_with_history(serde_json::json!({
		"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first]}},
		"opaque thread/2":{"thread":{"id":"opaque thread/2","turns":[failure]}}
	}))
	.await;

	agent.start_agent("agent", "coordinate").await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":first}),
		})
		.await
		.unwrap();
	agent.create_worker("agent", "worker", "work").await.unwrap();

	while sent.try_recv().is_ok() {}

	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/2","turn":failure}),
		})
		.await
		.unwrap();

	assert!(!iter::from_fn(|| sent.try_recv().ok()).any(|frame| frame["method"] == "turn/start"));

	let retry = agent.store.pending_agent_capacity_retry("worker".into()).await.unwrap().unwrap();

	agent
		.handle_event(ServerEvent::Notification {
			method: "thread/reverted".into(),
			params: serde_json::json!({"threadId":"opaque thread/2"}),
		})
		.await
		.unwrap();
	agent.check_due_followups(i64::MAX).await.unwrap();
	agent.wake_pending().await.unwrap();

	let starts: Vec<_> = iter::from_fn(|| sent.try_recv().ok())
		.filter(|frame| frame["method"] == "turn/start")
		.collect();

	assert!(starts.is_empty());
	assert!(
		agent
			.store
			.begin_agent_capacity_retry("worker".into(), retry.event_id, i64::MAX)
			.await
			.is_err()
	);
	assert!(
		agent
			.store
			.list_pending_agent_events(100)
			.await
			.unwrap()
			.iter()
			.all(|e| e.event_kind != "worker_turn_completed")
	);
	assert!(agent.store.pending_agent_capacity_retry("worker".into()).await.unwrap().is_none());
}

#[tokio::test]
async fn task_selection_during_capacity_backoff_cancels_old_retry() {
	let first = failed("opaque turn/1", "serverOverloaded");
	let (mut agent, mut sent, _dir) = tests::fixture_with_history(
		serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first]}}}),
	)
	.await;

	agent.start_agent("agent", "request").await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":first}),
		})
		.await
		.unwrap();
	agent
		.client
		.request(
			"thread/settings/update",
			serde_json::json!({"threadId":"opaque thread/1","model":"new-user-choice","effort":"medium"}),
		)
		.await
		.unwrap();

	while sent.try_recv().is_ok() {}

	agent.check_due_followups(i64::MAX).await.unwrap();

	assert!(!iter::from_fn(|| sent.try_recv().ok()).any(|v| v["method"] == "turn/start"));
	assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);

	agent.check_due_followups(i64::MAX).await.unwrap();

	assert!(!iter::from_fn(|| sent.try_recv().ok()).any(|v| v["method"] == "turn/start"));
}

#[tokio::test]
async fn selected_turn_model_survives_restart_and_capacity_retry_without_startup_defaults() {
	let first = failed("opaque turn/1", "serverOverloaded");
	let (mut agent, mut sent, dir) = tests::fixture_with_history(
		serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first]}}}),
	)
	.await;

	AgentCoordinator::reserve_root(&agent.store, "agent", "request").await.unwrap();

	agent.store.enqueue_agent_event(EnqueueAgentEvent {
		source_event_id:"selected-input".into(),work_item_id:"agent".into(),event_kind:"user_message".into(),
		payload:serde_json::json!({"text":"request","options":{"execution":{"model":"user-selected","reasoning_effort":"medium","fast":false},"attachments":[]}}).to_string(),
	}).await.unwrap();
	agent.wake_pending().await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":first}),
		})
		.await
		.unwrap();

	let paths = DecodexRoot::new(dir.path().canonicalize().unwrap().join("root")).unwrap().paths();
	let client = agent.client.clone();
	let mut config = agent.config.clone();

	config.model = "changed-process-default".into();

	drop(agent);

	let mut agent =
		AgentCoordinator::new(SqliteStore::open(&paths).unwrap(), client, config).unwrap();

	agent.loaded_threads.insert("opaque thread/1".into());

	while sent.try_recv().is_ok() {}

	agent.check_due_followups(i64::MAX).await.unwrap();

	let starts: Vec<_> =
		iter::from_fn(|| sent.try_recv().ok()).filter(|v| v["method"] == "turn/start").collect();

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["model"], "user-selected");
	assert_eq!(starts[0]["params"]["effort"], "medium");

	let selection = agent
		.store
		.agent_turn_execution("agent".into(), "opaque thread/1".into(), "opaque turn/2".into())
		.await
		.unwrap()
		.unwrap();

	assert_eq!(selection.model, "user-selected");
	assert_eq!(selection.effort.as_deref(), Some("medium"));
}

#[tokio::test]
async fn capacity_selection_changes_do_not_revive_a_cancelled_retry_when_changed_back() {
	let first = failed("opaque turn/1", "serverOverloaded");
	let (mut agent, mut sent, _dir) = tests::fixture_with_history(
		serde_json::json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first]}}}),
	)
	.await;

	agent.start_agent("agent", "request").await.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: serde_json::json!({"threadId":"opaque thread/1","turn":first}),
		})
		.await
		.unwrap();

	for model in ["new-choice", "selected-model"] {
		agent.handle_event(ServerEvent::Notification {method:"thread/settings/updated".into(),params:serde_json::json!({"threadId":"opaque thread/1","threadSettings":{"model":model,"modelProvider":"openai","effort":"high","serviceTier":null}})}).await.unwrap();
	}

	while sent.try_recv().is_ok() {}

	agent.check_due_followups(i64::MAX).await.unwrap();

	assert!(!iter::from_fn(|| sent.try_recv().ok()).any(|v| v["method"] == "turn/start"));
	assert!(agent.store.pending_agent_capacity_retry("agent".into()).await.unwrap().is_none());
}

#[tokio::test]
async fn ordinary_continuation_binds_current_task_choice_instead_of_initial_defaults() {
	let (mut agent, mut sent, _dir) = tests::fixture().await;

	agent.start_agent("agent", "request").await.unwrap();

	tests::complete(&mut agent, "agent").await;

	agent
		.client
		.request(
			"thread/settings/update",
			serde_json::json!({"threadId":"opaque thread/1","model":"recovered-task-model","effort":"medium"}),
		)
		.await
		.unwrap();

	while sent.try_recv().is_ok() {}

	agent.continue_worker("agent", "next user request").await.unwrap();

	let starts: Vec<_> =
		iter::from_fn(|| sent.try_recv().ok()).filter(|v| v["method"] == "turn/start").collect();

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["model"], "recovered-task-model");
	assert_eq!(starts[0]["params"]["effort"], "medium");
	assert_eq!(
		agent
			.store
			.agent_turn_execution("agent".into(), "opaque thread/1".into(), "opaque turn/2".into())
			.await
			.unwrap()
			.unwrap()
			.model,
		"recovered-task-model"
	);
}

#[tokio::test]
async fn effort_only_input_preserves_native_model_and_tier_after_recovery() {
	let (mut agent, mut sent, _dir) = tests::fixture().await;

	agent.start_agent("agent", "initial").await.unwrap();

	tests::complete(&mut agent, "agent").await;

	agent
		.client
		.request(
			"thread/settings/update",
			serde_json::json!({"threadId":"opaque thread/1","model":"recovered-native","effort":"high"}),
		)
		.await
		.unwrap();
	agent.store.enqueue_agent_event(EnqueueAgentEvent {source_event_id:"effort-only".into(),work_item_id:"agent".into(),event_kind:"user_message".into(),payload:serde_json::json!({"text":"continue","options":{"execution":{"reasoning_effort":"medium"},"attachments":[]}}).to_string()}).await.unwrap();

	while sent.try_recv().is_ok() {}

	agent.wake_pending().await.unwrap();

	let starts: Vec<_> =
		iter::from_fn(|| sent.try_recv().ok()).filter(|v| v["method"] == "turn/start").collect();

	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["model"], "recovered-native");
	assert_eq!(starts[0]["params"]["effort"], "medium");
	assert!(starts[0]["params"].get("serviceTier").is_none());
	assert!(starts[0]["params"].get("serviceTierForTurn").is_none());
}
