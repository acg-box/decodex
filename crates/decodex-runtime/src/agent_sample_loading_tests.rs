//! Cold suggestion preparation must not send a turn or resume an active goal.
use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

use decodex_codex::app_server_client::AppServerClient;

#[tokio::test]
async fn cold_suggestion_loads_only_a_dormant_source_without_submitting_input() {
	for case in ["loaded", "cold", "active_goal", "wrong_source"] {
		let (local, remote) = io::duplex(16_384);
		let (read, write) = io::split(local);
		let (client, _events) = AppServerClient::from_io(read, write);
		let server = tokio::spawn(async move {
			let (read, mut write) = io::split(remote);
			let mut lines = BufReader::new(read).lines();
			let mut methods = Vec::new();
			while let Some(line) = lines.next_line().await.unwrap() {
				let request: serde_json::Value = serde_json::from_str(&line).unwrap();
				let method = request["method"].as_str().unwrap();
				methods.push(method.to_owned());
				let result = match method {
					"thread/read" => serde_json::json!({"thread":{
						"id":if case == "wrong_source" {"other"} else {"source"},
						"status":{"type":if case == "loaded" {"idle"} else {"notLoaded"}}
					}}),
					"thread/goal/get" if case == "active_goal" => serde_json::json!({"goal":{
						"threadId":"source", "objective":"Keep working", "status":"active",
						"tokenBudget":null, "tokensUsed":0, "timeUsedSeconds":0,
						"createdAt":1, "updatedAt":1
					}}),
					"thread/goal/get" => serde_json::json!({"goal":null}),
					_ => panic!("unexpected operation: {method}"),
				};
				write
					.write_all(
						format!("{}\n", serde_json::json!({"id":request["id"],"result":result}))
							.as_bytes(),
					)
					.await
					.unwrap();
				if method == "thread/goal/get" || matches!(case, "wrong_source" | "loaded") {
					break;
				}
			}
			methods
		});
		let result = super::sample_needs_resume(&client, "source").await;
		assert_eq!(result.is_ok(), matches!(case, "loaded" | "cold"), "{case}");
		let methods = server.await.unwrap();
		assert_eq!(result.unwrap_or(false), case == "cold");
		assert!(!methods.contains(&"thread/resume".to_owned()));
	}
}
