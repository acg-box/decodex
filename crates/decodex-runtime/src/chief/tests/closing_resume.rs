use super::*;

#[tokio::test]
async fn deferred_closing_recovery_discards_changed_history_work_and_archives() {
	for change in ["history", "turn", "state", "archive"] {
		let (mut chief, mut sent, _directory) = fixture().await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		chief.store.mark_chief_dispatch_unknown("chief".into()).await.unwrap();
		chief.closing_resumes.insert(
			"chief".into(),
			super::super::resume_recovery::ClosingResume {
				thread: "opaque thread/1".into(),
				turn: if change == "turn" { "old-turn" } else { "opaque turn/1" }.into(),
				revision: chief.client.history_revision() + u64::from(change == "history"),
				next: tokio::time::Instant::now(),
			},
		);
		if change == "state" {
			chief
				.store
				.reconcile_chief_dispatch("chief".into(), "opaque turn/1".into())
				.await
				.unwrap();
		}
		if change == "archive" {
			chief
				.handle_event(ServerEvent::Notification {
					method: "thread/archived".into(),
					params: json!({"threadId":"opaque thread/1"}),
				})
				.await
				.unwrap();
		}
		while sent.try_recv().is_ok() {}
		chief.recover_closing_threads().await.unwrap();
		assert!(chief.closing_resumes.is_empty());
		assert!(sent.try_recv().is_err(), "{change} must revoke a deferred resume");
	}
}

#[tokio::test]
async fn ordinary_resume_refusal_does_not_enter_deferred_recovery() {
	let (mut chief, mut sent, _directory) =
		fixture_with_history(json!({"_resume_failures":1})).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	chief.recover_persisted().await.unwrap();
	assert!(chief.closing_resumes.is_empty());
	chief.check_due_followups(0).await.unwrap();
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
	let history = json!({"_resume_failures":5,"_resume_closing":true,
		"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"idle"},
			"turns":[{"id":"opaque turn/1","status":"completed","items":[]}]}}});
	let (mut chief, mut sent, _directory) = fixture_with_history(history).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	chief.recover_persisted().await.unwrap();
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Unknown
	);
	assert_eq!(chief.closing_resumes.len(), 1);
	chief.create_goal("chief", "goal", "Goal").await.unwrap();
	chief.create_worker("goal", "worker", "Inspect").await.unwrap();
	while sent.try_recv().is_ok() {}
	chief.check_due_followups(0).await.unwrap();
	assert!(sent.try_recv().is_err(), "backoff must not be bypassed by regular ticks");
	chief.closing_resumes.get_mut("chief").unwrap().next = tokio::time::Instant::now();
	chief.pause_dispatch(true);
	chief.check_due_followups(0).await.unwrap();
	assert!(sent.try_recv().is_err(), "account suspension prevents resume activation");
	chief.pause_dispatch(false);
	chief.check_due_followups(0).await.unwrap();
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Idle
	);
	assert_eq!(
		chief.store.get_chief_work_item("worker".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Running
	);
	assert!(chief.closing_resumes.is_empty());
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
		let (mut original, mut sent, _directory) = fixture_with_history(history).await;
		original.start_chief("chief", "Coordinate").await.unwrap();
		while sent.try_recv().is_ok() {}
		let mut recovered = ChiefCoordinator::new(
			original.store.clone(),
			original.client.clone(),
			original.config.clone(),
		)
		.unwrap();
		drop(original);
		recovered.recover_persisted().await.unwrap();
		let work = recovered.store.get_chief_work_item("chief".into()).await.unwrap();
		assert_eq!(work.codex_thread_id.as_deref(), Some("opaque thread/1"));
		assert_eq!(
			work.dispatch_state,
			if terminal {
				decodex_database::ChiefDispatchState::Idle
			} else {
				decodex_database::ChiefDispatchState::Running
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
	}
}
