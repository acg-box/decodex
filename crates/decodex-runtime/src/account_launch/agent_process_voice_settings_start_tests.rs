//! Live calls use the account voice selected by the native backend.
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
	agent_voice_settings,
};
use decodex_database::AgentDispatchState;
use decodex_protocol::{
	AgentVoiceOptions, AgentVoiceRequest, EntityId, HistoryText, VoiceSdp, WireText,
};

fn assert_voice_start(start: &Value, thread: &str, advanced: bool) {
	assert_eq!(start["params"]["threadId"], thread);
	assert_eq!(start["params"]["version"], "v3");
	assert_eq!(start["params"]["realtimeSessionId"], "voice");
	assert_eq!(start["params"]["transport"]["type"], "webrtc");
	assert_eq!(start["params"]["outputModality"], "audio");
	assert_eq!(start["params"]["backendReasoningStatus"], true);

	if advanced {
		assert_eq!(start["params"]["model"], "realtime-fixture");
		assert_eq!(start["params"]["realtimeStartInstructions"], "Start fixture");
		assert_eq!(start["params"]["realtimeEndInstructions"], "");
	} else {
		for field in ["model", "realtimeStartInstructions", "realtimeEndInstructions"] {
			assert!(start["params"].get(field).is_none());
		}
	}

	assert!(start["params"].get("voice").is_none());
}

fn voice_options(advanced: bool) -> AgentVoiceOptions {
	if advanced {
		AgentVoiceOptions {
			draft: None,
			model: Some(
				WireText::new("realtime-fixture").expect("Use the bounded fixture model name."),
			),
			start_instructions: Some(
				HistoryText::new("Start fixture")
					.expect("Use the bounded fixture start instruction."),
			),
			end_instructions: Some(
				HistoryText::new("").expect("Use the empty fixture end instruction."),
			),
		}
	} else {
		Default::default()
	}
}

fn start_options(advanced: bool, unstarted: bool) -> AgentVoiceOptions {
	let mut options = voice_options(advanced);
	if unstarted {
		options.draft = Some(Box::new(decodex_protocol::AgentVoiceDraft {
			execution: decodex_protocol::AgentExecutionOverrides {
				model: Some(
					decodex_protocol::ConversationModel::new("selected-voice-agent-model")
						.expect("fixture model"),
				),
				reasoning_effort: Some(decodex_protocol::ConversationReasoningEffort::Medium),
				service_tier: Some(decodex_protocol::ServiceTier::priority()),
				permission_mode: Some(decodex_protocol::PermissionMode::ReadOnly),
			},
			..Default::default()
		}));
	}
	options
}

#[tokio::test]
async fn voice_start_uses_native_account_selection_and_honors_managed_policy() {
	for (advanced, policy, hosted, occupied, unstarted) in [
		(true, None, false, false, false),
		(false, Some(true), false, false, false),
		(true, None, true, false, false),
		(true, None, true, true, false),
		(true, Some(false), false, false, false),
		(true, None, true, false, true),
		(true, Some(false), true, false, true),
	] {
		let thread = if hosted { "hosted-voice-thread" } else { "voice-thread" };
		let work = if hosted { "conversation" } else { "root" };
		let fails = policy == Some(false) || occupied;
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
					"model/list" => serde_json::json!({"result":{"data":[],"nextCursor":null}}),
					"thread/start" => serde_json::json!({"result":{
						"thread":{"id":thread},"model":request["params"]["model"],
						"reasoningEffort":request["params"]["config"]["model_reasoning_effort"]
					}}),
					"thread/name/set" => serde_json::json!({"result":{}}),
					"thread/resume" if occupied =>
						serde_json::json!({"error":{"code":-32_600,"message":format!("thread {thread} already has an active writer")}}),
					"thread/read" | "thread/resume" =>
						serde_json::json!({"result":{"thread":{"id":thread,"cwd":"/tmp","historyMode":"legacy","turns":[]}}}),
					"configRequirements/read" =>
						serde_json::json!({"result":{"requirements":policy.map(|allowed| serde_json::json!({"featureRequirements":{"in_app_voice":allowed}}))}}),
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

		if unstarted {
			reserve_hosted_voice(&owned, work).await;
		} else if hosted {
			bind_hosted_voice(&owned, work, thread).await;
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
				options: start_options(advanced, unstarted),
			})
			.await;

		assert_eq!(result.is_err(), fails, "voice start: {result:?}");

		let mut starts = Vec::new();
		let mut thread_starts = 0;

		while let Ok(request) = requests.try_recv() {
			assert_ne!(request["method"], "turn/start", "Live waits for spoken input");
			thread_starts += usize::from(request["method"] == "thread/start");
			if unstarted && request["method"] == "thread/start" {
				assert_eq!(request["params"]["model"], "selected-voice-agent-model");
				assert_eq!(request["params"]["config"]["model_reasoning_effort"], "medium");
				assert_eq!(request["params"]["serviceTier"], "priority");
				assert_eq!(request["params"]["sandbox"], "read-only");
			}
			if request["method"] == "thread/realtime/start" {
				starts.push(request);
			}
		}

		assert_eq!(starts.len(), usize::from(!fails));
		assert_eq!(thread_starts, usize::from(unstarted && !fails));
		assert_eq!(owned.store.open_agent_voice_calls().await.unwrap().len(), usize::from(!fails));

		let blocked = owned.store.list_agent_thread_in_use_events().await.unwrap();

		assert_eq!(blocked.len(), usize::from(occupied));

		if occupied {
			assert_eq!(blocked[0].work_item_id, work);
		}

		if let Some(start) = starts.first() {
			assert_voice_start(start, thread, advanced);
		}

		server.abort();
	}
}

