//! Isolated native process fixture shared with tool visibility qualification.
use super::*;

pub(crate) async fn native(home: &Path) -> (AppServerClient, tokio::process::Child) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit installed binary");
	assert!(Path::new(&binary).is_absolute());
	let mut child = tokio::process::Command::new(binary)
		.arg("app-server")
		.env_clear()
		.env("HOME", home)
		.env("CODEX_HOME", home)
		.env("PATH", "/usr/bin:/bin")
		.current_dir(home)
		.stdin(std::process::Stdio::piped())
		.stdout(std::process::Stdio::piped())
		.stderr(std::process::Stdio::null())
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
		.initialize(json!({"clientInfo":{"name":"decodex_link_settings_test","version":"0.1"},
			"capabilities":{"experimentalApi":true}}))
		.await
		.unwrap();
	(client, child)
}
