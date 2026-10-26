//! Exercise the public branch commands against an explicitly prepared native app-server.
use std::{collections::HashSet, ffi::OsStr, path::Path, sync::atomic::AtomicUsize};

use serde_json::Value;
use tokio::net::TcpListener;

use crate::account_launch::agent_process::native_tests::{
	self, Arc, Duration, NativeSession, ServerEvent,
	cold_settings::recap_socket::{
		self, AgentActionDto, AgentClient, EntityId, Ordering, SqliteStore, WireText,
	},
	env, fs, time,
};
use decodex_codex::app_server_client::{AppServerClient, ThreadForkBoundary};
use decodex_protocol::{
	AgentDispatchStateDto, AgentSnapshotResult, AttachedBranchesResult, HistoryText,
	PromptForkBoundary, PromptForkPhase, PromptForkResult,
};

pub(super) async fn settled_parent(client: &AgentClient) -> String {
	if let Ok(thread) = time::timeout(Duration::from_secs(20), recap_socket::settled(client)).await
	{
		return thread;
	}

	let snapshot = client.query().await;
	let history = client.history(EntityId::new("recap-root").expect("fixture source")).await;

	panic!("Native fork source did not settle: {snapshot:?}; history: {history:?}");
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

	for (index, boundary) in [
		PromptForkBoundary::BeforeInput,
		PromptForkBoundary::AfterTurn,
		PromptForkBoundary::AttachedAfterTurn,
	]
	.into_iter()
	.enumerate()
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

	assert_attachment_index(client, store, work, targets.last()).await;
	continue_attached_branch(
		client,
		native,
		thread,
		targets.last().expect("native fork fixture").as_str(),
		&source,
		requests,
	)
	.await;
}

