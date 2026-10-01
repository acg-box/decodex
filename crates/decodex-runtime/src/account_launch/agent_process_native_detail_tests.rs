//! Read complete native patches through the retained bridge, including after cold restart.
use std::{env, fs, sync::atomic::AtomicUsize};

use tokio::{net::TcpListener, time};

use crate::{
	account_launch::agent_process::native_tests::{
		self, AppServerClient, Arc, Duration, NativeSession, Ordering, ServerEvent,
	},
	agent_detail,
	agent_usage_estimate::{Source, SourceKey},
};
use decodex_core::{AccountId, ProcessGenerationId};
use decodex_protocol::{AgentActivityDetailCursor, AgentActivityDetailResult};

fn source_key(thread: &str, generation: u8) -> SourceKey {
	SourceKey {
		generation: ProcessGenerationId::new(format!("10000000-0000-4000-8000-{generation:012}"))
			.expect("fixture process generation"),
		account: AccountId::new("10000000-0000-4000-8000-000000000001")
			.expect("retained bridge fixture account"),
		revision: 1,
		history_revision: 0,
		thread: thread.into(),
		work: "work".into(),
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native patch continuation"]
async fn installed_native_patch_pages_survive_cold_restart_without_replay() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let body = format!("{}final patch line\n", "Unicode 界🙂e\u{301} fixture line\n".repeat(2_500));
	let patch = format!(
		"*** Begin Patch\n*** Add File: large.txt\n{}*** End Patch",
		body.lines().map(|line| format!("+{line}\n")).collect::<String>()
	);
	let backend = tokio::spawn(native_tests::serve_fixture_usage(
		listener,
		requests.clone(),
		None,
		None,
		|_| serde_json::json!({"input_tokens":0,"output_tokens":0,"total_tokens":0}),
		move |serial| {
			if serial == 0 {
				serde_json::json!({"type":"custom_tool_call","name":"apply_patch","input":patch,"call_id":"large-patch"})
			} else {
				serde_json::json!({"type":"message","role":"assistant","id":"answer","content":[{"type":"output_text","text":"Patch complete"}]})
			}
		},
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Isolated patch fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	let mut session = NativeSession::start(&binary, home.path());
	let (thread, turn, item, text, cursor) = time::timeout(Duration::from_secs(60), async {
		let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"workspace-write"})).await.unwrap();
		let thread = started["thread"]["id"].as_str().unwrap().to_owned();
		let turn = session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Create the fixture patch."}]})).await.unwrap();
		let turn = turn["turn"]["id"].as_str().unwrap().to_owned();

		loop {
			if let ServerEvent::Notification { method, params } = session.events.recv().await.unwrap()
				&& method == "turn/completed" && params["turn"]["id"] == turn {
				assert_eq!(params["turn"]["status"], "completed");

				break;
			}
		}

		assert_eq!(fs::read_to_string(home.path().join("large.txt")).unwrap(), body);

		let history = session.client.thread_read_turn(&thread, &turn).await.unwrap();
		let item = history["thread"]["turns"][0]["items"].as_array().unwrap().iter()
			.find(|item| item["type"] == "fileChange").expect("native patch history");
		let item = item["id"].as_str().unwrap().to_owned();
		let key = source_key(&thread, 1);
		let (text, cursor) = complete(&session.client, &key, &turn, &item).await;

		assert!(text.len() > 64 * 1_024);
		assert!(text.contains("final patch line"));
		assert_eq!(text.matches("Unicode 界🙂e\u{301} fixture line").count(), 2_500);

		(thread, turn, item, text, cursor)
	}).await.expect("native patch completion");

	assert_eq!(requests.load(Ordering::Acquire), 2);

	drop(session);

	let reopened = NativeSession::start(&binary, home.path());

	time::timeout(Duration::from_secs(60), async {
		let key = source_key(&thread, 2);

		assert_eq!(
			read(&reopened.client, &key, &turn, &item, Some(&cursor)).await,
			AgentActivityDetailResult::Unavailable,
			"old process continuation must expire"
		);
		assert_eq!(complete(&reopened.client, &key, &turn, &item).await.0, text);
		assert_eq!(requests.load(Ordering::Acquire), 2, "reads and restart must not run inference");
	})
	.await
	.expect("native cold patch read");

	drop(reopened);

	backend.abort();
}

async fn read(
	client: &AppServerClient,
	key: &SourceKey,
	turn: &str,
	item: &str,
	cursor: Option<&AgentActivityDetailCursor>,
) -> AgentActivityDetailResult {
	agent_detail::read_bound(
		|| {
			let client = client.clone();
			let key = key.clone();

			async move { Some(Source { client, key }) }
		},
		turn,
		item,
		cursor,
	)
	.await
}

async fn complete(
	client: &AppServerClient,
	key: &SourceKey,
	turn: &str,
	item: &str,
) -> (String, AgentActivityDetailCursor) {
	let mut cursor = None;
	let mut first = None;
	let mut text = String::new();

	for _ in 0..64 {
		let result = read(client, key, turn, item, cursor.as_ref()).await;
		let AgentActivityDetailResult::Available { text: portion, offset, next, truncated } =
			result
		else {
			panic!("native patch detail unavailable: {result:?}");
		};

		assert_eq!(offset as usize, text.len());
		assert!(!portion.is_empty() && portion.len() <= 8 * 1_024);
		assert_eq!(truncated, next.is_some());

		text.push_str(&portion);

		let Some(next) = next else {
			return (text, first.expect("multiple pages"));
		};

		first.get_or_insert(next.clone());

		cursor = Some(next);
	}

	panic!("patch paging did not terminate");
}
