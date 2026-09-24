//! Installed task plugin exclusions must affect hooks on subsequent turns and survive restart.
use super::*;
use decodex_codex::app_server_client::{NativeTaskPlugins, ThreadPluginSelection};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated plugin activation qualification"]
async fn installed_plugin_exclusions_filter_hooks_and_survive_restart() {
	tokio::time::timeout(Duration::from_secs(60), qualify(false))
		.await
		.expect("bounded plugin fixture");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated active plugin selection"]
async fn installed_active_plugin_selection_applies_to_later_turns() {
	tokio::time::timeout(Duration::from_secs(60), qualify(true))
		.await
		.expect("bounded active plugin fixture");
}

async fn qualify(running: bool) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
	let home = tempfile::tempdir().expect("home");
	let root = home.path().canonicalize().expect("canonical home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
	let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	setup(&root, listener.local_addr().expect("address"));
	let mut listener = Some(listener);
	let mut backend = None;
	if !running {
		backend = Some(tokio::spawn(serve(listener.take().expect("listener"), calls.clone())));
	}
	let session = NativeSession::start(&binary, &root);
	let catalog = session
		.client
		.installed_plugins_for_directory(root.to_str().expect("cwd"))
		.await
		.expect("catalog");
	assert!(catalog["marketplaces"][0]["plugins"][0]["installed"].as_bool().expect("installed"));
	trust_fixture_hook(&session, &root).await;
	drop(session);
	let mut session = NativeSession::start(&binary, &root);
	let start = session
		.client
		.thread_start(json!({"cwd":root,"approvalPolicy":"never","sandbox":"read-only"}))
		.await
		.expect("thread");
	let thread = start["thread"]["id"].as_str().expect("thread id").to_owned();
	let initial = session.client.configured_task_plugins(&thread).expect("initial selection").0;
	assert!(initial.disabled_plugin_ids.is_empty());
	if running {
		start_turn(&mut session, &thread).await;
		loop {
			if let ServerEvent::Notification { method, .. } =
				session.events.recv().await.expect("enabled hook")
				&& method == "hook/started"
			{
				break;
			}
		}
		assert!(session.client.observed_task_plugins(&thread).is_none());
	} else {
		assert_eq!(turn(&mut session, &thread).await, 1, "enabled hook");
	}
	select(&mut session, &thread, vec!["sample@test".into()]).await;
	if running {
		assert!(session.client.observed_task_plugins(&thread).is_none());
		backend = Some(tokio::spawn(serve(listener.take().expect("held listener"), calls.clone())));
		assert_eq!(
			finish_turn(&mut session, &thread).await,
			0,
			"first turn hook already ran before selection"
		);
	}
	assert_eq!(calls.load(Ordering::Acquire), 1, "setting does not start inference");
	assert_eq!(turn(&mut session, &thread).await, 0, "disabled hook");
	let other = session
		.client
		.thread_start(json!({"cwd":root,"approvalPolicy":"never","sandbox":"read-only"}))
		.await
		.expect("second thread");
	let other = other["thread"]["id"].as_str().expect("second id");
	assert_eq!(turn(&mut session, other).await, 1, "other thread keeps global plugin default");
	drop(session);
	let mut session = NativeSession::start(&binary, &root);
	let resumed = session
		.client
		.thread_resume(json!({"threadId":thread,"excludeTurns":true}))
		.await
		.expect("resume");
	assert_eq!(
		NativeTaskPlugins::from_settings(&resumed).expect("saved selection").disabled_plugin_ids,
		["sample@test"]
	);
	assert_eq!(
		session.client.observed_task_plugins(&thread).expect("hydration").0.disabled_plugin_ids,
		["sample@test"]
	);
	assert_eq!(turn(&mut session, &thread).await, 0, "cold disabled hook");
	select(&mut session, &thread, vec![]).await;
	assert_eq!(turn(&mut session, &thread).await, 1, "re-enabled hook");
	assert_eq!(calls.load(Ordering::Acquire), 5);
	drop(session);
	backend.expect("started backend").abort();
}

fn setup(root: &std::path::Path, address: std::net::SocketAddr) {
	let plugin = root.join("plugins/cache/test/sample/local");
	std::fs::create_dir_all(plugin.join(".codex-plugin")).expect("plugin");
	std::fs::create_dir_all(plugin.join("hooks")).expect("hooks");
	std::fs::create_dir(root.join(".git")).expect("repository");
	std::fs::create_dir_all(root.join(".agents/plugins")).expect("marketplace");
	std::fs::write(plugin.join(".codex-plugin/plugin.json"), r#"{"name":"sample"}"#)
		.expect("manifest");
	std::fs::write(plugin.join("hooks/hooks.json"),json!({"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"echo isolated-plugin-hook"}]}]}}).to_string()).expect("hook");
	std::fs::write(root.join(".agents/plugins/marketplace.json"),json!({"name":"test","plugins":[{"name":"sample","source":{"source":"local","path":"./plugins/cache/test/sample/local"}}]}).to_string()).expect("marketplace");
	std::fs::write(root.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nplugins=true\nhooks=true\n[plugins.\"sample@test\"]\nenabled=true\n[projects.{}]\ntrust_level=\"trusted\"\n[model_providers.fixture]\nname=\"Isolated plugin fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n",json!(root))).expect("config");
}

async fn select(session: &mut NativeSession, thread: &str, excluded: Vec<String>) {
	let (_, guard) = session.client.configured_task_plugins(thread).expect("current selection");
	session
		.client
		.queue_thread_plugin_selection(
			&ThreadPluginSelection::new(thread, excluded.clone()).expect("selection"),
			guard.clone(),
		)
		.await
		.expect("queue");
	loop {
		let event = session.events.recv().await.expect("settings event");
		if let ServerEvent::Notification { method, params } = event
			&& method == "thread/settings/updated"
			&& params["threadId"] == thread
		{
			assert_eq!(
				NativeTaskPlugins::from_settings(&params["threadSettings"])
					.expect("published selection")
					.disabled_plugin_ids,
				excluded
			);
			assert!(!guard.is_live());
			assert_eq!(
				session
					.client
					.configured_task_plugins(thread)
					.expect("wire facts")
					.0
					.disabled_plugin_ids,
				excluded
			);
			break;
		}
	}
}

async fn start_turn(session: &mut NativeSession, thread: &str) {
	session
		.client
		.turn_start(
			json!({"threadId":thread,"input":[{"type":"text","text":"Finish the isolated fixture turn"}]}),
		)
		.await
		.expect("turn");
}

async fn turn(session: &mut NativeSession, thread: &str) -> usize {
	start_turn(session, thread).await;
	finish_turn(session, thread).await
}

async fn finish_turn(session: &mut NativeSession, thread: &str) -> usize {
	let mut hooks = 0;
	loop {
		let event = session.events.recv().await.expect("turn event");
		if let ServerEvent::Notification { method, params } = event {
			if method == "hook/started" {
				hooks += 1;
			}
			if method == "turn/completed" && params["threadId"] == thread {
				assert_eq!(params["turn"]["status"], "completed");
				return hooks;
			}
		}
	}
}

async fn trust_fixture_hook(session: &NativeSession, root: &std::path::Path) {
	use std::io::Write as _;
	let listed =
		session.client.request("hooks/list", json!({"cwds":[root]})).await.expect("hook inventory");
	let hooks = listed["data"][0]["hooks"].as_array().expect("hooks");
	let hook = hooks.iter().find(|h| h["pluginId"] == "sample@test").expect("fixture hook");
	assert_eq!(hook["command"], "echo isolated-plugin-hook");
	let key = hook["key"].as_str().expect("hook key");
	let hash = hook["currentHash"].as_str().expect("hook hash");
	// Approve only this known harmless fixture command in the disposable home. Native hook trust
	// remains enabled; later plugin exclusion must still prevent the trusted hook from running.
	let mut config = std::fs::OpenOptions::new()
		.append(true)
		.open(root.join("config.toml"))
		.expect("fixture config");
	writeln!(config, "\n[hooks.state.{}]\ntrusted_hash={}\nenabled=true", json!(key), json!(hash))
		.expect("fixture hook trust");
}
