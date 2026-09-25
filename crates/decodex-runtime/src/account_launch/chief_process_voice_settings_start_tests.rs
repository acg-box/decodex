//! Effective configuration is read before creating a native voice session.
use super::*;
use crate::chief::{ChiefConfig, ChiefCoordinator};
use decodex_protocol::{ChiefVoiceRequest, EntityId, VoiceSdp};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

#[tokio::test]
async fn voice_start_applies_effective_voice_and_rejects_failed_reads_before_recording_call() {
	for (voice, fails) in [("juniper", false), ("future_voice", false), ("juniper", true)] {
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = tokio::io::duplex(8192);
		let (read, write) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(read, write);
		let (sent, mut requests) = tokio::sync::mpsc::unbounded_channel();
		let server = tokio::spawn(async move {
			let (read, mut write) = tokio::io::split(remote);
			let mut lines = BufReader::new(read).lines();
			while let Some(line) = lines.next_line().await.unwrap() {
				let request: Value = serde_json::from_str(&line).unwrap();
				let mut response = match request["method"].as_str().unwrap() {
					"thread/read" | "thread/resume" =>
						json!({"result":{"thread":{"id":"voice-thread","cwd":"/tmp","turns":[]}}}),
					"config/read" if fails =>
						json!({"error":{"code":-32603,"message":"unavailable"}}),
					"config/read" => json!({"result":{"config":{"realtime":{"voice":voice}}}}),
					"thread/realtime/start" => json!({"result":{}}),
					other => panic!("unexpected native method: {other}"),
				};
				sent.send(request.clone()).unwrap();
				response["id"] = request["id"].clone();
				if write.write_all(format!("{response}\n").as_bytes()).await.is_err() {
					break;
				}
			}
		});
		let owned = OwnedReviewer::new(home.path(), &client, "voice-thread", "turn").await;
		let mut chief = ChiefCoordinator::new(
			owned.store.clone(),
			client,
			ChiefConfig::new(
				"gpt-5.6-sol".into(),
				"high".into(),
				home.path().display().to_string(),
			),
		)
		.unwrap();
		chief.attach_voice_host(GENERATION.into(), crate::chief_voice::VoiceGateway::new());
		let result = chief
			.voice_request(ChiefVoiceRequest::Start {
				session_id: EntityId::new("voice").unwrap(),
				work_id: EntityId::new("root").unwrap(),
				offer: VoiceSdp::new("offer".into()).unwrap(),
			})
			.await;
		assert_eq!(result.is_err(), fails);
		let mut starts = Vec::new();
		while let Ok(request) = requests.try_recv() {
			if request["method"] == "thread/realtime/start" {
				starts.push(request);
			}
		}
		assert_eq!(starts.len(), usize::from(!fails));
		assert_eq!(owned.store.open_chief_voice_calls().await.unwrap().len(), usize::from(!fails));
		if let Some(start) = starts.first() {
			assert_eq!(
				start["params"]["voice"],
				if voice == "future_voice" { Value::Null } else { json!(voice) }
			);
		}
		server.abort();
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native effective voice configuration"]
async fn installed_native_voice_reads_project_override_and_refreshed_user_default() {
	use super::super::super::NativeSession;
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
	let home = tempfile::tempdir().unwrap();
	let project = home.path().join("project");
	std::fs::create_dir_all(project.join(".codex")).unwrap();
	let config = |voice: &str| {
		format!(
			"model=\"gpt-5.6-sol\"\n[realtime]\nvoice=\"{voice}\"\n[projects.{}]\ntrust_level=\"trusted\"\n",
			serde_json::to_string(&project).unwrap()
		)
	};
	std::fs::write(home.path().join("config.toml"), config("cove")).unwrap();
	std::fs::write(project.join(".codex/config.toml"), "[realtime]\nvoice=\"juniper\"\n").unwrap();
	let session = NativeSession::start(&binary, home.path());
	let started = session
		.client
		.thread_start(
			json!({"cwd":project,"ephemeral":true,"approvalPolicy":"never","sandbox":"read-only"}),
		)
		.await
		.unwrap();
	let thread = started["thread"]["id"].as_str().unwrap();
	assert_eq!(
		session.client.realtime_voice_for_thread(thread).await.unwrap().as_deref(),
		Some("juniper")
	);
	std::fs::remove_file(project.join(".codex/config.toml")).unwrap();
	std::fs::write(home.path().join("config.toml"), config("maple")).unwrap();
	assert_eq!(
		session.client.realtime_voice_for_thread(thread).await.unwrap().as_deref(),
		Some("maple")
	);
	std::fs::write(home.path().join("config.toml"), config("future_voice")).unwrap();
	// This installed server rejects an unknown configured enum; propagate its read failure.
	assert!(session.client.realtime_voice_for_thread(thread).await.is_err());
	std::fs::write(
		home.path().join("config.toml"),
		config("cove").replace("[realtime]\nvoice=\"cove\"\n", ""),
	)
	.unwrap();
	assert_eq!(
		session.client.realtime_voice_for_thread(thread).await.unwrap().as_deref(),
		Some("cove")
	);
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native voice preference bridge"]
async fn installed_voice_preference_bridge_saves_absent_file_and_rejects_old_connection() {
	use super::super::super::NativeSession;
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
	let home = tempfile::tempdir().unwrap();
	let session = NativeSession::start(&binary, home.path());
	let cwd = home.path().to_str().unwrap();
	let before = session.client.realtime_voice_settings(cwd).await.unwrap();
	assert_eq!(before.preference, None);
	let saved = session.client.write_realtime_voice(&before, "juniper").await.unwrap();
	assert_eq!(saved.preference.as_deref(), Some("juniper"));
	assert_eq!(saved.effective.as_deref(), Some("juniper"));
	drop(session);
	let session = NativeSession::start(&binary, home.path());
	assert!(session.client.write_realtime_voice(&saved, "maple").await.is_err());
	assert_eq!(
		session.client.realtime_voice_settings(cwd).await.unwrap().preference.as_deref(),
		Some("juniper")
	);
}
