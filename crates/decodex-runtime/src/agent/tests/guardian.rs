use std::{collections::BTreeSet, iter, time::Duration};

use tokio::{io, time};

use crate::{
	agent::tests::*,
	agent_guardian,
	application::{Application, ProductStore},
	conversation::ConversationCapability,
};
use decodex_core::{DecodexRoot, ProcessGenerationId};
use decodex_database::AgentGuardianObservation;
use decodex_protocol::{
	AgentActivityDto, AgentGuardianReviewsResult, CURRENT_VERSION, ConversationUnavailableReason,
	DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport, DoctorStatus, EntityId, QueryEnvelope,
	QueryId, QueryPayload, QueryResultPayload, ServerId, WireText,
};

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

fn approval_history() -> Value {
	json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","status":{"type":"active"},"turns":[{"id":"opaque turn/1","status":"inProgress","items":[]}]}}})
}

fn detail_service(store: SqliteStore) -> crate::application::ServiceApplication {
	let doctor = DoctorReport::new(
		ServerId::new("guardian-fixture").unwrap(),
		CURRENT_VERSION,
		DoctorComponent::ALL
			.into_iter()
			.map(|component| {
				DoctorCheck::new(component, DoctorStatus::Unavailable(DoctorIssue::NotProbed))
			})
			.collect(),
	)
	.unwrap();

	crate::application::ServiceApplication::new(
		ProductStore::Available(store),
		None,
		None,
		None,
		ConversationCapability::Unavailable(ConversationUnavailableReason::AppServerProfile),
		doctor,
	)
}

#[tokio::test]
async fn large_guardian_details_survive_native_wire_restart_and_exact_paging() {
	use decodex_protocol::AgentGuardianDetailResult as Detail;

	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let command = "echo 中文\\n\"quoted\" ".repeat(20_000) + "FINAL ACTION SUFFIX";
	let mut event = review("large-command", "inProgress");

	event["action"] = json!({"type":"command","source":"shell","command":command,"cwd":"/tmp"});

	deliver(&mut agent, event.clone()).await;

	event["completedAtMs"] = json!(101);
	event["decisionSource"] = json!("agent");
	event["review"]["status"] = json!("denied");
	event["review"]["rationale"] = json!("Long findings. ".repeat(6_000));

	deliver(&mut agent, event.clone()).await;

	assert!(sent.try_recv().is_err(), "retaining a review must not send native work");

	let row =
		agent.store.read_agent_guardian_reviews("agent".into(), None, 1).await.unwrap().remove(0);

	assert!(row.event_json.len() > 256 * 1_024);

	let digest = row.digest();
	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	drop(agent);

	let store = SqliteStore::open(&root.paths()).unwrap();
	let saved = store.agent_guardian_review("agent".into(), row.id).await.unwrap().unwrap();

	assert_eq!(saved.event_json, event.to_string());

	let summary = agent_guardian::read(&store, "agent", None, None).await;
	let AgentGuardianReviewsResult::Available { reviews, .. } = summary else {
		panic!("saved review missing");
	};

	assert!(reviews[0].details_paged && reviews[0].can_approve);
	assert!(reviews[0].action_json.is_none() && reviews[0].rationale.is_none());
	assert!(serde_json::to_vec(&reviews).unwrap().len() < 4_096);

	let peer = SqliteStore::open(&root.paths()).unwrap();
	let app = detail_service(peer.clone());
	let mut offset = 0;
	let mut complete = String::new();

	loop {
		let query = QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new("guardian-page").unwrap(),
			payload: QueryPayload::GetAgentGuardianDetail {
				work_id: EntityId::new("agent").unwrap(),
				review_row: row.id,
				review_digest: WireText::new(&digest).unwrap(),
				offset,
			},
		};
		let QueryResultPayload::AgentGuardianDetail(page) = Application::query(&app, &query).await
		else {
			panic!("detail result");
		};

		assert!(page.matches_request(row.id, &digest, offset));
		assert!(serde_json::to_vec(&page).unwrap().len() < 60 * 1_024);

		let Detail::Available { text, next_offset, .. } = page else {
			panic!("missing detail page");
		};

		complete.push_str(&text);

		let Some(next) = next_offset else {
			break;
		};

		offset = next;
	}

	assert_eq!(
		complete,
		format!(
			"Action\n{}\n\nFindings\n{}",
			event["action"],
			event["review"]["rationale"].as_str().unwrap()
		)
	);

	for (work, expected, offset) in [
		("other", digest.as_str(), 0),
		("agent", "stale", 0),
		("agent", digest.as_str(), complete.len()),
		("agent", digest.as_str(), complete.find('中').unwrap() + 1),
	] {
		assert_eq!(
			agent_guardian::detail(&peer, work, row.id, expected, offset).await,
			Detail::Unavailable
		);
	}
	// A second writer conflicts with the saved action. An earlier page digest must stop working.
	event["action"]["command"] = json!("different action");

	store
		.record_agent_guardian_review(AgentGuardianObservation {
			thread_id: saved.thread_id.clone(),
			turn_id: saved.turn_id.clone(),
			review_id: saved.review_id.clone(),
			connection_id: saved.connection_id.clone(),
			generation_id: saved.generation_id.clone(),
			event_json: event.to_string(),
		})
		.await
		.unwrap();

	assert_eq!(
		agent_guardian::detail(&peer, "agent", saved.id, &digest, 0).await,
		Detail::Unavailable
	);
}

