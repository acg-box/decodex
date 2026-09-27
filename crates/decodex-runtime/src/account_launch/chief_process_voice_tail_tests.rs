//! Transport loss preserves received speech without creating executable input.
use super::*;
use crate::chief::{ChiefConfig, ChiefCoordinator};
use decodex_codex::app_server_client::{ClientError, ServerEvent};
use decodex_protocol::{ChiefVoicePhase, ChiefVoiceRequest, EntityId, VoiceSdp};
use serde_json::json;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

#[tokio::test]
async fn disconnected_voice_tails_survive_reopen_without_replay() {
	check_disconnected_tails("Correction", "", "Correction", None).await;
}

#[tokio::test]
async fn disconnected_voice_tails_keep_recent_unicode_at_the_storage_limit() {
	check_disconnected_tails(
		&"文".repeat(10_922),
		"末尾",
		&format!("{}末尾", "文".repeat(10_920)),
		None,
	)
	.await;
	check_disconnected_tails(
		&"文".repeat(12_000),
		"末尾",
		&format!("{}末尾", "文".repeat(10_920)),
		None,
	)
	.await;
}

#[tokio::test]
async fn failed_final_voice_transcript_preserves_correction_until_close() {
	check_disconnected_tails(
		"Correction",
		"",
		"Correction",
		Some(("Corrected final answer", "Corrected final answer")),
	)
	.await;
}

#[tokio::test]
async fn oversized_final_voice_transcript_keeps_a_valid_suffix_after_write_failure() {
	let final_answer = format!("Start {} end", "文".repeat(12_000));
	let expected = format!("{} end", "文".repeat(10_921));
	check_disconnected_tails("Correction", "", "Correction", Some((&final_answer, &expected)))
		.await;
}

async fn check_disconnected_tails(
	user_delta: &str,
	user_end: &str,
	expected_user: &str,
	final_answer: Option<(&str, &str)>,
) {
	let home = tempfile::tempdir().expect("voice transcript fixture");
	let (local, remote) = tokio::io::duplex(65536);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let (sent, mut requests) = tokio::sync::mpsc::unbounded_channel();
	let server = tokio::spawn(async move {
		let (reader, mut writer) = tokio::io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		while let Some(line) = lines.next_line().await.expect("voice transcript fixture") {
			let request: serde_json::Value =
				serde_json::from_str(&line).expect("voice transcript fixture");
			let result = match request["method"].as_str().expect("voice transcript fixture") {
				"thread/resume" | "thread/read" =>
					json!({"thread":{"id":"voice-thread","cwd":"/tmp","turns":[]}}),
				"config/read" => json!({"config":{"realtime":{"voice":"juniper"}}}),
				"thread/realtime/start" => {
					assert_eq!(request["params"]["voice"], "juniper");
					json!({})
				},
				method => panic!("Unexpected native request: {method}"),
			};
			sent.send(request.clone()).expect("voice transcript fixture");
			writer
				.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
				.await
				.expect("voice transcript fixture");
		}
	});
	let owned = OwnedReviewer::new(home.path(), &client, "voice-thread", "active-turn").await;
	let mut chief = ChiefCoordinator::new(
		owned.store.clone(),
		client,
		ChiefConfig::new(
			"gpt-5.6-sol".into(),
			"high".into(),
			home.path().to_str().expect("voice transcript fixture").into(),
		),
	)
	.expect("voice transcript fixture");
	let gateway = crate::chief_voice::VoiceGateway::new();
	chief.attach_voice_host(GENERATION.into(), gateway.clone());
	let start = ChiefVoiceRequest::Start {
		session_id: EntityId::new("voice").expect("voice transcript fixture"),
		work_id: EntityId::new("root").expect("voice transcript fixture"),
		offer: VoiceSdp::new("offer".into()).expect("voice transcript fixture"),
	};
	gateway.exchange(&start);
	chief.voice_request(start).await.expect("voice transcript fixture");
	while requests.try_recv().is_ok() {}
	for (role, delta) in
		[("assistant", "Answer"), ("user", user_delta), ("assistant", " tail"), ("user", user_end)]
	{
		chief
			.handle_event(ServerEvent::Notification {
				method: "thread/realtime/transcript/delta".into(),
				params: json!({"threadId":"voice-thread","role":role,"delta":delta}),
			})
			.await
			.expect("voice transcript fixture");
	}
	let db = rusqlite::Connection::open(owned.root.paths().product_database_file())
		.expect("voice transcript fixture");
	db.execute_batch("CREATE TRIGGER fail_voice_tail BEFORE INSERT ON chief_inbox_events WHEN NEW.event_kind='voice_assistant' BEGIN SELECT RAISE(FAIL, 'injected transcript failure'); END;").expect("voice transcript fixture");
	if let Some((text, _)) = final_answer {
		assert!(
			chief
				.handle_event(ServerEvent::Notification {
					method: "thread/realtime/transcript/done".into(),
					params: json!({"threadId":"voice-thread","role":"assistant","text":text}),
				})
				.await
				.is_err()
		);
	}
	assert!(chief.handle_event(ServerEvent::Closed(ClientError::Closed)).await.is_err());
	db.execute_batch("DROP TRIGGER fail_voice_tail;").expect("voice transcript fixture");
	for _ in 0..2 {
		assert!(chief.handle_event(ServerEvent::Closed(ClientError::Closed)).await.is_err());
	}
	assert_eq!(
		gateway
			.exchange(&ChiefVoiceRequest::Poll {
				session_id: EntityId::new("voice").expect("voice transcript fixture")
			})
			.phase,
		ChiefVoicePhase::Failed
	);
	let reopened = SqliteStore::open(&owned.root.paths()).expect("voice transcript fixture");
	assert_eq!(
		reopened.open_chief_voice_calls().await.expect("voice transcript fixture").len(),
		1,
		"Transport loss does not prove process death"
	);
	drop(db);
	let db = rusqlite::Connection::open(owned.root.paths().product_database_file())
		.expect("voice transcript fixture");
	let rows = db.prepare("SELECT event_kind,json_extract(payload,'$.text'),disposition FROM chief_inbox_events WHERE event_kind LIKE 'voice_%' ORDER BY id").expect("voice transcript fixture").query_map([], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?))).expect("voice transcript fixture").collect::<Result<Vec<_>,_>>().expect("voice transcript fixture");
	assert_eq!(
		rows,
		vec![
			("voice_user".into(), expected_user.into(), "resolved".into()),
			(
				"voice_assistant".into(),
				final_answer.map_or("Answer tail", |(_, expected)| expected).into(),
				"resolved".into()
			)
		]
	);
	let complete: bool = db.query_row("SELECT json_extract(payload,'$.complete') FROM chief_inbox_events WHERE event_kind='voice_assistant'", [], |row| row.get(0)).expect("saved finality");
	assert_eq!(
		complete,
		final_answer.is_some_and(|(text, _)| text.len() <= 32_768),
		"truncated final text remains partial"
	);
	assert!(requests.try_recv().is_err(), "Transcript preservation must not send native input");
	server.abort();
	assert!(server.await.expect_err("cancelled fixture server").is_cancelled());
}
