//! Real config writes must control hook execution, preserve hash trust and survive restart.
use std::{env, fs, net::SocketAddr, path::Path, sync::atomic::AtomicUsize};

use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::{reviewer, *};
use decodex_codex::app_server_client::{HookSettingsChange, HookSettingsWrite};

fn setup(root: &Path, address: SocketAddr) {
	let plugin = root.join("plugins/cache/test/sample/local");

	fs::create_dir_all(plugin.join(".codex-plugin")).expect("plugin");
	fs::create_dir_all(plugin.join("hooks")).expect("hooks");
	fs::create_dir(root.join(".git")).expect("repository");
	fs::create_dir_all(root.join(".agents/plugins")).expect("marketplace");
	fs::write(plugin.join(".codex-plugin/plugin.json"), r#"{"name":"sample"}"#).expect("manifest");
	fs::write(plugin.join("hooks/hooks.json"),serde_json::json!({"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"echo isolated-plugin-hook"}]}]}}).to_string()).expect("hook");
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

	let backend = tokio::spawn(serve(listener, calls.clone()));
	let mut session = NativeSession::start(&binary, &root);
	let started = session
		.client
		.thread_start(
			serde_json::json!({"cwd":root,"approvalPolicy":"never","sandbox":"read-only"}),
		)
		.await
		.expect("thread");
	let thread = started["thread"]["id"].as_str().expect("thread id").to_owned();

	assert_eq!(turn(&mut session, &thread).await, 0, "untrusted hook must not execute");

	reviewer::trust_hook(&session.client, &root, &thread).await;

	assert_eq!(turn(&mut session, &thread).await, 1, "native reload activates reviewed hook");

	change(&session, &root, &thread, HookSettingsChange::Enabled(false)).await;

	assert_eq!(turn(&mut session, &thread).await, 0, "disabled trusted hook must not execute");

	drop(session);

	let mut session = NativeSession::start(&binary, &root);

	session.client.thread_resume(serde_json::json!({"threadId":thread})).await.expect("resume");

	assert_eq!(turn(&mut session, &thread).await, 0, "disabled setting survives restart");

	change(&session, &root, &thread, HookSettingsChange::Enabled(true)).await;

	assert_eq!(
		turn(&mut session, &thread).await,
		1,
		"enable does not require new trust for unchanged content"
	);

	fs::write(root.join("plugins/cache/test/sample/local/hooks/hooks.json"),serde_json::json!({"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"echo modified-isolated-plugin-hook"}]}]}}).to_string()).expect("modify fixture hook");

	drop(session);

	let mut session = NativeSession::start(&binary, &root);

	session
		.client
		.thread_resume(serde_json::json!({"threadId":thread}))
		.await
		.expect("resume modified");

	let review =
		session.client.hook_settings(root.to_str().expect("cwd")).await.expect("modified review");

	assert_eq!(review.inventory["hooks"][0]["trustStatus"], "modified");
	assert_eq!(
		turn(&mut session, &thread).await,
		0,
		"saved trust does not approve changed content"
	);

	change(&session, &root, &thread, HookSettingsChange::Trust).await;

	assert_eq!(
		turn(&mut session, &thread).await,
		1,
		"explicit new hash trust activates modified hook"
	);
	assert_eq!(calls.load(Ordering::Acquire), 7);

	drop(session);

	backend.abort();
}

async fn change(session: &NativeSession, root: &Path, thread: &str, change: HookSettingsChange) {
	let review = session.client.hook_settings(root.to_str().expect("cwd")).await.expect("review");
	let hook = &review.inventory["hooks"][0];

	assert!(matches!(
		hook["command"].as_str(),
		Some("echo isolated-plugin-hook" | "echo modified-isolated-plugin-hook")
	));

	let params = review.change(hook["key"].as_str().expect("key"), change).expect("selection");
	let guard = session.client.thread_settings_guard(thread).expect("guard");

	assert_eq!(
		session.client.write_hook_settings(params, guard).await.expect("write"),
		HookSettingsWrite::Saved
	);
}

async fn turn(session: &mut NativeSession, thread: &str) -> usize {
	let started = session
		.client
		.turn_start(
			serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Finish the isolated fixture turn"}]}),
		)
		.await
		.expect("turn");
	let turn = started["turn"]["id"].as_str().expect("turn id");
	let mut hooks = 0;

	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("turn event")
		{
			if method == "hook/started" {
				hooks += 1;
			}
			if method == "turn/completed"
				&& params["threadId"] == thread
				&& params["turn"]["id"] == turn
			{
				assert_eq!(params["turn"]["status"], "completed");

				return hooks;
			}
		}
	}
}
