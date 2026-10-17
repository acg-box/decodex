//! Effective configuration is read before creating a native voice session.
use std::{env, fs};

use serde_json::{self, Value};
use tokio::{
	io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
	sync::mpsc,
};

use crate::{
	account_launch::agent_process::native_tests::{
		NativeSession,
		reviewer::store::{AppServerClient, GENERATION, OwnedReviewer},
	},
	agent::{AgentConfig, AgentCoordinator},
	agent_voice::VoiceGateway,
};
use decodex_protocol::{
	AgentVoiceOptions, AgentVoiceRequest, EntityId, HistoryText, VoiceSdp, WireText,
};

#[tokio::test]
async fn voice_start_applies_effective_voice_and_rejects_failed_reads_before_recording_call() {
	for (voice, config_fails, policy, hosted, occupied) in [
		("juniper", false, None, false, false),
		("juniper", false, None, true, false),
		("juniper", false, None, true, true),
		("aube", false, None, false, false),
		("future_voice", false, Some(true), false, false),
		("juniper", true, None, false, false),
		("juniper", false, Some(false), false, false),
	] {
		let thread = if hosted { "hosted-voice-thread" } else { "voice-thread" };
		let work = if hosted { "conversation" } else { "root" };
		let fails = config_fails || policy == Some(false) || occupied;
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = io::duplex(8_192);
		let (read, write) = io::split(local);
		let (client, _events) = AppServerClient::from_io(read, write);
		let (sent, mut requests) = mpsc::unbounded_channel();
		let server = tokio::spawn(async move {
			let (read, mut write) = io::split(remote);
			let mut lines = BufReader::new(read).lines();

			while let Some(line) = lines.next_line().await.unwrap() {
				let request: Value = serde_json::from_str(&line).unwrap();
				let mut response = match request["method"].as_str().unwrap() {
					"thread/resume" if occupied =>
						serde_json::json!({"error":{"code":-32_600,"message":format!("thread {thread} already has an active writer")}}),
					"thread/read" | "thread/resume" =>
						serde_json::json!({"result":{"thread":{"id":thread,"cwd":"/tmp","turns":[]}}}),
					"config/read" if config_fails =>
						serde_json::json!({"error":{"code":-32_603,"message":"unavailable"}}),
					"config/read" =>
						serde_json::json!({"result":{"config":{"realtime":{"voice":voice}}}}),
					"configRequirements/read" =>
						serde_json::json!({"result":{"requirements":policy.map(|allowed| serde_json::json!({"featureRequirements":{"in_app_voice":allowed}}))}}),
					"thread/realtime/listVoices" =>
						serde_json::json!({"result":{"voices":{"v1":["juniper"],"v3":["juniper","aube"],"defaultV1":"juniper"}}}),
					"thread/realtime/start" => serde_json::json!({"result":{}}),
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
		if hosted {
			let mut conversation = owned.store.get_agent_work_item("root".into()).await.unwrap();
			conversation.id = work.into();
			conversation.parent_goal_id = Some("root".into());
			conversation.codex_thread_id = None;
			conversation.active_turn_id = None;
			conversation.dispatch_state = decodex_database::AgentDispatchState::Idle;
			owned.store.create_agent_manager(conversation).await.unwrap();
			owned.store.bind_agent_thread(work.into(), thread.into()).await.unwrap();
			assert!(
				!owned
					.store
					.agent_thread_is_owned(work.into(), thread.into(), Some(GENERATION.into()))
					.await
					.unwrap()
			);
		}
		let mut agent = AgentCoordinator::new(
			owned.store.clone(),
			client,
			AgentConfig::new(
				"gpt-5.6-sol".into(),
				"high".into(),
				home.path().display().to_string(),
			),
		)
		.unwrap();

		agent.attach_voice_host(GENERATION.into(), VoiceGateway::new());

		let result = agent
			.voice_request(AgentVoiceRequest::Start {
				session_id: EntityId::new("voice").unwrap(),
				work_id: EntityId::new(work).unwrap(),
				offer: VoiceSdp::new("offer".into()).unwrap(),
				options: if voice == "juniper" {
					AgentVoiceOptions {
						model: Some(WireText::new("realtime-fixture").unwrap()),
						start_instructions: Some(HistoryText::new("Start fixture").unwrap()),
						end_instructions: Some(HistoryText::new("").unwrap()),
					}
				} else {
					Default::default()
				},
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
		assert_eq!(owned.store.open_agent_voice_calls().await.unwrap().len(), usize::from(!fails));
		let blocked = owned.store.list_agent_thread_in_use_events().await.unwrap();
		assert_eq!(blocked.len(), usize::from(occupied));
		if occupied {
			assert_eq!(blocked[0].work_item_id, work);
		}

		if let Some(start) = starts.first() {
			assert_voice_start(start, thread, voice);
		}

		server.abort();
	}
}

fn assert_voice_start(start: &Value, thread: &str, voice: &str) {
	assert_eq!(start["params"]["threadId"], thread);
	assert_eq!(start["params"]["version"], "v3");
	assert_eq!(start["params"]["realtimeSessionId"], "voice");
	assert_eq!(start["params"]["transport"]["type"], "webrtc");
	assert_eq!(start["params"]["outputModality"], "audio");
	if voice == "juniper" {
		assert_eq!(start["params"]["model"], "realtime-fixture");
		assert_eq!(start["params"]["realtimeStartInstructions"], "Start fixture");
		assert_eq!(start["params"]["realtimeEndInstructions"], "");
	} else {
		for field in ["model", "realtimeStartInstructions", "realtimeEndInstructions"] {
			assert!(start["params"].get(field).is_none());
		}
	}
	assert_eq!(
		start["params"]["voice"],
		if voice == "future_voice" { Value::Null } else { serde_json::json!(voice) }
	);
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native effective voice configuration"]
async fn installed_native_voice_reads_project_override_and_refreshed_user_default() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
	let home = tempfile::tempdir().unwrap();
	let project = home.path().join("project");

	fs::create_dir_all(project.join(".codex")).unwrap();

	let config = |voice: &str| {
		format!(
			"model=\"gpt-5.6-sol\"\n[realtime]\nvoice=\"{voice}\"\n[projects.{}]\ntrust_level=\"trusted\"\n",
			serde_json::to_string(&project).unwrap()
		)
	};

	fs::write(home.path().join("config.toml"), config("cove")).unwrap();
	fs::write(project.join(".codex/config.toml"), "[realtime]\nvoice=\"juniper\"\n").unwrap();

	let session = NativeSession::start(&binary, home.path());
	let started = session
		.client
		.thread_start(
			serde_json::json!({"cwd":project,"ephemeral":true,"approvalPolicy":"never","sandbox":"read-only"}),
		)
		.await
		.unwrap();
	let thread = started["thread"]["id"].as_str().unwrap();

	assert_eq!(
		session.client.realtime_voice_for_thread(thread).await.unwrap().as_deref(),
		Some("juniper")
	);

	fs::remove_file(project.join(".codex/config.toml")).unwrap();
	fs::write(home.path().join("config.toml"), config("maple")).unwrap();

	assert_eq!(
		session.client.realtime_voice_for_thread(thread).await.unwrap().as_deref(),
		Some("maple")
	);

	fs::write(home.path().join("config.toml"), config("future_voice")).unwrap();

	// This installed server rejects an unknown configured enum; propagate its read failure.
	assert!(session.client.realtime_voice_for_thread(thread).await.is_err());

	fs::write(
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
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
	let home = tempfile::tempdir().unwrap();
	let session = NativeSession::start(&binary, home.path());
	let cwd = home.path().to_str().unwrap();
	let before = session.client.realtime_voice_settings(cwd).await.unwrap();

	assert_eq!(before.preference, None);
	assert!(before.voices.iter().any(|voice| voice == "juniper"));

	let saved = session.client.write_realtime_voice(&before, "juniper").await.unwrap();

	assert_eq!(saved.preference.as_deref(), Some("juniper"));
	assert_eq!(saved.effective.as_deref(), Some("juniper"));
	assert!(matches!(
		session.client.write_realtime_voice(&before, "maple").await,
		Err(decodex_codex::app_server_client::ClientError::Remote(_))
	));

	drop(session);

	let session = NativeSession::start(&binary, home.path());

	assert!(session.client.write_realtime_voice(&saved, "maple").await.is_err());

	let cold = session.client.realtime_voice_settings(cwd).await.unwrap();

	assert_eq!(cold.preference.as_deref(), Some("juniper"));
	assert_eq!(cold.effective.as_deref(), Some("juniper"));
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; native requirements read without managed policy"]
async fn installed_native_voice_policy_read_does_not_use_user_feature_flags() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
	for enabled in [false, true] {
		let home = tempfile::tempdir().unwrap();
		let config = format!("[features]\nin_app_voice = {enabled}\n");
		fs::write(home.path().join("config.toml"), &config).unwrap();
		let session = NativeSession::start(&binary, home.path());
		assert!(crate::agent_voice_settings::voice_allowed(&session.client).await.unwrap());
		assert_eq!(fs::read_to_string(home.path().join("config.toml")).unwrap(), config);
	}
}
