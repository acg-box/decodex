//! Native configuration roundtrip on an isolated home and production launch arguments.
use std::{env, fs, process::Stdio};

use tokio::process::Command;

use crate::account_launch::process::native_control_tests;
use decodex_codex::app_server_client::AppServerClient;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native configuration roundtrip"]
async fn installed_native_context_preferences_share_config_and_preserve_thresholds() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let home = tempfile::tempdir().unwrap();
	let cwd = home.path().to_str().unwrap();
	let file = home.path().join("config.toml");

	for async_value in ["false", "{enabled=false,model_async_compact_threshold_percent=70}"] {
		fs::write(
			&file,
			format!(
				"[features]\nasync_compaction={async_value}\nretain_client_tool_outputs=false\n[mcp_servers.\"docs.v2\"]\nurl=\"http://127.0.0.1:1/mcp\"\nenabled=false\n"
			),
		)
		.unwrap();

		let profile = native_control_tests::attested_profile(&binary, home.path());
		let mut child = Command::new(&profile.command.program)
			.args(&profile.command.app_server_args)
			.env_clear()
			.env("HOME", home.path())
			.env("CODEX_HOME", home.path())
			.env("PATH", "/usr/bin:/bin")
			.current_dir(home.path())
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.kill_on_drop(true)
			.spawn()
			.unwrap();
		let (client, mut events) =
			AppServerClient::from_io(child.stdout.take().unwrap(), child.stdin.take().unwrap());
		let drain = tokio::spawn(async move { while events.recv().await.is_some() {} });

		client.initialize(serde_json::json!({"clientInfo":{"name":"decodex_context_fixture","version":"0.1"},"capabilities":{"experimentalApi":true}})).await.unwrap();

		let observed = client.context_settings(cwd).await.unwrap();

		assert_eq!(
			observed.effective,
			[false, false],
			"production arguments must respect explicit disable"
		);
		assert_eq!(observed.preference, [Some(false), Some(false)]);
		assert_eq!(observed.mcp_startup, vec![("docs.v2".into(), false, None)]);

		let saved =
			client.write_context_preference(&observed, "async_compaction", true).await.unwrap();

		assert_eq!(saved.preference, [Some(true), Some(false)]);
		assert_eq!(saved.effective, [true, false]);

		let saved = client
			.write_context_preference(&saved, "retain_client_tool_outputs", true)
			.await
			.unwrap();

		assert_eq!(saved.preference, [Some(true), Some(true)]);
		assert_eq!(saved.effective, [true, true]);
		assert!(
			client.write_context_preference(&observed, "async_compaction", false).await.is_err(),
			"a stale observation cannot overwrite the latest native file"
		);

		let saved =
			client.write_context_preference(&saved, "async_compaction", false).await.unwrap();

		assert_eq!(saved.effective, [false, true]);

		if async_value.contains("70") {
			let config =
				client.request("config/read", serde_json::json!({"cwd":cwd})).await.unwrap();

			assert_eq!(
				config["config"]["features"]["async_compaction"]["model_async_compact_threshold_percent"],
				70
			);
		}

		let saved = client.write_mcp_startup_preference(&saved, "docs.v2", true).await.unwrap();
		assert_eq!(saved.mcp_startup, vec![("docs.v2".into(), true, Some(true))]);
		assert!(client.write_mcp_startup_preference(&saved, "not-configured", true).await.is_err());
		let saved = client.write_mcp_startup_preference(&saved, "docs.v2", false).await.unwrap();
		assert_eq!(saved.mcp_startup, vec![("docs.v2".into(), false, Some(false))]);
		let content = fs::read_to_string(&file).unwrap();
		assert!(content.contains("enabled=false"));
		assert!(content.contains("http://127.0.0.1:1/mcp"));

		// A manual config edit is the same owner as a UI write and invalidates its version.
		fs::write(&file, "[features]\nasync_compaction=true\nretain_client_tool_outputs=false\n")
			.unwrap();

		assert!(
			client
				.write_context_preference(&saved, "retain_client_tool_outputs", true)
				.await
				.is_err()
		);
		assert_eq!(client.context_settings(cwd).await.unwrap().effective, [true, false]);

		child.kill().await.unwrap();
		child.wait().await.unwrap();
		drain.abort();
	}
}
