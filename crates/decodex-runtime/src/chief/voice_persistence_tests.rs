//! Persistence of received voice text is separate from replay and call lifetime.
use super::*;

#[tokio::test]
async fn disconnected_voice_preserves_received_tail_without_replay() {
	let (mut chief, mut sent, _directory, database) = fixture().await;
	chief.voice_event(&ServerEvent::Notification {
		method: "thread/realtime/transcript/delta".into(),
		params: json!({"threadId":"opaque thread/1","role":"user","delta":"Received before disconnect."}),
	}).await.expect("received delta");
	chief
		.voice_event(&ServerEvent::Closed(ClientError::Closed))
		.await
		.expect("disconnect observation");
	let saved: String = database
		.query_row(
			"SELECT payload FROM chief_inbox_events WHERE event_kind='voice_user'",
			[],
			|row| row.get(0),
		)
		.expect("received text retained");
	let saved: Value = serde_json::from_str(&saved).expect("saved transcript");
	assert_eq!(saved["text"], "Received before disconnect.");
	assert_eq!(saved["complete"], false);
	chief
		.voice_event(&ServerEvent::Closed(ClientError::Closed))
		.await
		.expect("repeated disconnect");
	let count: i64 = database
		.query_row(
			"SELECT count(*) FROM chief_inbox_events WHERE event_kind='voice_user'",
			[],
			|row| row.get(0),
		)
		.expect("transcript count");
	assert_eq!(count, 1, "saved text is not inserted twice");
	assert_eq!(chief.store.open_chief_voice_calls().await.expect("call state").len(), 1);
	assert!(sent.try_recv().is_err(), "received text must not become new native input");
}

async fn fixture() -> (
	ChiefCoordinator,
	tokio::sync::mpsc::UnboundedReceiver<Value>,
	tempfile::TempDir,
	rusqlite::Connection,
) {
	let (mut chief, mut sent, directory) =
		super::super::tests::fixture_with_history(json!({})).await;
	let owner = crate::chief_model_settings::tests::OwnedReviewer::new(
		directory.path(),
		&chief.client,
		"opaque thread/1",
		"opaque turn/1",
	)
	.await;
	let generation = owner.key.generation.as_str().to_owned();
	chief.store = owner.store;
	let root = decodex_core::DecodexRoot::new(
		directory.path().canonicalize().expect("fixture home").join("state"),
	)
	.expect("fixture root");
	let database =
		rusqlite::Connection::open(root.paths().product_database_file()).expect("fixture database");
	chief
		.store
		.begin_chief_voice_call(decodex_database::ChiefVoiceCall {
			session_id: "voice".into(),
			work_id: "root".into(),
			thread_id: "opaque thread/1".into(),
			generation_id: generation.clone(),
			baseline_turn_id: Some("opaque turn/1".into()),
		})
		.await
		.expect("authorized fixture call");
	chief.attach_voice_host(generation, VoiceGateway::new());
	chief.voice.as_mut().expect("voice host").session =
		Some(("voice".into(), "opaque thread/1".into()));
	while sent.try_recv().is_ok() {}
	(chief, sent, directory, database)
}

#[tokio::test]
async fn failed_transcript_write_preserves_text_sequence_and_finality_until_saved() {
	for finalized in [false, true] {
		let (mut chief, mut sent, _directory, database) = fixture().await;
		chief
			.voice_event(&ServerEvent::Notification {
				method: "thread/realtime/transcript/delta".into(),
				params: json!({"threadId":"opaque thread/1","role":"user","delta":"Provisional words."}),
			})
			.await
			.expect("received words");
		database.execute_batch("CREATE TRIGGER fail_transcript BEFORE INSERT ON chief_inbox_events WHEN NEW.event_kind='voice_user' BEGIN SELECT RAISE(FAIL, 'injected transcript failure'); END;").expect("injected storage failure");
		let closed = ServerEvent::Notification {
			method: "thread/realtime/closed".into(),
			params: json!({"threadId":"opaque thread/1"}),
		};
		let completion = ServerEvent::Notification {
			method: "thread/realtime/transcript/done".into(),
			params: json!({"threadId":"opaque thread/1","role":"user","text":"Corrected final words."}),
		};
		assert!(chief.voice_event(if finalized { &completion } else { &closed }).await.is_err());
		let voice = chief.voice.as_ref().expect("retained voice");
		assert_eq!(voice.transcript_sequence, 0);
		assert_eq!(voice.transcript_complete[0], finalized);
		let expected = if finalized { "Corrected final words." } else { "Provisional words." };
		assert_eq!(voice.transcript_tail[0], expected);
		assert!(voice.session.is_some());
		database.execute_batch("DROP TRIGGER fail_transcript;").expect("restore fixture writes");
		chief.voice_event(&closed).await.expect("save retained transcript");
		let saved: String = database
			.query_row(
				"SELECT payload FROM chief_inbox_events WHERE event_kind='voice_user'",
				[],
				|row| row.get(0),
			)
			.expect("saved transcript");
		let saved: Value = serde_json::from_str(&saved).expect("transcript JSON");
		assert_eq!(saved["text"], expected);
		assert_eq!(saved["complete"], finalized);
		assert_eq!(chief.voice.as_ref().expect("voice").transcript_sequence, 1);
		assert!(chief.store.open_chief_voice_calls().await.expect("call state").is_empty());
		assert!(sent.try_recv().is_err(), "persistence cannot submit native input");
	}
}

