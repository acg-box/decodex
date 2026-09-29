//! Persistence of received voice text is separate from replay and call lifetime.
use super::*;

#[tokio::test]
async fn disconnected_voice_preserves_received_tail_without_replay() {
	let (mut agent, mut sent, _directory, database) = fixture().await;
	agent.voice_event(&ServerEvent::Notification {
		method: "thread/realtime/transcript/delta".into(),
		params: json!({"threadId":"opaque thread/1","role":"user","delta":"Received before disconnect."}),
	}).await.expect("received delta");
	agent
		.voice_event(&ServerEvent::Closed(ClientError::Closed))
		.await
		.expect("disconnect observation");
	let saved: String = database
		.query_row(
			"SELECT payload FROM agent_inbox_events WHERE event_kind='voice_user'",
			[],
			|row| row.get(0),
		)
		.expect("received text retained");
	let saved: Value = serde_json::from_str(&saved).expect("saved transcript");
	assert_eq!(saved["text"], "Received before disconnect.");
	assert_eq!(saved["complete"], false);
	agent
		.voice_event(&ServerEvent::Closed(ClientError::Closed))
		.await
		.expect("repeated disconnect");
	let count: i64 = database
		.query_row(
			"SELECT count(*) FROM agent_inbox_events WHERE event_kind='voice_user'",
			[],
			|row| row.get(0),
		)
		.expect("transcript count");
	assert_eq!(count, 1, "saved text is not inserted twice");
	assert_eq!(agent.store.open_agent_voice_calls().await.expect("call state").len(), 1);
	assert!(sent.try_recv().is_err(), "received text must not become new native input");
}

async fn fixture() -> (
	AgentCoordinator,
	tokio::sync::mpsc::UnboundedReceiver<Value>,
	tempfile::TempDir,
	rusqlite::Connection,
) {
	fixture_with_history(json!({})).await
}

async fn fixture_with_history(
	history: Value,
) -> (
	AgentCoordinator,
	tokio::sync::mpsc::UnboundedReceiver<Value>,
	tempfile::TempDir,
	rusqlite::Connection,
) {
	let (mut agent, mut sent, directory) = super::super::tests::fixture_with_history(history).await;
	let owner = crate::agent_model_settings::tests::OwnedReviewer::new(
		directory.path(),
		&agent.client,
		"opaque thread/1",
		"opaque turn/1",
	)
	.await;
	let generation = owner.key.generation.as_str().to_owned();
	agent.store = owner.store;
	let root = decodex_core::DecodexRoot::new(
		directory.path().canonicalize().expect("fixture home").join("state"),
	)
	.expect("fixture root");
	let database =
		rusqlite::Connection::open(root.paths().product_database_file()).expect("fixture database");
	agent
		.store
		.begin_agent_voice_call(decodex_database::AgentVoiceCall {
			session_id: "voice".into(),
			work_id: "root".into(),
			thread_id: "opaque thread/1".into(),
			generation_id: generation.clone(),
			baseline_turn_id: Some("opaque turn/1".into()),
		})
		.await
		.expect("authorized fixture call");
	agent.attach_voice_host(generation, VoiceGateway::new());
	agent.voice.as_mut().expect("voice host").session =
		Some(("voice".into(), "opaque thread/1".into()));
	while sent.try_recv().is_ok() {}
	(agent, sent, directory, database)
}

#[tokio::test]
async fn failed_transcript_write_preserves_text_sequence_and_finality_until_saved() {
	for finalized in [false, true] {
		let (mut agent, mut sent, _directory, database) = fixture().await;
		agent
			.voice_event(&ServerEvent::Notification {
				method: "thread/realtime/transcript/delta".into(),
				params: json!({"threadId":"opaque thread/1","role":"user","delta":"Provisional words."}),
			})
			.await
			.expect("received words");
		database.execute_batch("CREATE TRIGGER fail_transcript BEFORE INSERT ON agent_inbox_events WHEN NEW.event_kind='voice_user' BEGIN SELECT RAISE(FAIL, 'injected transcript failure'); END;").expect("injected storage failure");
		let closed = ServerEvent::Notification {
			method: "thread/realtime/closed".into(),
			params: json!({"threadId":"opaque thread/1"}),
		};
		let completion = ServerEvent::Notification {
			method: "thread/realtime/transcript/done".into(),
			params: json!({"threadId":"opaque thread/1","role":"user","text":"Corrected final words."}),
		};
		assert!(agent.voice_event(if finalized { &completion } else { &closed }).await.is_err());
		let voice = agent.voice.as_ref().expect("retained voice");
		assert_eq!(voice.transcript_sequence, 0);
		assert_eq!(voice.transcript_complete[0], finalized);
		let expected = if finalized { "Corrected final words." } else { "Provisional words." };
		assert_eq!(voice.transcript_tail[0], expected);
		assert!(voice.session.is_some());
		database.execute_batch("DROP TRIGGER fail_transcript;").expect("restore fixture writes");
		agent.voice_event(&closed).await.expect("save retained transcript");
		let saved: String = database
			.query_row(
				"SELECT payload FROM agent_inbox_events WHERE event_kind='voice_user'",
				[],
				|row| row.get(0),
			)
			.expect("saved transcript");
		let saved: Value = serde_json::from_str(&saved).expect("transcript JSON");
		assert_eq!(saved["text"], expected);
		assert_eq!(saved["complete"], finalized);
		assert_eq!(agent.voice.as_ref().expect("voice").transcript_sequence, 1);
		assert!(agent.store.open_agent_voice_calls().await.expect("call state").is_empty());
		assert!(sent.try_recv().is_err(), "persistence cannot submit native input");
	}
}

