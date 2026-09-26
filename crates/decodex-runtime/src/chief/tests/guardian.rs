use super::*;

fn review(id: &str, status: &str) -> Value {
	let mut value = json!({"threadId":"opaque thread/1","turnId":"opaque turn/1",
		"reviewId":id,"targetItemId":null,"startedAtMs":100,
		"review":{"status":status,"riskLevel":null,"userAuthorization":null,"rationale":null},
		"action":{"type":"networkAccess","target":"https://example.test:443",
			"host":"example.test","protocol":"https","port":443}});
	if status != "inProgress" {
		value["completedAtMs"] = json!(101);
		value["decisionSource"] = json!("agent");
		value["review"]["rationale"] = json!("User did not request this host.");
	}
	value
}

async fn deliver(chief: &mut ChiefCoordinator, value: Value) {
	let method = if value["review"]["status"] == "inProgress" {
		"item/autoApprovalReview/started"
	} else {
		"item/autoApprovalReview/completed"
	};
	let (io, mut write) = tokio::io::duplex(8192);
	let (read, writer) = tokio::io::split(io);
	let (_client, mut events) = AppServerClient::from_io(read, writer);
	let wire = json!({"method":method,"params":value});
	write.write_all(format!("{wire}\n").as_bytes()).await.unwrap();
	let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
		.await
		.unwrap()
		.unwrap();
	chief.handle_event(event).await.unwrap();
}

#[tokio::test]
async fn guardian_observations_are_monotonic_bound_durable_and_do_not_wake_work() {
	let (mut chief, mut sent, directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	for (thread, turn) in [("foreign", "opaque turn/1"), ("opaque thread/1", "unknown")] {
		let mut value = review("ignored", "denied");
		value["threadId"] = json!(thread);
		value["turnId"] = json!(turn);
		deliver(&mut chief, value).await;
	}
	deliver(&mut chief, review("network", "inProgress")).await;
	deliver(&mut chief, review("network", "denied")).await;
	deliver(&mut chief, review("network", "denied")).await;
	deliver(&mut chief, review("network", "inProgress")).await;
	// A second lifecycle with the same target item must remain independent.
	for id in ["execve-a", "execve-b"] {
		let mut value = review(id, "denied");
		value["targetItemId"] = json!("parent-command");
		deliver(&mut chief, value).await;
	}
	let mut large = review("large-action", "denied");
	large["action"] = json!({"type":"command","source":"shell","command":"界".repeat(100_000) + " exact-required-suffix","cwd":"/tmp"});
	deliver(&mut chief, large.clone()).await;
	deliver(&mut chief, review("unresolved", "inProgress")).await;
	let rows = chief.store.read_chief_guardian_reviews("chief".into(), None, 100).await.unwrap();
	assert_eq!(rows.len(), 5);
	assert!(rows.iter().all(|r| !r.conflicted));
	assert_eq!(rows.iter().find(|r| r.review_id == "network").unwrap().status, "denied");
	let work = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Running);
	chief.handle_event(ServerEvent::Notification {method:"turn/completed".into(), params:json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}})}).await.unwrap();
	// Native completion may be processed after the owning turn's terminal event.
	deliver(&mut chief, review("late", "approved")).await;
	chief.wake_pending().await.unwrap();
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start", "observation must not start a turn");
		assert_ne!(request["method"], "thread/approveGuardianDeniedAction");
	}
	assert!(chief.store.list_chief_wake_events("chief".into(), 32).await.unwrap().is_empty());
	let connection = chief.connection_id.clone();
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	drop(chief);
	let store = SqliteStore::open(&root.paths()).unwrap();
	let first = store.read_chief_guardian_reviews("chief".into(), None, 2).await.unwrap();
	let rest =
		store.read_chief_guardian_reviews("chief".into(), Some(first[1].id), 100).await.unwrap();
	assert_eq!(first.len() + rest.len(), 6);
	let retained =
		rest.iter().find(|row| row.review_id == "large-action").expect("large Guardian action");
	assert_eq!(
		serde_json::from_str::<Value>(&retained.event_json).expect("saved native event"),
		large
	);
	assert!(retained.approval_state.is_none());
	assert_eq!(first[1].status, "inProgress");
	assert_eq!(first[1].connection_id, connection);
	assert_eq!(first[0].status, "approved");
	assert_eq!(
		store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Idle
	);
}

