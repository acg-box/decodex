//! Exercise the installed Code Mode helper through the retained native bridge.
use std::{
	env, fs,
	sync::{Mutex, atomic::AtomicUsize},
};

use serde_json::Value;
use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::{
	self, Arc, Duration, NativeSession, Ordering, ServerEvent,
};

fn assert_delayed_call_history(
	before: &Value,
	after: &Value,
	item_ids: &[String],
	first_turn: &str,
) {
	for history in [before, after] {
		let turns = history["turns"].as_array().expect("history contains turns");

		assert_eq!(turns.len(), 2);

		for item_id in item_ids {
			let owners = turns
				.iter()
				.filter(|turn| {
					turn["items"]
						.as_array()
						.expect("history turn contains items")
						.iter()
						.any(|item| item["id"] == *item_id)
				})
				.collect::<Vec<_>>();

			assert_eq!(owners.len(), 1);
			assert_eq!(owners[0]["id"], first_turn);
		}

		assert!(history["nextCursor"].is_null());
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY and its packaged Code Mode helper"]
async fn installed_native_code_mode_yielded_cells_keep_their_outputs() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		Some(serde_json::json!({"input_tokens":1,"output_tokens":1,"total_tokens":2})),
		|serial| match serial {
			0 =>
				serde_json::json!({"type":"custom_tool_call","name":"exec","call_id":"first-cell","input":"// @exec: {\"yield_time_ms\": 1}\nclearTimeout(setTimeout(() => text('cancelled timer output'), 25)); await new Promise(resolve => setTimeout(resolve, 500)); notify('first notice'); text('first output');"}),
			1 =>
				serde_json::json!({"type":"custom_tool_call","name":"exec","call_id":"second-cell","input":"notify('second notice'); text('second output'); setTimeout(() => text('late timer output'), 25);"}),
			2 =>
				serde_json::json!({"type":"function_call","name":"wait","call_id":"first-wait","arguments":"{\"cell_id\":\"1\",\"yield_time_ms\":10000}"}),
			_ =>
				serde_json::json!({"type":"message","role":"assistant","id":"done","content":[{"type":"output_text","text":"Done"}]}),
		},
	));

	fs::write(home.path().join("config.toml"), format!(
		"model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[features]\ncode_mode=true\nenable_request_compression=false\n"
	)).unwrap();

	let mut session = NativeSession::start(&binary, home.path());

	time::timeout(Duration::from_secs(30), async {
		let start = session
			.client
			.thread_start(serde_json::json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}))
			.await
			.unwrap();
		let thread = start["thread"]["id"].as_str().unwrap();

		session
			.client
			.turn_start(
				serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Run the local Code Mode fixture"}]}),
			)
			.await
			.unwrap();

		loop {
			if let ServerEvent::Notification { method, params } =
				session.events.recv().await.unwrap()
				&& method == "turn/completed"
			{
				assert_eq!(params["turn"]["status"], "completed");

				break;
			}
		}
	})
	.await
	.unwrap();

	assert_eq!(calls.load(Ordering::Acquire), 4);

	let captured = bodies.lock().unwrap();

	for (index, call, expected, excluded) in [
		(1, "first-cell", "Script running with cell ID 1", "second output"),
		(2, "second-cell", "second output", "first output"),
		(3, "first-wait", "first output", "second output"),
	] {
		let output = captured[index]["input"]
			.as_array()
			.unwrap()
			.iter()
			.filter(|item| {
				item["call_id"] == call
					&& matches!(
						item["type"].as_str(),
						Some("custom_tool_call_output" | "function_call_output")
					)
			})
			.map(|item| serde_json::to_string(&item["output"]).unwrap())
			.collect::<Vec<_>>()
			.join("\n");

		assert!(output.contains(expected), "missing {expected}: {output}");
		assert!(!output.contains(excluded), "another cell's output reached {call}");
	}
	for (call, notice) in [("first-cell", "first notice"), ("second-cell", "second notice")] {
		assert!(
			captured[3]["input"]
				.as_array()
				.unwrap()
				.iter()
				.any(|item| item["type"] == "custom_tool_call_output"
					&& item["call_id"] == call
					&& item["output"] == notice),
			"notification missing from its originating call: {call}"
		);
	}
	// The first cell remains alive past both deadlines. Neither a cleared timer nor
	// a timer belonging to the completed second cell may append output to history.
	for item in captured[3]["input"].as_array().unwrap() {
		if matches!(item["type"].as_str(), Some("custom_tool_call_output" | "function_call_output"))
		{
			let output = serde_json::to_string(&item["output"]).unwrap();

			assert!(!output.contains("cancelled timer output"), "cleared timer fired: {output}");
			assert!(!output.contains("late timer output"), "completed cell timer fired: {output}");
		}
	}

	drop(captured);
	drop(session);

	backend.abort();
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY and its packaged Code Mode helper"]
async fn installed_native_delayed_mcp_keeps_original_turn_after_restart() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let gate = home.path().join("release");
	let fixture = home.path().join("delayed.py");

	fs::write(&fixture, include_str!("native_mcp_delayed.py")).unwrap();

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		Some(serde_json::json!({"input_tokens":1,"output_tokens":1,"total_tokens":2})),
		|serial| match serial {
			0 =>
				serde_json::json!({"type":"custom_tool_call","name":"exec","call_id":"delayed-cell","id":"originating-code-cell","input":"// @exec: {\"yield_time_ms\": 1}\ntext(await tools.mcp__fixture__hold({})); text(await tools.mcp__fixture__hold({}));"}),
			2 =>
				serde_json::json!({"type":"function_call","name":"wait","call_id":"wait-delayed","arguments":"{\"cell_id\":\"1\",\"yield_time_ms\":10000}"}),
			_ =>
				serde_json::json!({"type":"message","role":"assistant","id":format!("done-{serial}"),"content":[{"type":"output_text","text":"Done"}]}),
		},
	));

	fs::write(home.path().join("config.toml"), format!(
        "model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[features]\ncode_mode=true\nenable_request_compression=false\n[mcp_servers.fixture]\ncommand=\"/usr/bin/python3\"\nargs=[{},{}]\n",
        serde_json::to_string(&fixture).unwrap(), serde_json::to_string(&gate).unwrap()
    )).unwrap();

	let mut session = NativeSession::start(&binary, home.path());
	let (thread, first_turn, item_ids, before) = time::timeout(Duration::from_secs(30), async {
        let start = session.client.thread_start(serde_json::json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
        let thread = start["thread"]["id"].as_str().unwrap().to_owned();
        let mut first_turn = String::new();
        let mut item_ids = Vec::new();

        for phase in 0..2 {
            let response = session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":if phase==0 {"Start delayed MCP"} else {"Wait for prior MCP"}}]})).await.unwrap();
            let turn = response["turn"]["id"].as_str().unwrap();

            if phase==0 { first_turn = turn.into(); }

            else {
                assert_ne!(turn,first_turn);

                fs::write(&gate,b"release").unwrap();
            }

            loop {
                let event = session.events.recv().await.unwrap();

                if let ServerEvent::Notification { method, params } = event {
                    if method=="item/completed" && params["item"]["type"]=="mcpToolCall" {
                        assert_eq!(phase,1);
                        assert_eq!(params["turnId"],first_turn);
                        assert_eq!(params["item"]["status"],"completed");

                        item_ids.push(params["item"]["id"].as_str().unwrap().to_owned());
                    }
                    if method=="turn/completed" {
                        assert_eq!(params["turn"]["id"],turn);
                        assert_eq!(params["turn"]["status"],"completed");

                        break;
                    }
                }
            }

            if phase==0 {
                while !gate.with_extension("started").exists() {
                    time::sleep(Duration::from_millis(10)).await;
                }
            }
        }

        assert_eq!(item_ids.len(), 2);
        assert_ne!(item_ids[0], item_ids[1]);

        let history=session.client.thread_history_page(&thread,None,5).await.unwrap();

        (thread,first_turn,item_ids,history)
    }).await.unwrap();

	assert_eq!(calls.load(Ordering::Acquire), 4);

	let metadata = fs::read_to_string(gate.with_extension("metadata")).unwrap();
	let metadata: Vec<Value> =
		metadata.lines().map(|line| serde_json::from_str(line).unwrap()).collect();

	assert_eq!(metadata.len(), 2, "both nested calls reached the MCP server");

	let origin = bodies.lock().unwrap()[0]["client_metadata"].clone();

	assert!(origin["session_id"].as_str().is_some_and(|id| !id.is_empty()));
	assert!(origin["x-codex-window-id"].as_str().is_some_and(|id| !id.is_empty()));

	for meta in metadata {
		assert_eq!(meta["threadId"], thread);
		assert_eq!(meta["sessionId"], origin["session_id"]);
		assert_eq!(meta["windowId"], origin["x-codex-window-id"]);
		assert_eq!(meta["itemId"], "originating-code-cell");
	}

	drop(session);

	let cold = NativeSession::start(&binary, home.path());
	let after = cold.client.thread_history_page(&thread, None, 5).await.unwrap();

	assert_delayed_call_history(&before, &after, &item_ids, &first_turn);

	assert_eq!(calls.load(Ordering::Acquire), 4);

	backend.abort();
}
