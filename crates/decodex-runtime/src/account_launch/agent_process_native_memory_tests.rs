//! Qualify read-only memory version selection against the exact prepared native artifact.
use std::{
	env, fs,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
};

use tokio::net::TcpListener;

use crate::account_launch::agent_process::native_tests::{self, NativeSession};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native memory read"]
async fn installed_native_memory_view_selects_versions_without_model_calls_or_fallback() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("backend");
	let address = listener.local_addr().expect("address");
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_tests::serve(listener, calls.clone()));

	for (version, legacy, use_memories, expected) in [
		(Some("v1"), true, true, "memories"),
		(Some("v2"), true, true, "memories_v2"),
		(None, true, true, "memories"),
		(None, false, true, "memories_v2"),
		(Some("v2"), true, false, "memories_v2"),
	] {
		let home = tempfile::tempdir().expect("isolated native home");

		fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nmemories=true\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("config");

		for directory in ["memories", "memories_v2"] {
			if directory == "memories" && !legacy {
				continue;
			}

			fs::create_dir(home.path().join(directory)).expect("memory directory");
			fs::write(
				home.path().join(directory).join("memory_summary.md"),
				format!("v1\n{directory} only 界\n"),
			)
			.expect("summary");
		}

		let session = NativeSession::start(&binary, home.path());
		let mut config = serde_json::json!({"memories.generate_memories":false,"memories.use_memories":use_memories});

		if let Some(version) = version {
			config["memories.version"] = serde_json::json!(version);
		}

		let started=session.client.thread_start(serde_json::json!({"cwd":home.path(),"ephemeral":true,"config":config,"approvalPolicy":"never","sandbox":"read-only"})).await.expect("native thread");
		let thread = started["thread"]["id"].as_str().expect("thread");
		let summary = session.client.memory_summary(thread).await.expect("native memory read");

		assert_eq!(summary, use_memories.then(|| format!("v1\n{expected} only 界\n")));

		fs::remove_file(home.path().join(expected).join("memory_summary.md"))
			.expect("remove selected summary");

		assert_eq!(
			session.client.memory_summary(thread).await.expect("missing read"),
			None,
			"no fallback to the other memory version"
		);

		fs::write(home.path().join(expected).join("memory_summary.md"), " \n")
			.expect("blank summary");

		assert_eq!(session.client.memory_summary(thread).await.expect("blank read"), None);
		assert!(
			session.client.memory_summary("10000000-0000-4000-8000-000000000001").await.is_err(),
			"inspection never resumes an unloaded conversation"
		);
	}

	assert_eq!(calls.load(Ordering::Acquire), 0, "memory inspection must not start model work");

	backend.abort();
}