#[tokio::test]
async fn conflicting_guardian_evidence_retains_original_and_invalidates_approval_identity() {
	let (mut chief, _sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let original = review("network", "denied");
	deliver(&mut chief, original.clone()).await;
	let mut changed = original.clone();
	changed["action"]["host"] = json!("different.test");
	deliver(&mut chief, changed).await;
	deliver(&mut chief, original.clone()).await;
	let rows = chief.store.read_chief_guardian_reviews("chief".into(), None, 100).await.unwrap();
	assert_eq!(rows.len(), 1);
	assert!(rows[0].conflicted);
	assert_eq!(serde_json::from_str::<Value>(&rows[0].event_json).unwrap(), original);
	// A second terminal status must never turn the previous denial into approval.
	deliver(&mut chief, review("another", "denied")).await;
	deliver(&mut chief, review("another", "approved")).await;
	let rows = chief.store.read_chief_guardian_reviews("chief".into(), None, 100).await.unwrap();
	assert_eq!(rows[0].status, "denied");
	assert!(rows[0].conflicted);
}

#[tokio::test]
async fn guardian_review_from_unbound_native_generation_is_not_retained() {
	let (mut chief, _sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	chief.bind_native_generation(
		decodex_core::ProcessGenerationId::new("30000000-0000-4000-8000-000000000001").unwrap(),
	);
	deliver(&mut chief, review("wrong-process", "denied")).await;
	assert!(
		chief
			.store
			.read_chief_guardian_reviews("chief".into(), None, 100)
			.await
			.unwrap()
			.is_empty()
	);
}

fn approval_history() -> Value {
	json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"active"},"turns":[{"id":"opaque turn/1","status":"inProgress","items":[]}]}}})
}

#[tokio::test]
async fn guardian_user_approval_submits_exact_denial_once_without_executing_a_turn() {
	let (mut chief, mut sent, _directory) = fixture_with_history(approval_history()).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	deliver(&mut chief, review("network", "denied")).await;
	let saved =
		chief.store.read_chief_guardian_reviews("chief".into(), None, 1).await.unwrap().remove(0);
	while sent.try_recv().is_ok() {}
	assert!(
		chief.approve_guardian_denial("chief", saved.id, "stale", "stale-click").await.is_err()
	);
	assert!(sent.try_recv().is_err());
	chief.approve_guardian_denial("chief", saved.id, &saved.digest(), "user-click").await.unwrap();
	assert!(
		chief
			.approve_guardian_denial("chief", saved.id, &saved.digest(), "second-click")
			.await
			.is_err()
	);
	let mut approvals = Vec::new();
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
		if request["method"] == "thread/approveGuardianDeniedAction" {
			approvals.push(request);
		}
	}
	assert_eq!(approvals.len(), 1);
	assert_eq!(approvals[0]["params"]["threadId"], "opaque thread/1");
	assert_eq!(approvals[0]["params"]["event"]["status"], "denied");
	assert_eq!(approvals[0]["params"]["event"]["action"]["type"], "network_access");
	let stored =
		chief.store.chief_guardian_review("chief".into(), saved.id).await.unwrap().unwrap();
	assert_eq!(stored.approval_state.as_deref(), Some("submitted"));
	assert_eq!(stored.status, "denied");
	assert_eq!(stored.event_json, saved.event_json);
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().active_turn_id.as_deref(),
		Some("opaque turn/1")
	);
}

#[tokio::test]
async fn guardian_rejection_and_lost_reply_have_distinct_durable_outcomes() {
	for (mode, expected) in [("_guardian_reject", "rejected"), ("_guardian_disconnect", "pending")]
	{
		let mut history = approval_history();
		history[mode] = json!(true);
		let (mut chief, mut sent, directory) = fixture_with_history(history).await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		deliver(&mut chief, review("network", "denied")).await;
		let saved = chief
			.store
			.read_chief_guardian_reviews("chief".into(), None, 1)
			.await
			.unwrap()
			.remove(0);
		while sent.try_recv().is_ok() {}
		let outcome =
			chief.approve_guardian_denial("chief", saved.id, &saved.digest(), "user-click").await;
		if expected == "pending" {
			assert!(matches!(outcome, Err(ChiefError::UnknownDispatch)));
		} else {
			assert!(matches!(outcome, Err(ChiefError::Rejected(_))));
		}
		let mut calls = 0;
		while let Ok(request) = sent.try_recv() {
			assert_ne!(request["method"], "turn/start");
			calls += usize::from(request["method"] == "thread/approveGuardianDeniedAction");
		}
		assert_eq!(calls, 1);
		let root =
			decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
				.unwrap();
		drop(chief);
		let store = SqliteStore::open(&root.paths()).unwrap();
		let restored =
			store.chief_guardian_review("chief".into(), saved.id).await.unwrap().unwrap();
		assert_eq!(restored.approval_state.as_deref(), Some(expected));
		assert!(
			store
				.begin_chief_guardian_approval(
					"chief".into(),
					saved.id,
					saved.digest(),
					saved.connection_id.clone(),
					None,
					"user-click".into()
				)
				.await
				.is_err()
		);
		let next = store
			.begin_chief_guardian_approval(
				"chief".into(),
				saved.id,
				saved.digest(),
				saved.connection_id.clone(),
				None,
				"new-explicit-click".into(),
			)
			.await;
		assert_eq!(next.is_ok(), expected == "rejected");
	}
}

