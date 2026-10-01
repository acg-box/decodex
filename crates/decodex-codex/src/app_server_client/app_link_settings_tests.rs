//! Isolated native process fixture shared with tool visibility qualification.
use std::{env, process::Stdio};

use tokio::process::{Child, Command};

use crate::app_server_client::app_link_settings::*;

pub(crate) async fn native(home: &Path) -> (AppServerClient, Child) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit installed binary");

	assert!(Path::new(&binary).is_absolute());

	let mut child = Command::new(binary)
		.arg("app-server")
		.env_clear()
		.env("HOME", home)
		.env("CODEX_HOME", home)
		.env("PATH", "/usr/bin:/bin")
		.current_dir(home)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::null())
		.kill_on_drop(true)
		.spawn()
		.unwrap();
	let (client, events) =
		AppServerClient::from_io(child.stdout.take().unwrap(), child.stdin.take().unwrap());

	tokio::spawn(async move {
		let mut events = events;

		while events.recv().await.is_some() {}
	});

	client
		.initialize(
			serde_json::json!({"clientInfo":{"name":"decodex_link_settings_test","version":"0.1"},
			"capabilities":{"experimentalApi":true}}),
		)
		.await
		.unwrap();

	(client, child)
}
