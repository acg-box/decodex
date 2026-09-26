use super::{fixture, fixture_with_history};
use crate::agent::{AgentCoordinator, ServerEvent};
use serde_json::json;

#[tokio::test]
async fn deferred_closing_recovery_discards_changed_history_work_and_archives() {
	for change in
		["history", "turn", "state", "thread/archived", "thread/deleted", "thread/reverted"]
	{
		let (mut agent, mut sent, _directory) = fixture().await;
		agent.start_agent("agent", "Coordinate").await.unwrap();
		agent.store.mark_agent_dispatch_unknown("agent".into()).await.unwrap();
		agent.closing_resumes.insert(
			"agent".into(),
			super::super::resume_recovery::ClosingResume {
				thread: "opaque thread/1".into(),
				turn: if change == "turn" { "old-turn" } else { "opaque turn/1" }.into(),
				revision: agent.client.history_revision() + u64::from(change == "history"),
				next: tokio::time::Instant::now(),
				attempts: 1,
			},
		);
		if change == "state" {
			agent
				.store
				.reconcile_agent_dispatch("agent".into(), "opaque turn/1".into())
				.await
				.unwrap();
		}
		if change.starts_with("thread/") {
			agent
				.handle_event(ServerEvent::Notification {
					method: change.into(),
					params: json!({"threadId":"opaque thread/1"}),
				})
				.await
				.unwrap();
		}
		while sent.try_recv().is_ok() {}
		agent.recover_closing_threads().await.unwrap();
		assert!(agent.closing_resumes.is_empty());
		assert!(sent.try_recv().is_err(), "{change} must revoke a deferred resume");
	}
}

#[tokio::test]
async fn ordinary_resume_refusal_does_not_enter_deferred_recovery() {
	let (mut agent, mut sent, _directory) =
		fixture_with_history(json!({"_resume_failures":1})).await;
	agent.start_agent("agent", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	agent.recover_persisted().await.unwrap();
	assert!(agent.closing_resumes.is_empty());
	agent.check_due_followups(0).await.unwrap();
	let mut resumes = 0;
	while let Ok(request) = sent.try_recv() {
		if request["method"] == "thread/resume" {
			resumes += 1;
		}
	}
	assert_eq!(resumes, 1);
}

#[tokio::test]
async fn exhausted_closing_recovery_rechecks_only_its_original_work_on_due_tick() {
	let history = json!({"_resume_failures":1,"_resume_closing":true,
		"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"idle"},
			"turns":[{"id":"opaque turn/1","status":"completed","items":[]}]}}});
	let (mut agent, mut sent, _directory) = fixture_with_history(history).await;
	agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.recover_persisted().await.unwrap();
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Unknown
	);
	assert_eq!(agent.closing_resumes.len(), 1);
	agent.create_goal("agent", "goal", "Goal").await.unwrap();
	agent.create_worker("goal", "worker", "Inspect").await.unwrap();
	while sent.try_recv().is_ok() {}
	agent.check_due_followups(0).await.unwrap();
	assert!(sent.try_recv().is_err(), "backoff must not be bypassed by regular ticks");
	agent.closing_resumes.get_mut("agent").unwrap().next = tokio::time::Instant::now();
	agent.pause_dispatch(true);
	agent.check_due_followups(0).await.unwrap();
	assert!(sent.try_recv().is_err(), "account suspension prevents resume activation");
	agent.pause_dispatch(false);
	agent.check_due_followups(0).await.unwrap();
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);
	assert_eq!(
		agent.store.get_agent_work_item("worker".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Running
	);
	assert!(agent.closing_resumes.is_empty());
	let mut resumes = 0;
	while let Ok(request) = sent.try_recv() {
		assert!(["thread/resume", "thread/read"].contains(&request["method"].as_str().unwrap()));
		assert_eq!(request["params"]["threadId"], "opaque thread/1");
		if request["method"] == "thread/resume" {
			resumes += 1;
		}
	}
	assert_eq!(resumes, 1);
}

