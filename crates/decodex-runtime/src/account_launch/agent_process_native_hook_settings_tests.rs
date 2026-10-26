//! Native hook trust must control script effects, independently of enablement and previews.
use std::{env, fs, net::SocketAddr, path::Path, sync::atomic::AtomicUsize};

use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::{
	self, Arc, Duration, NativeSession, Ordering, ServerEvent,
};
use decodex_codex::app_server_client::{HookSettingsChange, HookSettingsWrite};

const COMMAND: &str = "python3 -I -S ${PLUGIN_ROOT}/hooks/effect.py";

fn hook_config(modified: bool) -> String {
	let mut hook = serde_json::json!({"type":"command","command":COMMAND});

	if modified {
		hook["statusMessage"] = serde_json::json!("Modified isolated hook");
	}

	serde_json::json!({"hooks":{"UserPromptSubmit":[{"hooks":[hook]}]}}).to_string()
}

fn write_script(root: &Path, effect: &str) {
	fs::write(
		root.join("plugins/cache/test/sample/local/hooks/effect.py"),
		format!(
			"import json, sys\nfrom pathlib import Path\njson.load(sys.stdin)\nwith Path({}).open('a') as output:\n    output.write({} + '\\n')\n",
			serde_json::json!(root.join("hook-effects.log")),
			serde_json::json!(effect),
		),
	)
	.expect("hook script");
}

fn effects(root: &Path) -> Vec<String> {
	fs::read_to_string(root.join("hook-effects.log"))
		.unwrap_or_default()
		.lines()
		.map(str::to_owned)
		.collect()
}