async fn continue_attached_branch(
	client: &AgentClient,
	native: &AppServerClient,
	source_thread: &str,
	branch_thread: &str,
	source: &[Value],
	requests: &AtomicUsize,
) {
	let before = requests.load(Ordering::Acquire);
	let target = EntityId::new("fork-2").expect("attached branch work");

	recap_socket::accepted(
		client,
		AgentActionDto::Send {
			root_id: target.clone(),
			text: HistoryText::new(
				"Explicitly continue the attached branch through its existing input owner.",
			)
			.expect("branch input"),
		},
		"attached-branch-explicit-input",
	)
	.await;

	time::timeout(Duration::from_secs(25), async {
		loop {
			if let AgentSnapshotResult::Available(snapshot) =
				client.query().await.expect("public branch snapshot")
				&& let Some(work) =
					snapshot.work_items.iter().find(|work| work.id == target.as_str())
				&& work.codex_thread_id.as_deref() == Some(branch_thread)
				&& work.dispatch_state == AgentDispatchStateDto::Idle
				&& !snapshot.pending_events.iter().any(|event| {
					event.work_item_id == target.as_str() && event.event_kind == "user_message"
				})
				&& requests.load(Ordering::Acquire) == before + 1
			{
				break;
			}

			time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("explicit branch input must settle in its existing owner");

	assert_eq!(
		native
			.thread_turns_since(branch_thread, None)
			.await
			.expect("continued branch history")
			.len(),
		2
	);
	assert_eq!(
		native.thread_turns_since(source_thread, None).await.expect("unchanged source history"),
		source
	);
}

async fn assert_attachment_index(
	client: &AgentClient,
	store: &SqliteStore,
	work: &EntityId,
	target: Option<&WireText>,
) {
	assert_eq!(
		store.list_agent_work_items().await.expect("read reserved work items").len(),
		4,
		"retries must not reserve extra work"
	);

	let AttachedBranchesResult::Available { branches, truncated } =
		client.attached_branches(work.clone()).await.expect("read attachment index")
	else {
		panic!("attachment index unavailable")
	};

	assert!(!truncated);
	assert_eq!(branches.len(), 1);
	assert_eq!(branches[0].boundary, PromptForkBoundary::AttachedAfterTurn);
	assert_eq!(branches[0].target_thread_id.as_ref(), target);
	assert_eq!(branches[0].phase, PromptForkPhase::Forked);
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native provider fork"]
async fn installed_fork_preserves_source_provider_after_default_changes() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	for history_mode in ["legacy", "paginated"] {
		qualify_native_fork(&binary, history_mode).await;
	}
}

async fn qualify_native_fork(binary: &OsStr, history_mode: &str) {
	let home = tempfile::tempdir().expect("native fork fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("native fork fixture");
	let address = listener.local_addr().expect("native fork fixture");
	let requests = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		requests.clone(),
		None,
		None,
		None,
		|_| serde_json::json!({"type":"message","role":"assistant","id":"answer","content":[{"type":"output_text","text":"Done"}]}),
	));
	let config = format!(
		"model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n[features]\ngoals=true\n[model_providers.fixture]\nname=\"Fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[model_providers.other]\nname=\"Other\"\nbase_url=\"http://127.0.0.1:9\"\nwire_api=\"responses\"\nrequires_openai_auth=false\n"
	);

	fs::write(home.path().join("config.toml"), &config).expect("native fork fixture");

	let mut session = NativeSession::start(binary, home.path());
	let mut attached = None;

	time::timeout(Duration::from_secs(30), async {
		let client = &session.client;
		let started = client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":history_mode,"approvalPolicy":"never","sandbox":"read-only"})).await.expect("native fork fixture");
		let thread = started["thread"]["id"].as_str().expect("native fork fixture");
		let turn = client.request("turn/start", serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Reply Done"}]})).await.expect("native fork fixture");

		while let Some(event) = session.events.recv().await {
			if matches!(event, ServerEvent::Notification {ref method, ref params} if method == "turn/completed" && params["threadId"] == thread) { break; }
		}

		client.request("thread/goal/set", serde_json::json!({"threadId":thread,"objective":"Keep the source goal","status":"paused","tokenBudget":12_345})).await.expect("native fork fixture");

		let source_goal = client.thread_goal(thread).await.expect("native fork fixture");

		fs::write(home.path().join("config.toml"), config.replacen("model_provider=\"fixture\"", "model_provider=\"other\"", 1)).expect("native fork fixture");

		let count = requests.load(Ordering::Acquire);

		for mode in 0..3 {
			let turn = turn["turn"]["id"].as_str().expect("native fork fixture");
			let boundary = match mode { 0 => ThreadForkBoundary::BeforeInput(turn), 1 => ThreadForkBoundary::AfterTurn(turn), _ => ThreadForkBoundary::AttachedAfterTurn(turn) };
			let fork = client.fork_thread_at_boundary(thread, boundary, client.thread_settings_guard(thread).expect("native fork fixture")).await.expect("native fork fixture");

			assert_eq!(fork["modelProvider"], "fixture");

			let read = client.thread_read(serde_json::json!({"threadId":fork["thread"]["id"],"includeTurns":false})).await.expect("native fork fixture");

			assert_eq!(read["thread"]["modelProvider"], "fixture");

			if mode == 2 {
				assert!(client.thread_goal(fork["thread"]["id"].as_str().expect("native fork fixture")).await.expect("native fork fixture").is_none(), "attached branches start without a copied goal");
				assert_eq!(client.thread_goal(thread).await.expect("native fork fixture"), source_goal);

				attached = Some((thread.to_owned(), fork["thread"]["id"].as_str().expect("native fork fixture").to_owned()));
			}
		}

		assert_eq!(requests.load(Ordering::Acquire), count, "fork must not infer");
	}).await.expect("native fork fixture");

	drop(session);

	let (parent, fork) = attached.expect("native fork fixture");
	let mut restarted = NativeSession::start(binary, home.path());

	time::timeout(Duration::from_secs(30), async {
		let client = &restarted.client;
		let count = requests.load(Ordering::Acquire);
		let resumed = client.thread_resume(serde_json::json!({"threadId":fork,"excludeTurns":true})).await.expect("native fork fixture");

		assert_eq!(resumed["modelProvider"], "fixture");
		assert_eq!(client.thread_archive_state(&fork).await.expect("native fork fixture"), decodex_codex::app_server_client::ThreadArchiveState::Active);
		assert!(client.request("thread/delete",serde_json::json!({"threadId":parent})).await.is_err(), "native parent deletion must retain attached children");

		client.request("thread/archive",serde_json::json!({"threadId":fork})).await.expect("native fork fixture");

		let observed = client.observe_thread_archives(&HashSet::from([fork.clone()])).await;

		assert_eq!(observed.get(&fork),Some(&decodex_codex::app_server_client::ThreadArchiveState::Archived));

		client.thread_unarchive(&fork).await.expect("native fork fixture");

		assert_eq!(client.attached_thread_archive_state(&fork,&parent).await.expect("native fork fixture"),decodex_codex::app_server_client::ThreadArchiveState::Active);

		let resumed = client.thread_resume(serde_json::json!({"threadId":fork,"excludeTurns":true})).await.expect("native fork fixture");

		assert_eq!(resumed["modelProvider"], "fixture");
		assert_eq!(requests.load(Ordering::Acquire),count,"fork discovery, restart and archive controls must not infer");

		client.request("turn/start",serde_json::json!({"threadId":fork,"input":[{"type":"text","text":"Explicitly continue this attached branch"}]})).await.expect("native fork fixture");

		while let Some(event)=restarted.events.recv().await {
			if matches!(event, ServerEvent::Notification {ref method, ref params} if method=="turn/completed" && params["threadId"]==fork) {break;}
		}

		assert_eq!(requests.load(Ordering::Acquire),count+1);
		assert_eq!(client.thread_turns_since(&parent,None).await.expect("native fork fixture").len(),1,"branch input must preserve the original history");
	}).await.expect("native fork fixture");

	drop(restarted);

	backend.abort();
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
		usize::from(boundary != decodex_protocol::PromptForkBoundary::BeforeInput)
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
