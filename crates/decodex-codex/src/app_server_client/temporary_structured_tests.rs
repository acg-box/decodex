use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

use crate::app_server_client::temporary_structured::*;

fn options() -> TemporaryStructuredOptions {
	TemporaryStructuredOptions {
		model: "model".into(),
		model_provider: "provider".into(),
		cwd: "/tmp".into(),
		active_permission_profile: None,
		mcp_server_names: vec!["known".into()],
	}
}

#[test]
fn isolation_disables_effective_and_observed_mcp_without_mutating_configuration() {
	let effective =
		serde_json::json!({"mcp_servers":{"native":{"required":true,"command":"must-not-run"}}});
	let config = isolation_config(&effective, &["observed".into()]).unwrap();

	assert_eq!(
		config["mcp_servers"],
		serde_json::json!({"native":{"enabled":false},"observed":{"enabled":false}})
	);
	assert_eq!(config["features.shell_tool"], false);
	assert_eq!(config["features.plugins"], false);
	assert_eq!(config["features.hooks"], false);
	assert_eq!(config["skills.include_instructions"], false);
	assert_eq!(config["web_search"], "disabled");
	assert_eq!(effective["mcp_servers"]["native"]["required"], true);
	assert!(isolation_config(&serde_json::json!({"mcp_servers":[]}), &[]).is_err());
}

fn message(thread: &str, turn: &str, text: &str) -> ServerEvent {
	ServerEvent::Notification {
		method: "item/completed".into(),
		params: serde_json::json!({"threadId":thread,"turnId":turn,"item":{"type":"agentMessage","text":text}}),
	}
}

fn completed(thread: &str, turn: &str, status: &str) -> ServerEvent {
	ServerEvent::Notification {
		method: "turn/completed".into(),
		params: serde_json::json!({"threadId":thread,"turn":{"id":turn,"status":status}}),
	}
}