async fn deliver(agent: &mut AgentCoordinator, value: Value) {
	let method = if value["review"]["status"] == "inProgress" {
		"item/autoApprovalReview/started"
	} else {
		"item/autoApprovalReview/completed"
	};
	let (io, mut write) = io::duplex(8_192);
	let (read, writer) = io::split(io);
	let (_client, mut events) = AppServerClient::from_io(read, writer);
	let wire = json!({"method":method,"params":value});

	write.write_all(format!("{wire}\n").as_bytes()).await.unwrap();

	let event = time::timeout(Duration::from_secs(2), events.recv()).await.unwrap().unwrap();

	agent.handle_event(event).await.unwrap();
}

#[tokio::test]
async fn guardian_observations_are_monotonic_bound_durable_and_do_not_wake_work() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	for (thread, turn) in [("foreign", "opaque turn/1"), ("opaque thread/1", "unknown")] {
		let mut value = review("ignored", "denied");

		value["threadId"] = json!(thread);
		value["turnId"] = json!(turn);

		deliver(&mut agent, value).await;
	}

	deliver(&mut agent, review("network", "inProgress")).await;
	deliver(&mut agent, review("network", "denied")).await;
	deliver(&mut agent, review("network", "denied")).await;
	deliver(&mut agent, review("network", "inProgress")).await;
	// A second lifecycle with the same target item must remain independent.
	for id in ["execve-a", "execve-b"] {
		let mut value = review(id, "denied");

		value["targetItemId"] = json!("parent-command");

		deliver(&mut agent, value).await;
	}

	let mut large = review("large-action", "denied");

	large["action"] = json!({"type":"command","source":"shell","command":"界".repeat(100_000) + " exact-required-suffix","cwd":"/tmp"});

	deliver(&mut agent, large.clone()).await;
	deliver(&mut agent, review("unresolved", "inProgress")).await;

	let rows = agent.store.read_agent_guardian_reviews("agent".into(), None, 100).await.unwrap();

	assert_eq!(rows.len(), 5);
	assert!(rows.iter().all(|r| !r.conflicted));
	assert_eq!(rows.iter().find(|r| r.review_id == "network").unwrap().status, "denied");

	let work = agent.store.get_agent_work_item("agent".into()).await.unwrap();

	assert_eq!(work.dispatch_state, decodex_database::AgentDispatchState::Running);

	agent.handle_event(ServerEvent::Notification {method:"turn/completed".into(), params:json!({"threadId":"opaque thread/1","turn":{"id":"opaque turn/1","status":"completed","items":[]}})}).await.unwrap();
	// Native completion may be processed after the owning turn's terminal event.
	deliver(&mut agent, review("late", "approved")).await;

	agent.wake_pending().await.unwrap();

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start", "observation must not start a turn");
		assert_ne!(request["method"], "thread/approveGuardianDeniedAction");
	}

	assert!(agent.store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());

	let connection = agent.connection_id.clone();
	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	drop(agent);

	let store = SqliteStore::open(&root.paths()).unwrap();
	let first = store.read_agent_guardian_reviews("agent".into(), None, 2).await.unwrap();
	let rest =
		store.read_agent_guardian_reviews("agent".into(), Some(first[1].id), 100).await.unwrap();

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
		store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);
}

