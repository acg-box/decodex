//! Real config writes must control hook execution, preserve hash trust and survive restart.
use super::*;
use decodex_codex::app_server_client::{HookSettingsChange, HookSettingsWrite};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated hook configuration"]
async fn installed_hook_trust_enablement_and_modified_content_are_independent() {
	tokio::time::timeout(Duration::from_secs(60), qualify_hooks())
		.await
		.expect("bounded hook fixture");
}
async fn qualify_hooks() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("binary");
	let home = tempfile::tempdir().expect("home");
	let root = home.path().canonicalize().expect("canonical home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
	let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	setup(&root, listener.local_addr().expect("address"));
	let backend = tokio::spawn(serve(listener, calls.clone()));
	let mut session = NativeSession::start(&binary, &root);
	let started = session
		.client
		.thread_start(json!({"cwd":root,"approvalPolicy":"never","sandbox":"read-only"}))
		.await
		.expect("thread");
	let thread = started["thread"]["id"].as_str().expect("thread id").to_owned();
	assert_eq!(turn(&mut session, &thread).await, 0, "untrusted hook must not execute");
	trust_fixture_hook(&session, &root).await;
	assert_eq!(turn(&mut session, &thread).await, 1, "native reload activates reviewed hook");
	change(&session, &root, &thread, HookSettingsChange::Enabled(false)).await;
	assert_eq!(turn(&mut session, &thread).await, 0, "disabled trusted hook must not execute");
	drop(session);
	let mut session = NativeSession::start(&binary, &root);
	session.client.thread_resume(json!({"threadId":thread})).await.expect("resume");
	assert_eq!(turn(&mut session, &thread).await, 0, "disabled setting survives restart");
	change(&session, &root, &thread, HookSettingsChange::Enabled(true)).await;
	assert_eq!(
		turn(&mut session, &thread).await,
		1,
		"enable does not require new trust for unchanged content"
	);
	std::fs::write(root.join("plugins/cache/test/sample/local/hooks/hooks.json"),json!({"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"echo modified-isolated-plugin-hook"}]}]}}).to_string()).expect("modify fixture hook");
	drop(session);
	let mut session = NativeSession::start(&binary, &root);
	session.client.thread_resume(json!({"threadId":thread})).await.expect("resume modified");
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
async fn change(
	session: &NativeSession,
	root: &std::path::Path,
	thread: &str,
	change: HookSettingsChange,
) {
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