#[tokio::test]
async fn temporary_permissions_override_builtin_defaults_but_preserve_custom_profiles() {
	for profile in [None, Some(":workspace"), Some(":read-only"), Some("restricted")] {
		let (local, remote) = io::duplex(16_384);
		let (read, write) = io::split(local);
		let (client, _events) = AppServerClient::from_io(read, write);
		let server = tokio::spawn(async move {
			let (read, mut write) = io::split(remote);
			let mut lines = BufReader::new(read).lines();

			for method in ["config/read", "thread/start", "thread/unsubscribe"] {
				let req: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(req["method"], method);

				let result = match method {
					"config/read" =>
						serde_json::json!({"config":{"default_permissions":":workspace"}}),
					"thread/start" => {
						let params = &req["params"];

						if profile == Some("restricted") {
							assert_eq!(params["permissions"], "restricted");
							assert!(params.get("sandbox").is_none());
							assert!(params["config"].get("default_permissions").is_none());
						} else {
							assert_eq!(params["sandbox"], "read-only");
							assert!(params.get("permissions").is_none());
							assert_eq!(params["config"]["default_permissions"], ":read-only");
						}

						serde_json::json!({"thread":{"id":"temporary","ephemeral":true},
							"sandbox":{"type":"readOnly"},"activePermissionProfile":{"id":profile}})
					},
					_ => serde_json::json!({"status":"unsubscribed"}),
				};

				write
					.write_all(
						format!("{}\n", serde_json::json!({"id":req["id"],"result":result}))
							.as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		let mut options = options();

		options.active_permission_profile = profile.map(str::to_owned);

		client.start_temporary_structured(options).await.unwrap().cancel().await.unwrap();
		server.await.unwrap();
	}
}

#[tokio::test]
async fn collector_keeps_latest_exact_turn_and_rejects_incomplete_or_oversized_results() {
	let (tx, mut rx) = mpsc::channel(8);

	for event in [
		message("other", "turn", "wrong thread"),
		message("thread", "other", "wrong turn"),
		message("thread", "turn", "first"),
		message("thread", "turn", "latest"),
		completed("other", "turn", "completed"),
		completed("thread", "turn", "completed"),
	] {
		tx.send(event).await.unwrap();
	}

	assert_eq!(collect(&mut rx, "thread", "turn").await.unwrap(), "latest");

	for event in [
		message("thread", "turn", &"x".repeat(MAX_RESPONSE + 1)),
		completed("thread", "turn", "failed"),
		completed("thread", "turn", "completed"),
	] {
		let (tx, mut rx) = mpsc::channel(1);

		tx.send(event).await.unwrap();

		drop(tx);

		assert!(collect(&mut rx, "thread", "turn").await.is_err());
	}
}

#[tokio::test]
async fn cancellation_waits_for_turn_identity_then_interrupts_and_detaches() {
	let (local, remote) = io::duplex(16_384);
	let (read, write) = io::split(local);
	let (client, events) = AppServerClient::from_io(read, write);
	let (cancel, watch) = watch::channel(false);
	let server = tokio::spawn(async move {
		let (read, mut write) = io::split(remote);
		let mut lines = BufReader::new(read).lines();

		for method in
			["config/read", "thread/start", "turn/start", "turn/interrupt", "thread/unsubscribe"]
		{
			let req: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

			assert_eq!(req["method"], method);

			let result = match method {
				"config/read" => serde_json::json!({"config":{"mcp_servers":{}}}),
				"thread/start" => {
					assert_eq!(req["params"]["ephemeral"], true);
					assert_eq!(req["params"]["dynamicTools"], serde_json::json!([]));

					serde_json::json!({"thread":{"id":"temporary","ephemeral":true},"sandbox":{"type":"readOnly"}})
				},
				"turn/start" => {
					cancel.send(true).unwrap();

					serde_json::json!({"turn":{"id":"exact-turn"}})
				},
				"turn/interrupt" => {
					assert_eq!(
						req["params"],
						serde_json::json!({"threadId":"temporary","turnId":"exact-turn"})
					);

					serde_json::json!({})
				},
				_ => {
					assert_eq!(req["params"]["threadId"], "temporary");

					serde_json::json!({"status":"unsubscribed"})
				},
			};

			write
				.write_all(
					format!("{}\n", serde_json::json!({"id":req["id"],"result":result})).as_bytes(),
				)
				.await
				.unwrap();
		}
	});
	let thread = client.start_temporary_structured(options()).await.unwrap();

	assert!(
		thread
			.run("summary".into(), serde_json::json!({"type":"object"}), None, events, watch)
			.await
			.is_err()
	);

	server.await.unwrap();
}

#[tokio::test]
async fn rejected_permissions_and_pre_cancelled_requests_detach_without_inference() {
	for invalid in [false, true] {
		let (local, remote) = io::duplex(16_384);
		let (read, write) = io::split(local);
		let (client, events) = AppServerClient::from_io(read, write);
		let server = tokio::spawn(async move {
			let (read, mut write) = io::split(remote);
			let mut lines = BufReader::new(read).lines();

			for method in ["config/read", "thread/start", "thread/unsubscribe"] {
				let req: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(req["method"], method);

				let result = match method {
					"config/read" => serde_json::json!({"config":{}}),
					"thread/start" =>
						serde_json::json!({"thread":{"id":"temporary","ephemeral":true},"sandbox":{"type":if invalid {"dangerFullAccess"} else {"readOnly"}}}),
					_ => serde_json::json!({"status":"unsubscribed"}),
				};

				write
					.write_all(
						format!("{}\n", serde_json::json!({"id":req["id"],"result":result}))
							.as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		let thread = client.start_temporary_structured(options()).await;

		if invalid {
			assert!(thread.is_err());
		} else {
			let (_cancel, watch) = watch::channel(true);

			assert!(
				thread
					.unwrap()
					.run("summary".into(), serde_json::json!({}), None, events, watch)
					.await
					.is_err()
			);
		}

		server.await.unwrap();
	}
}