#[tokio::test]
async fn conflicting_guardian_evidence_retains_original_and_invalidates_approval_identity() {
	let (mut agent, _sent, _directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let original = review("network", "denied");

	deliver(&mut agent, original.clone()).await;

	let mut changed = original.clone();

	changed["action"]["host"] = json!("different.test");

	deliver(&mut agent, changed).await;
	deliver(&mut agent, original.clone()).await;

	let rows = agent.store.read_agent_guardian_reviews("agent".into(), None, 100).await.unwrap();

	assert_eq!(rows.len(), 1);
	assert!(rows[0].conflicted);
	assert_eq!(serde_json::from_str::<Value>(&rows[0].event_json).unwrap(), original);
	// A second terminal status must never turn the previous denial into approval.
	deliver(&mut agent, review("another", "denied")).await;
	deliver(&mut agent, review("another", "approved")).await;

	let rows = agent.store.read_agent_guardian_reviews("agent".into(), None, 100).await.unwrap();

	assert_eq!(rows[0].status, "denied");
	assert!(rows[0].conflicted);
}

#[tokio::test]
async fn guardian_review_from_unbound_native_generation_is_not_retained() {
	let (mut agent, _sent, _directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();
	agent.bind_native_generation(
		ProcessGenerationId::new("30000000-0000-4000-8000-000000000001").unwrap(),
	);

	deliver(&mut agent, review("wrong-process", "denied")).await;

	assert!(
		agent
			.store
			.read_agent_guardian_reviews("agent".into(), None, 100)
			.await
			.unwrap()
			.is_empty()
	);
}

#[tokio::test]
async fn guardian_user_approval_submits_exact_denial_once_without_executing_a_turn() {
	let (mut agent, mut sent, _directory) = fixture_with_history(approval_history()).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	deliver(&mut agent, review("network", "denied")).await;

	let saved =
		agent.store.read_agent_guardian_reviews("agent".into(), None, 1).await.unwrap().remove(0);

	while sent.try_recv().is_ok() {}

	assert!(
		agent.approve_guardian_denial("agent", saved.id, "stale", "stale-click").await.is_err()
	);
	assert!(sent.try_recv().is_err());

	agent.approve_guardian_denial("agent", saved.id, &saved.digest(), "user-click").await.unwrap();

	assert!(
		agent
			.approve_guardian_denial("agent", saved.id, &saved.digest(), "second-click")
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
		agent.store.agent_guardian_review("agent".into(), saved.id).await.unwrap().unwrap();

	assert_eq!(stored.approval_state.as_deref(), Some("submitted"));
	assert_eq!(stored.status, "denied");
	assert_eq!(stored.event_json, saved.event_json);
	assert_eq!(
		agent.store.get_agent_work_item("agent".into()).await.unwrap().active_turn_id.as_deref(),
		Some("opaque turn/1")
	);
}

#[tokio::test]
async fn guardian_unloaded_approval_preserves_native_settings_without_starting_a_turn() {
	let (mut agent, mut sent, _directory) = fixture_with_history(approval_history()).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	deliver(&mut agent, review("network", "denied")).await;

	let saved =
		agent.store.read_agent_guardian_reviews("agent".into(), None, 1).await.unwrap().remove(0);

	agent.config = AgentConfig::new("different-startup-model".into(), "low".into(), "/tmp".into());

	agent
		.handle_event(ServerEvent::Notification {
			method: "thread/closed".into(),
			params: json!({"threadId":"opaque thread/1"}),
		})
		.await
		.unwrap();

	while sent.try_recv().is_ok() {}

	agent.approve_guardian_denial("agent", saved.id, &saved.digest(), "cold-click").await.unwrap();

	let requests: Vec<_> = iter::from_fn(|| sent.try_recv().ok()).collect();

	assert!(requests.iter().all(|r| r["method"] != "turn/start" && r["method"] != "thread/start"));

	let resumes: Vec<_> = requests.iter().filter(|r| r["method"] == "thread/resume").collect();

	assert_eq!(resumes.len(), 1);
	assert_eq!(
		resumes[0]["params"],
		json!({"threadId":"opaque thread/1","excludeTurns":true,"experimentalRawEvents":true})
	);

	let approvals: Vec<_> =
		requests.iter().filter(|r| r["method"] == "thread/approveGuardianDeniedAction").collect();

	assert_eq!(approvals.len(), 1);
	assert_eq!(approvals[0]["params"]["threadId"], "opaque thread/1");
	assert_eq!(approvals[0]["params"]["event"]["action"]["type"], "network_access");

	let stored =
		agent.store.agent_guardian_review("agent".into(), saved.id).await.unwrap().unwrap();

	assert_eq!(stored.approval_state.as_deref(), Some("submitted"));
	assert_eq!(stored.event_json, saved.event_json);
}

#[tokio::test]
async fn guardian_rejection_and_lost_reply_have_distinct_durable_outcomes() {
	for (mode, expected) in [("_guardian_reject", "rejected"), ("_guardian_disconnect", "pending")]
	{
		let mut history = approval_history();

		history[mode] = json!(true);

		let (mut agent, mut sent, directory) = fixture_with_history(history).await;

		agent.start_agent("agent", "Coordinate").await.unwrap();

		deliver(&mut agent, review("network", "denied")).await;

		let saved = agent
			.store
			.read_agent_guardian_reviews("agent".into(), None, 1)
			.await
			.unwrap()
			.remove(0);

		while sent.try_recv().is_ok() {}

		let outcome =
			agent.approve_guardian_denial("agent", saved.id, &saved.digest(), "user-click").await;

		if expected == "pending" {
			assert!(matches!(outcome, Err(AgentError::UnknownDispatch)));
		} else {
			assert!(matches!(outcome, Err(AgentError::Rejected(_))));
		}

		let mut calls = 0;

		while let Ok(request) = sent.try_recv() {
			assert_ne!(request["method"], "turn/start");

			calls += usize::from(request["method"] == "thread/approveGuardianDeniedAction");
		}

		assert_eq!(calls, 1);

		let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

		drop(agent);

		let store = SqliteStore::open(&root.paths()).unwrap();
		let restored =
			store.agent_guardian_review("agent".into(), saved.id).await.unwrap().unwrap();

		assert_eq!(restored.approval_state.as_deref(), Some(expected));
		assert!(
			store
				.begin_agent_guardian_approval(
					"agent".into(),
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
			.begin_agent_guardian_approval(
				"agent".into(),
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

		let (mut agent, mut sent, _directory) = fixture_with_history(history).await;

		agent.start_agent("agent", "Coordinate").await.unwrap();

		deliver(&mut agent, review("network", "denied")).await;

		let saved = agent
			.store
			.read_agent_guardian_reviews("agent".into(), None, 1)
			.await
			.unwrap()
			.remove(0);

		if changed_action {
			let mut changed = review("network", "denied");

			changed["action"]["host"] = json!("different.test");

			deliver(&mut agent, changed).await;
		}

		while sent.try_recv().is_ok() {}

		assert!(matches!(
			agent.approve_guardian_denial("agent", saved.id, &saved.digest(), "user-click").await,
			Err(AgentError::Rejected(_))
		));

		while let Ok(request) = sent.try_recv() {
			assert_ne!(request["method"], "thread/approveGuardianDeniedAction");
		}

		assert_eq!(
			agent
				.store
				.agent_guardian_review("agent".into(), saved.id)
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
	let (mut agent, _sent, _directory) = fixture_with_history(approval_history()).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	deliver(&mut agent, review("network", "denied")).await;

	let saved =
		agent.store.read_agent_guardian_reviews("agent".into(), None, 1).await.unwrap().remove(0);
	let claim = |key: &str| {
		agent.store.begin_agent_guardian_approval(
			"agent".into(),
			saved.id,
			saved.digest(),
			agent.connection_id.clone(),
			None,
			key.into(),
		)
	};
	let (first, second) = tokio::join!(claim("first-click"), claim("second-click"));

	assert_ne!(first.is_ok(), second.is_ok());

	let stored =
		agent.store.agent_guardian_review("agent".into(), saved.id).await.unwrap().unwrap();

	assert_eq!(stored.approval_state.as_deref(), Some("pending"));
	assert_eq!(stored.status, "denied");
}

#[tokio::test]
async fn guardian_query_pages_preserve_all_reviews_and_frame_budget() {
	let (mut agent, _sent, _directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	for number in 0..19 {
		let mut value = review(&format!("review-{number}"), "denied");

		value["action"] = json!({"type":"command","source":"shell","command":"word ".repeat(10_000),"cwd":"/tmp"});

		deliver(&mut agent, value).await;
	}

	let mut seen = BTreeSet::new();
	let mut before = None;

	loop {
		let page = agent_guardian::read(&agent.store, "agent", before, None).await;

		assert!(serde_json::to_vec(&page).unwrap().len() < 140 * 1_024);

		let AgentGuardianReviewsResult::Available { reviews, next_before } = page else {
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

#[tokio::test]
async fn guardian_expanded_approval_frame_is_rejected_before_reservation_or_rpc() {
	let (mut agent, mut sent, _directory) = fixture_with_history(approval_history()).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let mut value = review("expanded-path", "denied");

	value["action"] = json!({"type":"writeStdin","approvalId":"child",
		"processId":"terminal","stdin":"exact input","cwd":"/".to_owned() + &"界".repeat(1_000_000)});

	let wire = json!({"method":"item/autoApprovalReview/completed","params":value});

	assert!(serde_json::to_vec(&wire).unwrap().len() < decodex_core::MAX_NATIVE_MESSAGE_BYTES);

	deliver(&mut agent, wire["params"].clone()).await;

	let saved =
		agent.store.read_agent_guardian_reviews("agent".into(), None, 1).await.unwrap().remove(0);

	while sent.try_recv().is_ok() {}

	assert!(matches!(
		agent.approve_guardian_denial("agent", saved.id, &saved.digest(), "user-click").await,
		Err(AgentError::Rejected(_))
	));
	assert!(sent.try_recv().is_err());

	let stored =
		agent.store.agent_guardian_review("agent".into(), saved.id).await.unwrap().unwrap();

	assert_eq!(stored.approval_state, None);
	assert_eq!(stored.event_json, saved.event_json);
}

#[tokio::test]
async fn strict_review_from_unbound_native_generation_is_not_retained() {
	let (mut agent, mut sent, _directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	agent.bind_native_generation(
		ProcessGenerationId::new("30000000-0000-4000-8000-000000000001").unwrap(),
	);
	agent
		.handle_event(ServerEvent::Notification {
			method: "autoApprovalReview/strictReviewRequired".into(),
			params: json!({"threadId":"opaque thread/1","turnId":"opaque turn/1","startedAtMs":100}),
		})
		.await
		.unwrap();

	let (history, _) = agent.store.read_agent_transcript("agent".into(), None, 32).await.unwrap();

	assert!(!history.iter().any(|event| event.event_kind == "strict_review_notice"));
	assert!(sent.try_recv().is_err());
	assert!(agent.store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());
}

#[tokio::test]
async fn guardian_review_failure_preserves_absent_assessment_after_restart() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let mut event = review("failed-review", "denied");
	let rationale = "Automatic approval review failed: temporary review error";

	event["review"]["rationale"] = json!(rationale);

	deliver(&mut agent, event.clone()).await;

	assert!(sent.try_recv().is_err(), "review failure must not replay the action");

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	drop(agent);

	let store = SqliteStore::open(&root.paths()).unwrap();
	let AgentGuardianReviewsResult::Available { reviews, .. } =
		agent_guardian::read(&store, "agent", None, None).await
	else {
		panic!("saved review missing");
	};

	assert_eq!(reviews.len(), 1);
	assert_eq!(reviews[0].status, decodex_protocol::AgentGuardianStatus::Denied);
	assert_eq!(reviews[0].risk_level, None);
	assert_eq!(reviews[0].user_authorization, None);
	assert_eq!(reviews[0].rationale.as_deref(), Some(rationale));

	let saved = store.read_agent_guardian_reviews("agent".into(), None, 1).await.unwrap();

	assert_eq!(saved[0].event_json, event.to_string());
	assert_eq!(saved[0].approval_state, None);
}

#[tokio::test]
async fn finished_command_survives_late_network_review_cancellation() {
	let (mut agent, mut sent, directory) = fixture().await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let mut pending = review("network", "inProgress");

	pending["targetItemId"] = json!("command");

	deliver(&mut agent, pending.clone()).await;

	agent
		.handle_event(ServerEvent::Notification {
			method: "item/completed".into(),
			params: json!({"threadId":"opaque thread/1","turnId":"opaque turn/1",
			"item":{"id":"command","type":"commandExecution","status":"completed",
				"exitCode":0,"aggregatedOutput":"build complete\n"}}),
		})
		.await
		.unwrap();
	agent
		.handle_event(ServerEvent::Notification {
			method: "turn/completed".into(),
			params: json!({"threadId":"opaque thread/1",
			"turn":{"id":"opaque turn/1","status":"completed","items":[]}}),
		})
		.await
		.unwrap();

	let mut cancelled = review("network", "aborted");

	cancelled["targetItemId"] = json!("command");
	cancelled["review"]["rationale"] = json!(null);

	deliver(&mut agent, cancelled.clone()).await;
	deliver(&mut agent, pending).await;
	deliver(&mut agent, cancelled.clone()).await;

	agent.wake_pending().await.unwrap();

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");
		assert_ne!(request["method"], "thread/approveGuardianDeniedAction");
	}

	assert!(agent.pending_requests.is_empty());

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	drop(agent);

	let store = SqliteStore::open(&root.paths()).unwrap();
	let rows = store.read_agent_guardian_reviews("agent".into(), None, 100).await.unwrap();

	assert_eq!(rows.len(), 1);
	assert_eq!(rows[0].status, "aborted");
	assert!(!rows[0].conflicted);
	assert!(rows[0].approval_state.is_none());
	assert_eq!(serde_json::from_str::<Value>(&rows[0].event_json).unwrap(), cancelled);

	let (events, _) = store.read_agent_transcript("agent".into(), None, 32).await.unwrap();
	let activities: Vec<AgentActivityDto> = events
		.iter()
		.filter(|event| event.event_kind.starts_with("activity_"))
		.map(|event| serde_json::from_str(&event.payload).unwrap())
		.collect();

	assert_eq!(activities.len(), 1);
	assert_eq!(activities[0].item_id, "command");
	assert_eq!(activities[0].status, "completed");
	assert_eq!(activities[0].detail, "Exit code 0");
	assert_eq!(
		store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
		decodex_database::AgentDispatchState::Idle
	);
	assert!(store.list_agent_wake_events("agent".into(), 32).await.unwrap().is_empty());
}

#[tokio::test]
async fn large_guardian_approval_keeps_the_complete_action_on_the_native_request() {
	let (mut agent, mut sent, _directory) = fixture_with_history(approval_history()).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	let command = "echo 中\\\" ".repeat(40_000) + "EXACT FINAL ARGUMENT";
	let mut event = review("large-denial", "denied");

	event["action"] = json!({"type":"command","source":"shell","command":command,"cwd":"/tmp"});

	deliver(&mut agent, event).await;

	let saved =
		agent.store.read_agent_guardian_reviews("agent".into(), None, 1).await.unwrap().remove(0);

	while sent.try_recv().is_ok() {}

	agent
		.approve_guardian_denial("agent", saved.id, &saved.digest(), "large-approval")
		.await
		.unwrap();

	let mut approvals = Vec::new();

	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "turn/start");

		if request["method"] == "thread/approveGuardianDeniedAction" {
			approvals.push(request);
		}
	}

	assert_eq!(approvals.len(), 1);
	assert_eq!(approvals[0]["params"]["event"]["action"]["command"], command);
	assert_eq!(
		agent
			.store
			.agent_guardian_review("agent".into(), saved.id)
			.await
			.unwrap()
			.unwrap()
			.approval_state
			.as_deref(),
		Some("submitted")
	);
}

#[tokio::test]
async fn foreign_guardian_paths_survive_storage_and_explicit_approval() {
	let (mut agent, mut sent, directory) = fixture_with_history(approval_history()).await;

	agent.start_agent("agent", "Coordinate").await.unwrap();

	while sent.try_recv().is_ok() {}

	let paths = [r"C:\work\中文 %23", r"\\executor\share\work", "/C:/literal/%23"];
	let mut saved_events = Vec::new();

	for (index, path) in paths.iter().enumerate() {
		for kind in ["command", "applyPatch"] {
			let mut event = review(&format!("foreign-{index}-{kind}"), "denied");

			event["action"] = if kind == "command" {
				json!({"type":kind,"source":"unifiedExec","command":"inspect","cwd":path})
			} else {
				json!({"type":kind,"cwd":path,"files":[format!("{path}/file #.txt")]})
			};

			deliver(&mut agent, event.clone()).await;

			assert!(sent.try_recv().is_err(), "observation cannot submit approval");

			let saved = agent
				.store
				.read_agent_guardian_reviews("agent".into(), None, 1)
				.await
				.unwrap()
				.remove(0);

			assert_eq!(serde_json::from_str::<Value>(&saved.event_json).unwrap(), event);

			agent
				.approve_guardian_denial(
					"agent",
					saved.id,
					&saved.digest(),
					&format!("click-{index}-{kind}"),
				)
				.await
				.unwrap();

			let mut approvals = Vec::new();

			while let Ok(request) = sent.try_recv() {
				assert_ne!(request["method"], "turn/start");

				if request["method"] == "thread/approveGuardianDeniedAction" {
					approvals.push(request["params"]["event"]["action"].clone());
				}
			}

			let mut expected = event["action"].clone();

			if kind == "command" {
				expected["source"] = json!("unified_exec");
			} else {
				expected["type"] = json!("apply_patch");
			}

			assert_eq!(approvals, vec![expected]);

			saved_events.push((saved.id, event));
		}
	}

	let root = DecodexRoot::new(directory.path().canonicalize().unwrap().join("root")).unwrap();

	drop(agent);

	let store = SqliteStore::open(&root.paths()).unwrap();

	for (id, event) in saved_events {
		let restored = store.agent_guardian_review("agent".into(), id).await.unwrap().unwrap();

		assert_eq!(serde_json::from_str::<Value>(&restored.event_json).unwrap(), event);
		assert_eq!(restored.approval_state.as_deref(), Some("submitted"));
	}
}
