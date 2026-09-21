use super::*;

fn failed(turn: &str, code: &str) -> Value {
	json!({"id":turn,"status":"failed","error":{"message":"Selected model is at capacity.","codexErrorInfo":code},"items":[{"id":"input","type":"userMessage","content":[{"type":"text","text":"original request"}]}]})
}

#[tokio::test]
async fn fresh_user_input_supersedes_a_due_capacity_retry() {
	let first = failed("opaque turn/1", "serverOverloaded");
	let (mut chief, mut sent, _dir) = fixture_with_history(
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first]}}}),
	)
	.await;
	chief.start_chief("chief", "original request").await.unwrap();
	chief
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":first}),
		})
		.await
		.unwrap();
	chief.enqueue_user_message("chief", "new-command", "changed request").await.unwrap();
	while sent.try_recv().is_ok() {}
	chief.check_due_followups(i64::MAX).await.unwrap();
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|frame| frame["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert!(starts[0]["params"]["input"][0]["text"].as_str().unwrap().contains("changed request"));
	assert!(chief.store.pending_chief_capacity_retry("chief".into()).await.unwrap().is_none());
}

#[tokio::test]
async fn successful_capacity_continuation_handles_the_original_input_once() {
	let first = failed("opaque turn/1", "serverOverloaded");
	let second = json!({"id":"opaque turn/2","status":"completed","items":[]});
	let (mut chief, mut sent, _dir) = fixture_with_history(
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first,second]}}}),
	)
	.await;
	ChiefCoordinator::reserve_root(&chief.store, "chief", "original request").await.unwrap();
	chief.enqueue_user_message("chief", "command", "original request").await.unwrap();
	chief.wake_pending().await.unwrap();
	chief
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":first}),
		})
		.await
		.unwrap();
	let retry = chief.store.pending_chief_capacity_retry("chief".into()).await.unwrap().unwrap();
	while sent.try_recv().is_ok() {}
	chief.check_due_followups(retry.due_at_micros).await.unwrap();
	chief
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":second}),
		})
		.await
		.unwrap();
	assert!(chief.store.list_pending_chief_events(100).await.unwrap().is_empty());
	let history = chief.store.read_chief_work_events("chief".into(), 100).await.unwrap();
	let inputs: Vec<_> =
		history.iter().filter(|event| event.event_kind == "user_message").collect();
	assert_eq!(inputs.len(), 1);
	assert_eq!(inputs[0].delivered_turn_id.as_deref(), Some("opaque turn/2"));
	assert_eq!(inputs[0].disposition, Some(ChiefDisposition::Resolved));
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|frame| frame["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert!(
		!starts[0]["params"]["toolOutput"]["output"].as_str().unwrap().contains("original request")
	);
}

#[tokio::test]
async fn worker_capacity_wait_does_not_wake_chief_and_cancel_publishes_failure() {
	let first = json!({"id":"opaque turn/1","status":"completed","items":[]});
	let failure = failed("opaque turn/2", "serverOverloaded");
	let (mut chief, mut sent, _dir) = fixture_with_history(json!({
		"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[first]}},
		"opaque thread/2":{"thread":{"id":"opaque thread/2","turns":[failure]}}
	}))
	.await;
	chief.start_chief("chief", "coordinate").await.unwrap();
	chief
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":first}),
		})
		.await
		.unwrap();
	chief.create_worker("chief", "worker", "work").await.unwrap();
	while sent.try_recv().is_ok() {}
	chief
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/2","turn":failure}),
		})
		.await
		.unwrap();
	assert!(
		!std::iter::from_fn(|| sent.try_recv().ok()).any(|frame| frame["method"] == "turn/start")
	);
	let retry = chief.store.pending_chief_capacity_retry("worker".into()).await.unwrap().unwrap();
	chief.store.cancel_chief_capacity_retry("worker".into(), retry.event_id).await.unwrap();
	chief.wake_pending().await.unwrap();
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|frame| frame["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["threadId"], "opaque thread/1");
	assert!(chief.store.pending_chief_capacity_retry("worker".into()).await.unwrap().is_none());
}