async fn bind_hosted_voice(owned: &OwnedReviewer, work: &str, thread: &str) {
	reserve_hosted_voice(owned, work).await;
	owned.store.bind_agent_thread(work.into(), thread.into()).await.expect("voice thread");

	assert!(
		!owned
			.store
			.agent_thread_is_owned(work.into(), thread.into(), Some(GENERATION.into()))
			.await
			.expect("voice ownership")
	);
}

async fn reserve_hosted_voice(owned: &OwnedReviewer, work: &str) {
	let mut conversation = owned.store.get_agent_work_item("root".into()).await.expect("root");

	conversation.id = work.into();
	conversation.parent_goal_id = Some("root".into());
	conversation.codex_thread_id = None;
	conversation.active_turn_id = None;
	conversation.dispatch_state = AgentDispatchState::Idle;

	owned.store.create_agent_manager(conversation).await.expect("hosted conversation");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated signed-out account"]
async fn installed_account_voice_requires_chatgpt_even_with_local_preference() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
	let home = tempfile::tempdir().unwrap();
	fs::write(home.path().join("config.toml"), "[realtime]\nvoice=\"juniper\"\n").unwrap();
	let session = NativeSession::start(&binary, home.path());
	assert!(matches!(
		session.client.realtime_voice_settings().await,
		Err(decodex_codex::app_server_client::ClientError::Remote(_))
	));
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

		assert!(agent_voice_settings::voice_allowed(&session.client).await.unwrap());
		assert_eq!(fs::read_to_string(home.path().join("config.toml")).unwrap(), config);
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; synthetic ChatGPT account on loopback"]
async fn installed_account_voice_is_shared_between_projects_after_explicit_save() {
	use crate::account_launch::agent_process::native_tests::account_nudge::write_fixture_credentials;
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let (writes, mut received) = mpsc::unbounded_channel();
	let backend = tokio::spawn(async move {
		let mut selected = "cove";
		while let Ok((stream, _)) = listener.accept().await {
			let mut stream = BufReader::new(stream);
			let mut request = String::new();
			stream.read_line(&mut request).await.unwrap();
			let mut header = String::new();
			loop {
				header.clear();
				if stream.read_line(&mut header).await.unwrap() == 0 || header == "\r\n" {
					break;
				}
			}
			let (status, body) = match request
				.split_whitespace()
				.take(2)
				.collect::<Vec<_>>()
				.as_slice()
			{
				["GET", "/backend-api/settings/voices?voice_mode=wingman"] => (
					"200 OK",
					serde_json::json!({"selected":selected,"voices":[{"voice":"cove","name":"Cove"},{"voice":"account-option","name":"Account display name"}]}),
				),
				[
					"PATCH",
					"/backend-api/settings/account_user_setting?feature=voice_name&value=account-option",
				] => {
					selected = "account-option";
					writes.send(()).unwrap();
					("200 OK", serde_json::json!({}))
				},
				_ => ("404 Not Found", Value::Null),
			};
			let body = body.to_string();
			let response = format!(
				"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
				body.len()
			);
			let _ = stream.get_mut().write_all(response.as_bytes()).await;
		}
	});
	// Each project uses its own native home and a different legacy local preference.
	let homes = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
	for (home, legacy) in homes.iter().zip(["juniper", "maple"]) {
		write_fixture_credentials(home.path());
		fs::write(home.path().join("config.toml"), format!("chatgpt_base_url=\"http://{address}\"\ncli_auth_credentials_store=\"file\"\n[realtime]\nvoice=\"{legacy}\"\n")).unwrap();
	}

	let first = NativeSession::start(&binary, homes[0].path());
	let second = NativeSession::start(&binary, homes[1].path());
	let before = first.client.realtime_voice_settings().await.unwrap();
	assert_eq!(before.selected, "cove");
	assert_eq!(before.voices[1].name, "Account display name");
	assert_eq!(second.client.realtime_voice_settings().await.unwrap().selected, "cove");
	assert!(second.client.write_realtime_voice(&before, "account-option").await.is_err());
	assert!(received.try_recv().is_err());
	let saved = first.client.write_realtime_voice(&before, "account-option").await.unwrap();
	assert_eq!(saved.selected, "account-option");
	assert_eq!(second.client.realtime_voice_settings().await.unwrap().selected, "account-option");
	assert!(received.try_recv().is_ok());
	assert!(received.try_recv().is_err());
	backend.abort();
}