#[tokio::test]
async fn long_voice_transcripts_keep_utf8_suffix_and_never_claim_truncated_finality() {
	for finalized in [false, true] {
		let (mut agent, mut sent, _directory, database) = fixture().await;
		let prefix = "界".repeat(11_000);
		let suffix = " The latest correction must remain.";
		if finalized {
			agent
				.voice_event(&ServerEvent::Notification {
					method: "thread/realtime/transcript/done".into(),
					params: json!({"threadId":"opaque thread/1","role":"user","text":prefix.clone()+suffix}),
				})
				.await
				.expect("bounded final transcript");
		} else {
			for text in [&prefix, suffix] {
				agent
					.voice_event(&ServerEvent::Notification {
						method: "thread/realtime/transcript/delta".into(),
						params: json!({"threadId":"opaque thread/1","role":"user","delta":text}),
					})
					.await
					.expect("bounded delta");
			}
		}
		agent
			.voice_event(&ServerEvent::Closed(ClientError::Closed))
			.await
			.expect("save received suffix");
		let saved: String = database
			.query_row(
				"SELECT payload FROM agent_inbox_events WHERE event_kind='voice_user'",
				[],
				|row| row.get(0),
			)
			.expect("saved suffix");
		let saved: Value = serde_json::from_str(&saved).expect("transcript JSON");
		let text = saved["text"].as_str().expect("transcript text");
		assert!((TRANSCRIPT_TAIL_BYTES - 3..=TRANSCRIPT_TAIL_BYTES).contains(&text.len()));
		assert!(text.ends_with(suffix));
		assert!((prefix + suffix).ends_with(text));
		assert_eq!(saved["complete"], false);
		assert!(sent.try_recv().is_err());
	}
}

#[tokio::test]
async fn precaution_stop_preserves_text_before_retiring_the_call() {
	let (mut agent, mut sent, _directory, database) = fixture().await;
	agent.voice_event(&ServerEvent::Notification {
		method: "thread/realtime/transcript/delta".into(),
		params: json!({"threadId":"opaque thread/1","role":"user","delta":"Received before native precaution."}),
	}).await.expect("received delta");
	agent.stop_voice_for_precaution("opaque thread/1").await.expect("native stop acknowledged");
	let saved: String = database
		.query_row(
			"SELECT payload FROM agent_inbox_events WHERE event_kind='voice_user'",
			[],
			|row| row.get(0),
		)
		.expect("precaution transcript retained");
	let saved: Value = serde_json::from_str(&saved).expect("saved transcript");
	assert_eq!(saved["text"], "Received before native precaution.");
	assert_eq!(saved["complete"], false);
	assert!(agent.store.open_agent_voice_calls().await.expect("call state").is_empty());
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	assert_eq!(requests.len(), 1);
	assert_eq!(requests[0]["method"], "thread/realtime/stop");
}

#[tokio::test]
async fn precaution_storage_failure_does_not_prevent_native_stop() {
	let (mut agent, mut sent, _directory, database) = fixture().await;
	agent.voice.as_mut().expect("voice").transcript_tail[0] = "Unsaved precaution text.".into();
	database.execute_batch("CREATE TRIGGER fail_transcript BEFORE INSERT ON agent_inbox_events WHEN NEW.event_kind='voice_user' BEGIN SELECT RAISE(FAIL, 'injected transcript failure'); END;").expect("inject failure");
	assert!(agent.stop_voice_for_precaution("opaque thread/1").await.is_err());
	let voice = agent.voice.as_ref().expect("retained voice");
	assert!(voice.precaution_retired);
	assert!(voice.session.is_some());
	assert_eq!(voice.transcript_tail[0], "Unsaved precaution text.");
	assert_eq!(voice.transcript_sequence, 0);
	assert_eq!(sent.try_recv().expect("native stop")["method"], "thread/realtime/stop");
	assert!(sent.try_recv().is_err());
	database.execute_batch("DROP TRIGGER fail_transcript;").expect("restore writes");
	agent
		.voice_event(&ServerEvent::Notification {
			method: "thread/realtime/closed".into(),
			params: json!({"threadId":"opaque thread/1"}),
		})
		.await
		.expect("native closure saves retained text");
	let count: i64 = database
		.query_row(
			"SELECT count(*) FROM agent_inbox_events WHERE event_kind='voice_user'",
			[],
			|row| row.get(0),
		)
		.expect("saved record");
	assert_eq!(count, 1);
	assert!(agent.voice.as_ref().expect("voice").session.is_none());
}