#[tokio::test]
async fn recovery_retries_closing_thread_and_reconciles_without_replaying_input() {
	for terminal in [false, true] {
		let history = json!({
			"_resume_failures":1,"_resume_closing":true,
			"opaque thread/1":{"thread":{"id":"opaque thread/1",
				"status":{"type":if terminal {"idle"} else {"active"}},
				"turns":[{"id":"opaque turn/1",
					"status":if terminal {"completed"} else {"inProgress"},
					"items":[]}]}}
		});
		let (mut original, mut sent, directory) = fixture_with_history(history).await;
		original.start_agent("agent", "Coordinate").await.unwrap();
		while sent.try_recv().is_ok() {}
		let root =
			decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
				.unwrap();
		let reopened = decodex_database::SqliteStore::open(&root.paths()).unwrap();
		let mut recovered = AgentCoordinator::new(reopened, original.client.clone(), {
			let mut config = original.config.clone();
			config.model = "new-default-model".into();
			config.agent_effort = Some("low".into());
			config
		})
		.unwrap();
		drop(original);
		recovered.recover_persisted().await.unwrap();
		assert_eq!(recovered.closing_resumes.len(), 1);
		recovered.closing_resumes.get_mut("agent").unwrap().next = tokio::time::Instant::now();
		recovered.check_due_followups(0).await.unwrap();
		let work = recovered.store.get_agent_work_item("agent".into()).await.unwrap();
		assert_eq!(work.codex_thread_id.as_deref(), Some("opaque thread/1"));
		assert_eq!(
			work.dispatch_state,
			if terminal {
				decodex_database::AgentDispatchState::Idle
			} else {
				decodex_database::AgentDispatchState::Running
			}
		);
		let mut resumes = Vec::new();
		while let Ok(request) = sent.try_recv() {
			assert!(
				["thread/resume", "thread/read"].contains(&request["method"].as_str().unwrap())
			);
			if request["method"] == "thread/resume" {
				resumes.push(request["params"].clone());
			}
		}
		assert_eq!(resumes.len(), 2);
		assert_eq!(resumes[0], resumes[1]);
		assert_eq!(resumes[0]["threadId"], "opaque thread/1");
		assert_eq!(resumes[0]["excludeTurns"], true);
		assert_eq!(
			resumes[0],
			json!({"threadId":"opaque thread/1","excludeTurns":true,"experimentalRawEvents":true})
		);
	}
}