#[tokio::test]
async fn guardian_approval_rejects_superseded_turn_and_changed_action_before_rpc() {
	for changed_action in [false, true] {
		let mut history = approval_history();
		if !changed_action {
			history["opaque thread/1"]["thread"]["turns"][0]["id"] = json!("newer-turn");
		}
		let (mut chief, mut sent, _directory) = fixture_with_history(history).await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		deliver(&mut chief, review("network", "denied")).await;
		let saved = chief
			.store
			.read_chief_guardian_reviews("chief".into(), None, 1)
			.await
			.unwrap()
			.remove(0);
		if changed_action {
			let mut changed = review("network", "denied");
			changed["action"]["host"] = json!("different.test");
			deliver(&mut chief, changed).await;
		}
		while sent.try_recv().is_ok() {}
		assert!(matches!(
			chief.approve_guardian_denial("chief", saved.id, &saved.digest(), "user-click").await,
			Err(ChiefError::Rejected(_))
		));
		while let Ok(request) = sent.try_recv() {
			assert_ne!(request["method"], "thread/approveGuardianDeniedAction");
		}
		assert_eq!(
			chief
				.store
				.chief_guardian_review("chief".into(), saved.id)
				.await
				.unwrap()
				.unwrap()
				.approval_state,
			None
		);
	}
}

#[tokio::test]
async fn concurrent_guardian_clicks_reserve_only_one_durable_submission() {
	let (mut chief, _sent, _directory) = fixture_with_history(approval_history()).await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	deliver(&mut chief, review("network", "denied")).await;
	let saved =
		chief.store.read_chief_guardian_reviews("chief".into(), None, 1).await.unwrap().remove(0);
	let claim = |key: &str| {
		chief.store.begin_chief_guardian_approval(
			"chief".into(),
			saved.id,
			saved.digest(),
			chief.connection_id.clone(),
			None,
			key.into(),
		)
	};
	let (first, second) = tokio::join!(claim("first-click"), claim("second-click"));
	assert_ne!(first.is_ok(), second.is_ok());
	let stored =
		chief.store.chief_guardian_review("chief".into(), saved.id).await.unwrap().unwrap();
	assert_eq!(stored.approval_state.as_deref(), Some("pending"));
	assert_eq!(stored.status, "denied");
}

#[tokio::test]
async fn guardian_query_pages_preserve_all_reviews_and_frame_budget() {
	let (mut chief, _sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	for number in 0..19 {
		let mut value = review(&format!("review-{number}"), "denied");
		value["action"] = json!({"type":"command","source":"shell","command":"word ".repeat(10_000),"cwd":"/tmp"});
		deliver(&mut chief, value).await;
	}
	let mut seen = std::collections::BTreeSet::new();
	let mut before = None;
	loop {
		let page = crate::chief_guardian::read(&chief.store, "chief", before, None).await;
		assert!(serde_json::to_vec(&page).unwrap().len() < 140 * 1024);
		let decodex_protocol::ChiefGuardianReviewsResult::Available { reviews, next_before } = page
		else {
			panic!("available reviews")
		};
		assert!(!reviews.is_empty());
		for review in reviews {
			assert!(seen.insert(review.row_id));
			assert!(review.can_approve);
		}
		if next_before.is_none() {
			break;
		}
		assert_ne!(before, next_before);
		before = next_before;
	}
	assert_eq!(seen.len(), 19);
}