fn setup(root: &Path, address: SocketAddr) {
	let plugin = root.join("plugins/cache/test/sample/local");

	fs::create_dir_all(plugin.join(".codex-plugin")).expect("plugin");
	fs::create_dir_all(plugin.join("hooks")).expect("hooks");
	fs::create_dir(root.join(".git")).expect("repository");
	fs::create_dir_all(root.join(".agents/plugins")).expect("marketplace");
	fs::write(plugin.join(".codex-plugin/plugin.json"), r#"{"name":"sample"}"#).expect("manifest");
	fs::write(plugin.join("hooks/hooks.json"), hook_config(false)).expect("hook");

	write_script(root, "original");

	fs::write(root.join(".agents/plugins/marketplace.json"),serde_json::json!({"name":"test","plugins":[{"name":"sample","source":{"source":"local","path":"./plugins/cache/test/sample/local"}}]}).to_string()).expect("marketplace");
	fs::write(root.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nplugins=true\nhooks=true\n[plugins.\"sample@test\"]\nenabled=true\n[projects.{}]\ntrust_level=\"trusted\"\n[model_providers.fixture]\nname=\"Isolated plugin fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n",serde_json::json!(root))).expect("config");
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated hook configuration"]
async fn installed_hook_trust_enablement_and_modified_content_are_independent() {
	time::timeout(Duration::from_secs(60), qualify_hooks()).await.expect("bounded hook fixture");
}

async fn qualify_hooks() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("binary");
	let home = tempfile::tempdir().expect("home");
	let root = home.path().canonicalize().expect("canonical home");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
	let calls = Arc::new(AtomicUsize::new(0));

	setup(&root, listener.local_addr().expect("address"));

	let backend = tokio::spawn(native_tests::serve(listener, calls.clone()));
	let mut session = NativeSession::start(&binary, &root);
	let started = session
		.client
		.thread_start(
			serde_json::json!({"cwd":root,"approvalPolicy":"never","sandbox":"read-only"}),
		)
		.await
		.expect("thread");
	let thread = started["thread"]["id"].as_str().expect("thread id").to_owned();

	assert_eq!(turn(&mut session, &root, &thread).await, 0, "untrusted script must not run");

	change(&session, &root, &thread, HookSettingsChange::Trust).await;

	assert_eq!(turn(&mut session, &root, &thread).await, 1, "review activates the script");

	change(&session, &root, &thread, HookSettingsChange::Enabled(false)).await;

	assert_eq!(turn(&mut session, &root, &thread).await, 0, "disabled script must not run");

	drop(session);

	let mut session = NativeSession::start(&binary, &root);

	session.client.thread_resume(serde_json::json!({"threadId":thread})).await.expect("resume");

	assert_eq!(turn(&mut session, &root, &thread).await, 0, "disable survives restart");

	change(&session, &root, &thread, HookSettingsChange::Enabled(true)).await;

	assert_eq!(turn(&mut session, &root, &thread).await, 1, "unchanged trust survives enablement");

	fs::write(root.join("plugins/cache/test/sample/local/hooks/hooks.json"), hook_config(true))
		.expect("change declaration");

	drop(session);

	let mut session = NativeSession::start(&binary, &root);

	session
		.client
		.thread_resume(serde_json::json!({"threadId":thread}))
		.await
		.expect("resume changed declaration");

	let review = session
		.client
		.hook_settings(root.to_str().expect("UTF-8 fixture cwd"))
		.await
		.expect("review");

	assert_eq!(review.inventory["hooks"][0]["trustStatus"], "modified");
	assert_eq!(
		turn(&mut session, &root, &thread).await,
		0,
		"old trust must not approve a changed declaration"
	);

	change(&session, &root, &thread, HookSettingsChange::Trust).await;

	assert_eq!(
		turn(&mut session, &root, &thread).await,
		1,
		"review activates the changed declaration"
	);
	assert_eq!(effects(&root), ["original", "original", "original"]);

	check_modified_script(&mut session, &root, &thread).await;
	drop(session);

	let mut session = NativeSession::start(&binary, &root);

	session
		.client
		.thread_resume(serde_json::json!({"threadId":thread}))
		.await
		.expect("resume approved script");

	assert_eq!(turn(&mut session, &root, &thread).await, 1, "content approval survives restart");
	assert_eq!(effects(&root), ["original", "original", "original", "modified", "modified"]);
	assert_eq!(calls.load(Ordering::Acquire), 10);

	drop(session);

	backend.abort();
}

async fn change(session: &NativeSession, root: &Path, thread: &str, change: HookSettingsChange) {
	let review = session.client.hook_settings(root.to_str().expect("cwd")).await.expect("review");
	let hook = &review.inventory["hooks"][0];

	assert_eq!(
		hook["command"],
		COMMAND.replace(
			"${PLUGIN_ROOT}",
			root.join("plugins/cache/test/sample/local").to_str().expect("UTF-8 plugin path")
		)
	);

	let params = review.change(hook["key"].as_str().expect("key"), change).expect("selection");
	let guard = session.client.thread_settings_guard(thread).expect("guard");

	assert_eq!(
		session.client.write_hook_settings(params, guard).await.expect("write"),
		HookSettingsWrite::Saved
	);
}

async fn turn(session: &mut NativeSession, root: &Path, thread: &str) -> usize {
	let before = effects(root).len();
	let started = session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Finish the isolated fixture turn"}]})).await.expect("turn");
	let turn = started["turn"]["id"].as_str().expect("turn id");
	let mut previews = 0;
	let mut trust_error = false;

	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("turn event")
		{
			if method == "hook/started" {
				previews += 1;
			}
			if method == "hook/completed" {
				trust_error |= params.to_string().contains("hook approval required");
			}
			if method == "turn/completed"
				&& params["threadId"] == thread
				&& params["turn"]["id"] == turn
			{
				assert_eq!(params["turn"]["status"], "completed");

				let executed = effects(root).len() - before;

				// A started notification previews verification, not script execution.
				if previews > 0 && executed == 0 {
					assert!(trust_error, "a skipped preview must report the trust failure");
				}

				return executed;
			}
		}
	}
}

async fn check_modified_script(session: &mut NativeSession, root: &Path, thread: &str) {
	// Keep the declaration and live session unchanged. The next execution must hash fresh bytes.
	let declaration = fs::read(root.join("plugins/cache/test/sample/local/hooks/hooks.json"))
		.expect("hook declaration");

	write_script(root, "modified");

	assert_eq!(
		turn(session, root, thread).await,
		0,
		"old trust must not execute changed script bytes"
	);
	assert_eq!(
		fs::read(root.join("plugins/cache/test/sample/local/hooks/hooks.json"))
			.expect("hook declaration"),
		declaration
	);

	let review = session
		.client
		.hook_settings(root.to_str().expect("UTF-8 fixture cwd"))
		.await
		.expect("script review");

	assert_eq!(review.inventory["hooks"][0]["trustStatus"], "modified");

	change(session, root, thread, HookSettingsChange::Trust).await;

	assert_eq!(turn(session, root, thread).await, 1, "new content requires a new review");
}
