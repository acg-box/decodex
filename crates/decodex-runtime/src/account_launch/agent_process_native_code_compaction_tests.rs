//! Installed Code Mode observations must reach compaction and cold continuation.
use std::{env, fs, sync::Mutex};

use serde_json::Value;
use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::{
	native_tests,
	native_tests::compaction::{self, Arc, AtomicUsize, Duration, NativeSession, SUMMARY},
};

fn assert_compaction_metadata(requests: &[Value], code: &str) {
	assert_eq!(requests.len(), 4);

	let find = |index: usize| {
		requests[index]["input"]
			.as_array()
			.expect("captured request input is an array")
			.iter()
			.find(|i| i["type"] == "custom_tool_call_output" && i["call_id"] == "code-cell")
			.expect("captured request contains Code Mode output")
	};
	let sampled = find(1);

	if code.starts_with("throw") {
		assert!(sampled["output"].to_string().contains("empty-cell-error"));
	} else if code.contains("ALL_TOOLS") {
		assert!(sampled["output"].to_string().contains("mcp__fixture__hold"));
	}

	let compacted = find(2);
	let metadata = &sampled["internal_chat_message_metadata_passthrough"];

	assert_eq!(metadata["tool_calls_complete"], true);

	let calls =
		metadata["executed_tool_calls"].as_array().expect("completed tool inventory is an array");

	if code.starts_with("text(await") {
		assert_eq!(calls.len(), 1);
		assert!(calls[0]["name"].as_str().expect("executed tool has a name").contains("hold"));
	} else {
		assert!(calls.is_empty(), "completed discovery/error has an explicit empty inventory");
	}

	assert_eq!(compacted["internal_chat_message_metadata_passthrough"], *metadata);
	assert!(
		requests[3]["input"]
			.as_array()
			.expect("post-compaction input is an array")
			.iter()
			.any(|i| i["type"] == "compaction" && i["encrypted_content"] == SUMMARY)
	);
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY and packaged Code Mode helper"]
async fn installed_native_code_mode_metadata_reaches_compaction() {
	for code in [
		"text(await tools.mcp__fixture__hold({}));",
		"text(ALL_TOOLS.map(tool => tool.name));",
		"throw new Error(\"empty-cell-error\");",
	] {
		let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
		let home = tempfile::tempdir().unwrap();
		let fixture = home.path().join("mcp.py");
		let gate = home.path().join("ready");

		fs::write(&fixture, include_str!("native_mcp_delayed.py")).unwrap();
		fs::write(&gate, b"ready").unwrap();

		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let address = listener.local_addr().unwrap();
		let bodies = Arc::new(Mutex::new(Vec::new()));
		let backend = tokio::spawn(native_tests::serve_fixture_usage(
			listener,
			Arc::new(AtomicUsize::new(0)),
			None,
			Some(bodies.clone()),
			|serial| {
				let tokens = if serial == 1 { 250_000 } else { 100 };

				serde_json::json!({"input_tokens":tokens,"output_tokens":0,"total_tokens":tokens})
			},
			move |serial| match serial {
				0 =>
					serde_json::json!({"type":"custom_tool_call","name":"exec","call_id":"code-cell","id":"code-item","input":code}),
				2 => serde_json::json!({"type":"compaction","encrypted_content":SUMMARY}),
				_ =>
					serde_json::json!({"type":"message","role":"assistant","id":format!("reply-{serial}"),"content":[{"type":"output_text","text":"Done"}]}),
			},
		));

		fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\nmodel_auto_compact_token_limit=200000\ncli_auth_credentials_store=\"file\"\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[features]\ncode_mode=true\nexecuted_tool_call_metadata=true\nremote_compaction_v2=false\nenable_request_compression=false\n[mcp_servers.fixture]\ncommand=\"/usr/bin/python3\"\nargs=[{},{}]\n",serde_json::json!(fixture),serde_json::json!(gate))).unwrap();

		let mut session = NativeSession::start(&binary, home.path());

		time::timeout(Duration::from_secs(60), async {
			let started = session
				.client
				.thread_start(
					serde_json::json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}),
				)
				.await
				.unwrap();
			let thread = started["thread"]["id"].as_str().unwrap().to_owned();

			compaction::start_turn(
				&session.client,
				&thread,
				"Run the fixture tool through Code Mode.",
			)
			.await;

			assert!(compaction::completed(&mut session, &thread).await.is_empty());

			compaction::start_turn(&session.client, &thread, "Continue through compaction.").await;

			assert_eq!(compaction::completed(&mut session, &thread).await.len(), 2);

			{
				let requests = bodies.lock().unwrap();

				assert_compaction_metadata(&requests, code);
			}

			drop(session);

			let mut cold = NativeSession::start(&binary, home.path());

			cold.client
				.thread_resume(serde_json::json!({"threadId":thread,"excludeTurns":true}))
				.await
				.unwrap();

			compaction::start_turn(&cold.client, &thread, "Continue after restart.").await;

			assert!(compaction::completed(&mut cold, &thread).await.is_empty());

			let requests = bodies.lock().unwrap();

			assert_eq!(requests.len(), 5);
			assert!(
				requests[4]["input"]
					.as_array()
					.unwrap()
					.iter()
					.any(|i| i["type"] == "compaction" && i["encrypted_content"] == SUMMARY)
			);
		})
		.await
		.expect("Code Mode compaction deadline");

		backend.abort();
	}
}