#[tokio::test]
async fn continued_closing_refusals_back_off_without_blocking_or_replaying() {
	let (mut agent, mut sent, _directory) =
		fixture_with_history(json!({"_resume_failures":6,"_resume_closing":true})).await;
	agent.start_agent("agent", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	agent.recover_persisted().await.unwrap();
	for delay in [1, 2, 4, 8, 60, 60] {
		let next = agent.closing_resumes["agent"].next;
		let remaining = next.saturating_duration_since(tokio::time::Instant::now());
		assert!(remaining <= std::time::Duration::from_secs(delay));
		assert!(remaining > std::time::Duration::from_millis(delay * 1000 - 500));
		agent.check_due_followups(0).await.unwrap();
		assert_eq!(agent.closing_resumes["agent"].next, next);
		if agent.closing_resumes["agent"].attempts < 6 {
			agent.closing_resumes.get_mut("agent").unwrap().next = tokio::time::Instant::now();
			agent.check_due_followups(0).await.unwrap();
		}
	}
	let mut count = 0;
	while let Ok(request) = sent.try_recv() {
		assert!(["thread/resume", "thread/read"].contains(&request["method"].as_str().unwrap()));
		assert_eq!(request["params"]["threadId"], "opaque thread/1");
		if request["method"] == "thread/resume" {
			count += 1;
		}
	}
	assert_eq!(count, 6);
}

#[tokio::test]
async fn closing_retry_requires_both_the_exact_thread_and_native_error_code() {
	for (code, message) in [
		(-32600, "thread other is closing; retry"),
		(-32603, "thread opaque thread/1 is closing; retry"),
		(-32600, "thread opaque thread/1-extra is closing; retry"),
	] {
		let (mut agent, mut sent, _directory) = fixture_with_history(json!({
			"_resume_failures":1,"_resume_error":{"code":code,"message":message}
		}))
		.await;
		agent.start_agent("agent", "Coordinate").await.unwrap();
		while sent.try_recv().is_ok() {}
		agent.recover_persisted().await.unwrap();
		assert!(agent.closing_resumes.is_empty());
		agent.check_due_followups(0).await.unwrap();
		let mut resumes = 0;
		while let Ok(request) = sent.try_recv() {
			assert!(
				["thread/resume", "thread/read"].contains(&request["method"].as_str().unwrap())
			);
			if request["method"] == "thread/resume" {
				resumes += 1;
			}
		}
		assert_eq!(resumes, 1);
	}
}

#[tokio::test]
async fn closing_recovery_confirms_exact_steer_history_without_resubmitting_input() {
	for terminal in [false, true] {
		let history = json!({"_resume_failures":1,"_resume_closing":true,
			"opaque thread/1":{"thread":{"id":"opaque thread/1",
				"status":{"type":if terminal {"idle"} else {"active"}},
				"turns":[{"id":"opaque turn/1",
					"status":if terminal {"completed"} else {"inProgress"},
					"items":[{"type":"userMessage","id":"accepted-input","clientId":"confirmed",
						"content":[{"type":"text","text":"Identical input"}]}]}]}}});
		let (mut original, mut sent, directory) = fixture_with_history(history).await;
		original.start_agent("agent", "Coordinate").await.unwrap();
		for key in ["confirmed", "unconfirmed"] {
			original
				.store
				.begin_agent_steer(
					"agent".into(),
					"opaque turn/1".into(),
					key.into(),
					json!({"text":"Identical input","source":"user"}).to_string(),
				)
				.await
				.unwrap();
		}
		let paths =
			decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
				.unwrap()
				.paths();
		let mut recovered = AgentCoordinator::new(
			decodex_database::SqliteStore::open(&paths).unwrap(),
			original.client.clone(),
			original.config.clone(),
		)
		.unwrap();
		drop(original);
		while sent.try_recv().is_ok() {}
		recovered.recover_persisted().await.unwrap();
		assert_eq!(recovered.closing_resumes.len(), 1);
		recovered.closing_resumes.get_mut("agent").unwrap().next = tokio::time::Instant::now();
		recovered.check_due_followups(0).await.unwrap();
		assert!(recovered.closing_resumes.is_empty());
		let reopened = decodex_database::SqliteStore::open(&paths).unwrap();
		for (key, confirmed) in [("confirmed", true), ("unconfirmed", false)] {
			assert_eq!(
				reopened
					.agent_steer_confirmed(
						"agent".into(),
						"opaque thread/1".into(),
						"opaque turn/1".into(),
						key.into()
					)
					.await
					.unwrap(),
				confirmed
			);
		}
		let (events, _) = reopened.read_agent_transcript("agent".into(), None, 100).await.unwrap();
		assert_eq!(events.iter().filter(|event| event.event_kind == "user_message").count(), 1);
		let mut resumes = 0;
		while let Ok(request) = sent.try_recv() {
			assert!(
				["thread/resume", "thread/read", "thread/turns/list", "thread/items/list"]
					.contains(&request["method"].as_str().unwrap()),
				"unexpected request: {request}"
			);
			assert_eq!(request["params"]["threadId"], "opaque thread/1");
			resumes += usize::from(request["method"] == "thread/resume");
		}
		assert_eq!(resumes, 2);
	}
}
