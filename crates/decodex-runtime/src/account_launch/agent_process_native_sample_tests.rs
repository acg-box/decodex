//! Sampling uses the selected CLI and preserves parent history for text and tool responses.
use std::{
	env, fs,
	sync::{
		Arc, Mutex,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::{self, NativeSession, ServerEvent};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native sampling"]
async fn installed_native_sampling_preserves_parent_and_blocks_tools_without_replay() {
	for mode in ["legacy", "paginated"] {
		time::timeout(Duration::from_secs(45), qualify(mode)).await.expect("sampling deadline");
	}
}

async fn qualify(mode: &str) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().expect("isolated home");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("native sample fixture");
	let address = listener.local_addr().expect("native sample fixture");
	let calls = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let forbidden_file = home.path().join("sample-must-not-write.txt");
	let forbidden_patch = format!(
		"*** Begin Patch\n*** Add File: {}\n+must not execute\n*** End Patch",
		forbidden_file.display()
	);
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		None,
		move |index| {
			if index == 2 {
				serde_json::json!({"type":"function_call","id":"sample-tool-item","call_id":"sample-tool-call","name":"sample_fixture_tool","arguments":"{}"})
			} else if index == 3 {
				serde_json::json!({"type":"custom_tool_call","id":"sample-custom-item","call_id":"sample-custom-call","name":"apply_patch","input":forbidden_patch.clone()})
			} else {
				serde_json::json!({"type":"message","id":format!("message-{index}"),"role":"assistant","content":[{"type":"output_text","text":"Another perspective from inherited context."}]})
			}
		},
	));
	let configuration = format!(
		"model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n"
	);
	fs::write(home.path().join("config.toml"), &configuration).expect("native sample fixture");
	let mut session = NativeSession::start(&binary, home.path());
	let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":mode,"approvalPolicy":"never","sandbox":"workspace-write","dynamicTools":[{"name":"sample_fixture_tool","description":"Must not execute during sampling","inputSchema":{"type":"object","properties":{}}}]})).await.expect("native sample fixture");
	let parent = started["thread"]["id"].as_str().expect("native sample fixture").to_owned();
	session.client.turn_start(serde_json::json!({"threadId":parent,"input":[{"type":"text","text":"PARENT_SAMPLE_CONTEXT_MARKER"}]})).await.expect("native sample fixture");
	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("native sample fixture")
			&& method == "turn/completed"
			&& params["threadId"] == parent
		{
			assert_eq!(params["turn"]["status"], "completed");
			break;
		}
	}
	let before =
		session.client.thread_markdown_transcript(&parent).await.expect("native sample fixture");
	let turn = session
		.client
		.thread_latest_turn_id(&parent)
		.await
		.expect("native sample fixture")
		.expect("native sample fixture");
	for tool in [false, true, true] {
		let guard = session
			.client
			.history_guard(session.client.history_revision())
			.expect("native sample fixture");
		let result = session
			.client
			.sample_thread(&parent, &turn, "Give another perspective.", guard)
			.await
			.expect("native sample fixture");
		assert_eq!(result.source_turn_id, turn);
		if tool {
			assert!(result.text.is_none(), "tool calls must not become ordinary text or execution");
		} else {
			assert_eq!(result.text.as_deref(), Some("Another perspective from inherited context."));
		}
	}
	assert!(!forbidden_file.exists(), "sampled patch tools must not write to the parent workspace");
	let after =
		session.client.thread_markdown_transcript(&parent).await.expect("native sample fixture");
	assert_eq!(before, after, "parent conversation must stay unchanged");
	assert_eq!(
		calls.load(Ordering::SeqCst),
		4,
		"one parent turn and three explicit samples, with no replay"
	);
	let listed = session
		.client
		.request("thread/list", serde_json::json!({"limit":100}))
		.await
		.expect("native sample fixture");
	assert_eq!(
		listed["data"].as_array().expect("native sample fixture").len(),
		1,
		"private samples must not enter durable history"
	);
	while let Ok(Some(event)) =
		time::timeout(Duration::from_millis(20), session.events.recv()).await
	{
		assert!(
			!matches!(event, ServerEvent::Request { .. }),
			"sample tool requests must never reach the application"
		);
	}
	let captured = bodies.lock().expect("native sample fixture");
	for request in captured.iter().skip(1) {
		assert!(request["input"].to_string().contains("PARENT_SAMPLE_CONTEXT_MARKER"));
		assert_eq!(request["model"], captured[0]["model"]);
	}
	let unchanged =
		fs::read_to_string(home.path().join("config.toml")).expect("native sample fixture");
	assert_eq!(unchanged, configuration, "sampling must preserve native configuration");
	backend.abort();
}
