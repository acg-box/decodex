//! Qualify an explicit sharing candidate without changing the production launch.

use std::{
	env, fs,
	path::Path,
	sync::{Arc, atomic::AtomicUsize},
};

use tokio::{net::TcpListener, time};

use crate::account_launch::{
	agent_process::native_tests::{self, Duration, NativeSession, ServerEvent},
	process::APP_SERVER_ARGS,
};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native MCP sharing"]
async fn explicit_sharing_candidate_uses_compatible_same_process_mcp_connections() {
	qualify_sharing(false).await;
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native MCP sharing"]
async fn explicit_sharing_keeps_unmarked_legacy_connection_private() {
	qualify_sharing(true).await;
}

async fn qualify_sharing(unmarked_legacy: bool) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(Path::new(&binary).is_absolute());
	let root = tempfile::tempdir().expect("temporary MCP sharing root");
	let first_home = root.path().join("first");
	let second_home = root.path().join("second");
	fs::create_dir(&first_home).expect("first fixture home");
	fs::create_dir(&second_home).expect("second fixture home");
	let fixture = root.path().join("mcp.py");
	fs::write(&fixture, include_str!("native_mcp_sharing.py")).expect("write MCP fixture");
	let fixture = serde_json::to_string(&fixture).expect("serialize MCP fixture path");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("fixture address");
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		Arc::new(AtomicUsize::new(0)),
		None,
		None,
		None,
		|serial| {
			if serial.is_multiple_of(2) {
				serde_json::json!({"type":"function_call","id":format!("call-{serial}"),"call_id":format!("call-{serial}"),"namespace":"mcp__modern","name":"identify","arguments":"{}"})
			} else {
				serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Done"}]})
			}
		},
	));
	let legacy_mode = if unmarked_legacy { "legacy-empty" } else { "legacy" };
	let legacy_env = if unmarked_legacy {
		""
	} else {
		"[mcp_servers.legacy.env]\nCODEX_MCP_PROTOCOL_VERSION = \"2025-06-18\"\n"
	};
	let config = format!(
		"thread_unload_delay_secs = 0\ncli_auth_credentials_store = \"file\"\nmodel=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n\
		[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n\
		[features]\ncode_mode=false\nenable_request_compression=false\n\
		[mcp_servers.modern]\ncommand = \"/usr/bin/python3\"\nargs = [{fixture}, \"modern\"]\n\
		[mcp_servers.legacy]\ncommand = \"/usr/bin/python3\"\nargs = [{fixture}, \"{legacy_mode}\"]\nrequired = true\n\
		{legacy_env}"
	);
	fs::write(first_home.join("config.toml"), &config).expect("first fixture config");
	fs::write(second_home.join("config.toml"), &config).expect("second fixture config");
	let sharing_args = APP_SERVER_ARGS
		.iter()
		.copied()
		.chain(["-c", "features.mcp_connection_sharing=true"])
		.collect::<Vec<_>>();

	time::timeout(Duration::from_secs(90), async {
		let baseline = NativeSession::with_arguments(
			&binary,
			&first_home,
			Default::default(),
			&APP_SERVER_ARGS,
		);
		let old_first = start(&baseline, &first_home, "baseline first").await;
		let old_second = start(&baseline, &first_home, "baseline second").await;
		assert_ne!(helper(&baseline, &old_first, "modern").await, helper(&baseline, &old_second, "modern").await);
		drop(baseline);

		let mut first = NativeSession::with_arguments(
			&binary,
			&first_home,
			Default::default(),
			&sharing_args,
		);
		let first_thread = start(&first, &first_home, "sharing first").await;
		let modern = helper(&first, &first_thread, "modern").await;
		let legacy = helper(&first, &first_thread, "legacy").await;
		let second_thread = start(&first, &first_home, "sharing second").await;
		assert_eq!(helper(&first, &second_thread, "modern").await, modern);
		assert_ne!(helper(&first, &second_thread, "legacy").await, legacy);
		assert_eq!(call_tool(&mut first, &first_thread).await, modern);
		assert_eq!(call_tool(&mut first, &second_thread).await, modern);

		first.client.request("thread/unsubscribe", serde_json::json!({"threadId":first_thread})).await.expect("unsubscribe first thread");
		while let Some(event) = first.events.recv().await {
			if matches!(event, ServerEvent::Notification { ref method, ref params } if method == "thread/closed" && params["threadId"] == first_thread) {
				break;
			}
		}
		assert_eq!(helper(&first, &second_thread, "modern").await, modern);

		let independent = NativeSession::with_arguments(
			&binary,
			&second_home,
			Default::default(),
			&sharing_args,
		);
		let independent_thread = start(&independent, &second_home, "independent process").await;
		assert_ne!(helper(&independent, &independent_thread, "modern").await, modern);
	})
	.await
	.expect("bounded native sharing fixture");
	backend.abort();
}

async fn call_tool(session: &mut NativeSession, thread: &str) -> u64 {
	let started = session
		.client
		.turn_start(serde_json::json!({
			"threadId":thread,"input":[{"type":"text","text":"Run the fixture tool."}]
		}))
		.await
		.expect("start the fixture turn");
	let turn = started["turn"]["id"].as_str().expect("native turn ID");
	let mut helper = None;
	while let Some(event) = session.events.recv().await {
		if let ServerEvent::Notification { method, params } = event {
			if method == "item/completed"
				&& params["threadId"] == thread
				&& params["turnId"] == turn
				&& params["item"]["type"] == "mcpToolCall"
			{
				assert_eq!(params["item"]["status"], "completed");
				helper = params["item"]["result"]["structuredContent"]["pid"].as_u64();
			}
			if method == "turn/completed"
				&& params["threadId"] == thread
				&& params["turn"]["id"] == turn
			{
				assert_eq!(params["turn"]["status"], "completed");
				return helper.expect("native MCP call returned the helper identity");
			}
		}
	}
	panic!("native connection closed before fixture turn completed")
}

async fn start(session: &NativeSession, cwd: &Path, phase: &str) -> String {
	let response = session
		.client
		.thread_start(serde_json::json!({
			"cwd":cwd,"approvalPolicy":"never","sandbox":"read-only"
		}))
		.await
		.unwrap_or_else(|error| panic!("start {phase} fixture thread: {error:?}"));
	response["thread"]["id"].as_str().expect("native thread ID").to_owned()
}

async fn helper(session: &NativeSession, thread: &str, server: &str) -> u64 {
	loop {
		// Supplying serverName is essential: it reads the owning thread's live
		// connection, rather than launching the global inventory probe.
		let response = session
			.client
			.request(
				"mcpServerStatus/list",
				serde_json::json!({
					"threadId":thread,"serverName":server,"detail":"full","limit":100,"cursor":null
				}),
			)
			.await
			.expect("read native thread MCP status");
		let row = response["data"]
			.as_array()
			.expect("MCP status rows")
			.iter()
			.find(|row| row["name"] == server);
		if let Some(row) = row.filter(|row| row["runtimeStatus"] == "connected") {
			assert!(row["tools"].as_object().expect("MCP tool catalog").contains_key("identify"));
			return row["serverInfo"]["name"]
				.as_str()
				.expect("MCP server identity")
				.strip_prefix("sharing-fixture-")
				.expect("fixture server identity")
				.parse()
				.expect("fixture helper process ID");
		}
		time::sleep(Duration::from_millis(10)).await;
	}
}