#[tokio::test]
async fn capacity_retry_keeps_model_thread_and_context_and_stops_after_three_attempts() {
	let turns: Vec<_> =
		(1..=4).map(|n| failed(&format!("opaque turn/{n}"), "serverOverloaded")).collect();
	let (mut chief, mut sent, _dir) = fixture_with_history(
		json!({"_visible_turns_only":true,"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":turns}}}),
	)
	.await;
	chief.start_chief("chief", "original request").await.unwrap();
	while sent.try_recv().is_ok() {}
	for n in 1..=4 {
		chief.handle_event(ServerEvent::Notification { method:"turn/completed".into(),params:json!({"threadId":"opaque thread/1","turn":failed(&format!("opaque turn/{n}"),"serverOverloaded")}) }).await.unwrap();
		if n == 4 {
			break;
		}
		let retry =
			chief.store.pending_chief_capacity_retry("chief".into()).await.unwrap().unwrap();
		assert_eq!(retry.attempt, n);
		while sent.try_recv().is_ok() {}
		chief.check_due_followups(retry.due_at_micros - 1).await.unwrap();
		assert!(sent.try_recv().is_err());
		if n == 1 {
			chief.loaded_threads.clear();
			chief.recover_persisted().await.unwrap();
		}
		chief.check_due_followups(retry.due_at_micros).await.unwrap();
		let mut starts = 0;
		while let Ok(request) = sent.try_recv() {
			if request["method"] == "turn/start" {
				starts += 1;
				assert_eq!(request["params"]["threadId"], "opaque thread/1");
				assert_eq!(request["params"]["model"], "selected-model");
				assert_eq!(request["params"]["effort"], "high");
				assert_eq!(request["params"]["input"], json!([]));
				assert_eq!(request["params"]["toolOutput"]["name"], "capacity_retry");
				assert_eq!(request["params"]["toolOutput"]["namespace"], "decodex");
				let text = request["params"]["toolOutput"]["output"].as_str().unwrap();
				assert!(text.contains("saved thread context"));
				assert!(!text.contains("original request"));
			}
			assert_ne!(request["method"], "thread/start");
		}
		assert_eq!(starts, 1);
	}
	assert!(chief.store.pending_chief_capacity_retry("chief".into()).await.unwrap().is_none());
	while sent.try_recv().is_ok() {}
	chief.check_due_followups(i64::MAX).await.unwrap();
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn quota_other_errors_and_missing_history_do_not_schedule_capacity_retries() {
	for (code, history_present) in [
		("usageLimitExceeded", true),
		("rateLimitExceeded", true),
		("other", true),
		("serverOverloaded", false),
	] {
		let turn = failed("opaque turn/1", code);
		let history = if history_present {
			json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[turn]}}})
		} else {
			json!({})
		};
		let (mut chief, mut sent, _dir) = fixture_with_history(history).await;
		chief.start_chief("chief", "request").await.unwrap();
		chief
			.handle_event(ServerEvent::Notification {
				method: "turn/completed".into(),
				params: json!({"threadId":"opaque thread/1","turn":turn}),
			})
			.await
			.unwrap();
		assert!(chief.store.pending_chief_capacity_retry("chief".into()).await.unwrap().is_none());
		while sent.try_recv().is_ok() {}
		chief.check_due_followups(i64::MAX).await.unwrap();
		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn terminal_quota_failure_preserves_accepted_input_without_replay_after_recovery() {
	let turn = failed("opaque turn/1", "usageLimitExceeded");
	let (mut chief, mut sent, _dir) = fixture_with_history(
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[turn]}}}),
	)
	.await;
	ChiefCoordinator::reserve_root(&chief.store, "chief", "original request").await.unwrap();
	chief.enqueue_user_message("chief", "command", "original request").await.unwrap();
	chief.wake_pending().await.unwrap();
	chief
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":turn}),
		})
		.await
		.unwrap();
	while sent.try_recv().is_ok() {}
	chief.loaded_threads.clear();
	chief.recover_persisted().await.unwrap();
	chief.wake_pending().await.unwrap();
	chief.check_due_followups(i64::MAX).await.unwrap();
	assert!(
		!std::iter::from_fn(|| sent.try_recv().ok()).any(|frame| frame["method"] == "turn/start")
	);
	let history = chief.store.read_chief_work_events("chief".into(), 100).await.unwrap();
	let inputs: Vec<_> =
		history.iter().filter(|event| event.event_kind == "user_message").collect();
	assert_eq!(inputs.len(), 1);
	assert_eq!(inputs[0].delivered_turn_id.as_deref(), Some("opaque turn/1"));
	assert_eq!(inputs[0].disposition, None);
	chief.enqueue_user_message("chief", "retry", "Try again now").await.unwrap();
	chief.wake_pending().await.unwrap();
	let starts: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok())
		.filter(|frame| frame["method"] == "turn/start")
		.collect();
	assert_eq!(starts.len(), 1);
	assert_eq!(starts[0]["params"]["input"][0]["text"], "Try again now");
}

#[tokio::test]
async fn lost_retry_submission_is_unknown_and_never_replayed() {
	let turn = failed("opaque turn/1", "serverOverloaded");
	let (mut chief, _sent, _dir) = fixture_with_history(
		json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[turn]}}}),
	)
	.await;
	chief.start_chief("chief", "request").await.unwrap();
	chief
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1","turn":turn}),
		})
		.await
		.unwrap();
	let (io, remote) = tokio::io::duplex(1);
	drop(remote);
	let (reader, writer) = tokio::io::split(io);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	chief.client = client;
	assert!(chief.check_due_followups(i64::MAX).await.is_err());
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Unknown
	);
	chief.recover_persisted().await.unwrap();
	assert!(chief.store.due_chief_capacity_retries(i64::MAX).await.unwrap().is_empty());
}
