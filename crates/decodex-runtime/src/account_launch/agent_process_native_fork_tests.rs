//! Exercise the public branch commands against the installed native app-server.
use std::{path::Path, sync::atomic::AtomicUsize};

use serde_json::Value;

use crate::account_launch::agent_process::native_tests::cold_settings::recap_socket::{
	self, AgentActionDto, AgentClient, EntityId, Ordering, SqliteStore, WireText,
};
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::{PromptForkBoundary, PromptForkPhase, PromptForkResult};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native provider fork"]
async fn installed_fork_preserves_source_provider_after_default_changes() {
	use crate::account_launch::agent_process::native_tests::{
		Arc, Duration, NativeSession, ServerEvent, env, fs, serve_fixture, time,
	};
	use decodex_codex::app_server_client::ThreadForkBoundary;

	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve_fixture(
		listener,
		requests.clone(),
		None,
		None,
		None,
		|_| serde_json::json!({"type":"message","role":"assistant","id":"answer","content":[{"type":"output_text","text":"Done"}]}),
	));
	let config = format!(
		"model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n[model_providers.fixture]\nname=\"Fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[model_providers.other]\nname=\"Other\"\nbase_url=\"http://127.0.0.1:9\"\nwire_api=\"responses\"\nrequires_openai_auth=false\n"
	);
	fs::write(home.path().join("config.toml"), &config).unwrap();
	let mut session = NativeSession::start(&binary, home.path());
	time::timeout(Duration::from_secs(30), async {
		let client = &session.client;
		let started = client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
		let thread = started["thread"]["id"].as_str().unwrap();
		let turn = client.request("turn/start", serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Reply Done"}]})).await.unwrap();
		while let Some(event) = session.events.recv().await {
			if matches!(event, ServerEvent::Notification {ref method, ref params} if method == "turn/completed" && params["threadId"] == thread) { break; }
		}
		fs::write(home.path().join("config.toml"), config.replacen("model_provider=\"fixture\"", "model_provider=\"other\"", 1)).unwrap();
		let count = requests.load(Ordering::Acquire);
		for before in [true, false] {
			let turn = turn["turn"]["id"].as_str().unwrap();
			let boundary = if before { ThreadForkBoundary::BeforeInput(turn) } else { ThreadForkBoundary::AfterTurn(turn) };
			let fork = client.fork_thread_at_boundary(thread, boundary, client.thread_settings_guard(thread).unwrap()).await.unwrap();
			assert_eq!(fork["modelProvider"], "fixture");
			let read = client.thread_read(serde_json::json!({"threadId":fork["thread"]["id"],"includeTurns":false})).await.unwrap();
			assert_eq!(read["thread"]["modelProvider"], "fixture");
		}
		assert_eq!(requests.load(Ordering::Acquire), count, "fork must not infer");
	}).await.unwrap();
	drop(session);
	backend.abort();
}

pub(super) async fn check(
	client: &AgentClient,
	native: &AppServerClient,
	store: &SqliteStore,
	work: &EntityId,
	thread: &str,
	requests: &AtomicUsize,
	home: &Path,
) {
	let source = native.thread_turns_since(thread, None).await.expect("read source turns");
	let count = requests.load(Ordering::Acquire);
	let mut targets = Vec::new();

	for (index, boundary) in
		[PromptForkBoundary::BeforeInput, PromptForkBoundary::AfterTurn].into_iter().enumerate()
	{
		let selected = source[0]["id"].as_str().expect("source turn identity");
		let items =
			native.thread_read_turn_items(thread, selected).await.expect("read source input items");
		let input = items
			.as_array()
			.expect("source item array")
			.iter()
			.find(|item| item["type"] == "userMessage")
			.expect("selected user input");
		let target = EntityId::new(format!("fork-{index}")).expect("target work identity");

		prepare_review(
			client,
			work,
			thread,
			selected,
			input["id"].as_str().expect("source input identity"),
			index,
		)
		.await;

		let (review, content) = client
			.prompt_edit(work.clone(), WireText::new(thread).expect("source thread identity"))
			.await
			.expect("read prepared review");
		let token = review.evidence.expect("prepared review evidence").review_token;
		let action = AgentActionDto::ForkPromptEdit {
			work_id: work.clone(),
			thread_id: WireText::new(thread).expect("source thread identity"),
			review_token: token.clone(),
			target_work_id: target.clone(),
			boundary,
		};

		for retry in 0..2 {
			recap_socket::accepted(
				client,
				action.clone(),
				&format!("fork-confirm-{index}-{retry}"),
			)
			.await;
		}

		let PromptForkResult::Available(Some(receipt)) = client
			.prompt_fork(work.clone(), token.clone())
			.await
			.expect("read durable fork receipt")
		else {
			panic!("durable branch receipt")
		};

		assert_eq!(receipt.phase, PromptForkPhase::Forked, "{receipt:?}");

		let fork = receipt.target_thread_id.expect("acknowledged fork thread");

		assert_ne!(fork.as_str(), thread);
		assert!(!targets.contains(&fork));

		targets.push(fork.clone());

		assert_fork_lineage(native, thread, &fork, boundary, &source).await;

		if boundary == PromptForkBoundary::BeforeInput {
			let (status, restored) = client
				.prompt_edit(target.clone(), fork.clone())
				.await
				.expect("read branch input receipt");

			assert_eq!(restored, content);
			assert_eq!(status.phase, decodex_protocol::PromptEditPhase::Applied);

			recap_socket::qualify_prompt_acknowledgement(
				client,
				status,
				&content.expect("canonical reviewed input"),
				home,
			)
			.await;
		}

		recap_socket::accepted(
			client,
			AgentActionDto::RecoverPromptFork { work_id: work.clone(), review_token: token },
			&format!("fork-recover-{index}"),
		)
		.await;

		assert_eq!(
			requests.load(Ordering::Acquire),
			count,
			"branch/recovery/handback must not infer"
		);
	}

	assert_eq!(
		store.list_agent_work_items().await.expect("read reserved work items").len(),
		3,
		"retries must not reserve extra work"
	);
}

async fn assert_fork_lineage(
	native: &AppServerClient,
	thread: &str,
	fork: &WireText,
	boundary: PromptForkBoundary,
	source: &[Value],
) {
	let turns = native.thread_turns_since(fork.as_str(), None).await.expect("read fork prefix");

	assert_eq!(
		turns.len(),
		usize::from(boundary == decodex_protocol::PromptForkBoundary::AfterTurn)
	);
	assert_eq!(
		native.thread_turns_since(thread, None).await.expect("reread unchanged source"),
		source
	);

	let metadata = native
		.thread_read(serde_json::json!({"threadId":fork.as_str()}))
		.await
		.expect("read fork lineage");

	assert_eq!(metadata["thread"]["forkedFromId"], thread);
}

async fn prepare_review(
	client: &AgentClient,
	work: &EntityId,
	thread: &str,
	selected: &str,
	input_id: &str,
	index: usize,
) {
	recap_socket::accepted(
		client,
		AgentActionDto::PreparePromptEdit {
			work_id: work.clone(),
			thread_id: WireText::new(thread).expect("source thread identity"),
			turn_id: WireText::new(selected).expect("selected turn identity"),
			item_id: WireText::new(input_id).expect("bounded source input identity"),
		},
		&format!("fork-review-{index}"),
	)
	.await;
}