#[tokio::test]
async fn long_voice_transcripts_keep_utf8_suffix_and_never_claim_truncated_finality() {
	for finalized in [false, true] {
		let (mut chief, mut sent, _directory, database) = fixture().await;
		let prefix = "界".repeat(11_000);
		let suffix = " The latest correction must remain.";
		if finalized {
			chief
				.voice_event(&ServerEvent::Notification {
					method: "thread/realtime/transcript/done".into(),
					params: json!({"threadId":"opaque thread/1","role":"user","text":prefix.clone()+suffix}),
				})
				.await
				.expect("bounded final transcript");
		} else {
			for text in [&prefix, suffix] {
				chief
					.voice_event(&ServerEvent::Notification {
						method: "thread/realtime/transcript/delta".into(),
						params: json!({"threadId":"opaque thread/1","role":"user","delta":text}),
					})
					.await
					.expect("bounded delta");
			}
		}
		chief
			.voice_event(&ServerEvent::Closed(ClientError::Closed))
			.await
			.expect("save received suffix");
		let saved: String = database
			.query_row(
				"SELECT payload FROM chief_inbox_events WHERE event_kind='voice_user'",
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
	let (mut chief, mut sent, _directory, database) = fixture().await;
	chief.voice_event(&ServerEvent::Notification {
		method: "thread/realtime/transcript/delta".into(),
		params: json!({"threadId":"opaque thread/1","role":"user","delta":"Received before native precaution."}),
	}).await.expect("received delta");
	chief.stop_voice_for_precaution("opaque thread/1").await.expect("native stop acknowledged");
	let saved: String = database
		.query_row(
			"SELECT payload FROM chief_inbox_events WHERE event_kind='voice_user'",
			[],
			|row| row.get(0),
		)
		.expect("precaution transcript retained");
	let saved: Value = serde_json::from_str(&saved).expect("saved transcript");
	assert_eq!(saved["text"], "Received before native precaution.");
	assert_eq!(saved["complete"], false);
	assert!(chief.store.open_chief_voice_calls().await.expect("call state").is_empty());
	let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
	assert_eq!(requests.len(), 1);
	assert_eq!(requests[0]["method"], "thread/realtime/stop");
}

#[tokio::test]
async fn precaution_storage_failure_does_not_prevent_native_stop() {
	let (mut chief, mut sent, _directory, database) = fixture().await;
	chief.voice.as_mut().expect("voice").transcript_tail[0] = "Unsaved precaution text.".into();
	database.execute_batch("CREATE TRIGGER fail_transcript BEFORE INSERT ON chief_inbox_events WHEN NEW.event_kind='voice_user' BEGIN SELECT RAISE(FAIL, 'injected transcript failure'); END;").expect("inject failure");
	assert!(chief.stop_voice_for_precaution("opaque thread/1").await.is_err());
	let voice = chief.voice.as_ref().expect("retained voice");
	assert!(voice.precaution_retired);
	assert!(voice.session.is_some());
	assert_eq!(voice.transcript_tail[0], "Unsaved precaution text.");
	assert_eq!(voice.transcript_sequence, 0);
	assert_eq!(sent.try_recv().expect("native stop")["method"], "thread/realtime/stop");
	assert!(sent.try_recv().is_err());
	database.execute_batch("DROP TRIGGER fail_transcript;").expect("restore writes");
	chief
		.voice_event(&ServerEvent::Notification {
			method: "thread/realtime/closed".into(),
			params: json!({"threadId":"opaque thread/1"}),
		})
		.await
		.expect("native closure saves retained text");
	let count: i64 = database
		.query_row(
			"SELECT count(*) FROM chief_inbox_events WHERE event_kind='voice_user'",
			[],
			|row| row.get(0),
		)
		.expect("saved record");
	assert_eq!(count, 1);
	assert!(chief.voice.as_ref().expect("voice").session.is_none());
}