#[tokio::test]
async fn voice_start_rejects_independent_manager_before_native_requests() {
	use decodex_protocol::EntityId;
	let (mut agent, mut sent, _directory, _database) = fixture().await;
	let mut manager = agent.store.get_agent_work_item("root".into()).await.unwrap();
	manager.id = "independent".into();
	manager.parent_goal_id = Some("root".into());
	manager.codex_thread_id = None;
	manager.dispatch_state = decodex_database::AgentDispatchState::Idle;
	manager.active_turn_id = None;
	agent.store.create_agent_manager(manager, None).await.unwrap();
	agent.store.bind_agent_thread("independent".into(), "other-thread".into()).await.unwrap();
	let result = agent
		.voice_request(AgentVoiceRequest::Start {
			session_id: EntityId::new("other-call").unwrap(),
			work_id: EntityId::new("independent").unwrap(),
			offer: VoiceSdp::new("offer".into()).unwrap(),
			options: Default::default(),
		})
		.await;
	assert!(result.is_err());
	assert!(sent.try_recv().is_err(), "foreign voice target must not be resumed or started");
	assert_eq!(agent.store.open_agent_voice_calls().await.unwrap().len(), 1);
}

#[tokio::test]
async fn cold_voice_recovery_keeps_original_generation_and_never_replays_input() {
	for same_generation_first in [false, true] {
		let history = json!({"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":[
		{"id":"before-call","status":"completed","items":[]},
		{"id":"opaque turn/1","status":"completed","items":[]},
		{"id":"spoken-turn","status":"completed","items":[
			{"id":"reply","type":"agentMessage","text":"Saved spoken reply"}]}]}}});
		let (mut agent, mut sent, directory, database) = fixture_with_history(history).await;
		agent.store.complete_agent_turn("root".into(), "opaque turn/1".into()).await.unwrap();
		let original = agent.voice.as_ref().unwrap().generation.clone();
		// A transport retry in the original generation may observe history, but cannot close
		// authority.
		if same_generation_first {
			assert!(agent.recover_voice_calls().await.is_err());
			assert_eq!(agent.store.open_agent_voice_calls().await.unwrap().len(), 1);
		}
		let root =
			decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("state"))
				.unwrap();
		let reopened = decodex_database::SqliteStore::open(&root.paths()).unwrap();
		let mut cold =
			AgentCoordinator::new(reopened, agent.client.clone(), agent.config.clone()).unwrap();
		drop(agent);
		// The host supplies a newly admitted generation; process admission is tested separately.
		cold.attach_voice_host("new-admitted-generation".into(), VoiceGateway::new());
		cold.recover_voice_calls().await.unwrap();
		assert!(cold.store.open_agent_voice_calls().await.unwrap().is_empty());
		let item = cold.store.get_agent_work_item("root".into()).await.unwrap();
		assert_eq!(item.dispatch_state, decodex_database::AgentDispatchState::Idle);
		assert!(item.active_turn_id.is_none());
		let observed: Vec<(String, String)> = database
			.prepare(
				"SELECT generation_id,turn_id FROM agent_voice_observed_turns ORDER BY turn_id",
			)
			.unwrap()
			.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
			.unwrap()
			.collect::<Result<_, _>>()
			.unwrap();
		assert_eq!(observed, vec![(original, "spoken-turn".into())]);
		let receipts: Vec<String> = database
			.prepare(
				"SELECT payload FROM agent_inbox_events WHERE event_kind='agent_turn_completed'",
			)
			.unwrap()
			.query_map([], |r| r.get(0))
			.unwrap()
			.collect::<Result<_, _>>()
			.unwrap();
		assert_eq!(receipts.len(), 1);
		let receipt: Value = serde_json::from_str(&receipts[0]).unwrap();
		assert_eq!(receipt["terminal"]["turn"]["id"], "spoken-turn");
		assert!(receipts[0].contains("Saved spoken reply"));
		let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
		assert!(requests.iter().any(|r| r["method"] == "thread/resume"));
		assert!(requests.iter().all(|r| matches!(
			r["method"].as_str(),
			Some("thread/resume" | "thread/read" | "thread/turns/list" | "thread/items/list")
		)));
		cold.recover_voice_calls().await.unwrap();
		assert!(sent.try_recv().is_err(), "closed calls need no further native recovery");
	}
}
