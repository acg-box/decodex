//! Exact packaged runtime qualification through the retained process bridge.
use std::{
	env, fs,
	sync::{Arc, atomic::AtomicUsize},
	time::Duration,
};

use rusqlite::Connection;
use tempfile::TempDir;
use tokio::{net::TcpListener, task::JoinHandle, time};

use crate::{
	account_launch::agent_process::native_tests::{self, NativeSession, ServerEvent, Value},
	agent::{AgentConfig, AgentCoordinator},
};
use decodex_codex::app_server_client::{
	InitializeCapabilities, NativeTaskPermissions, ThreadArchiveState,
};
use decodex_core::{DecodexRoot, guardian::ReviewPreference};
use decodex_database::SqliteStore;

pub(super) async fn setup(
	response: impl Fn(usize) -> Value + Send + Sync + 'static,
) -> (TempDir, NativeSession, SqliteStore, AgentCoordinator, JoinHandle<()>) {
	let binary =
		env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit qualified native binary");
	let home = tempfile::tempdir().expect("Project and Work qualification fixture");
	let listener =
		TcpListener::bind("127.0.0.1:0").await.expect("Project and Work qualification fixture");
	let address = listener.local_addr().expect("Project and Work qualification fixture");
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		Arc::new(AtomicUsize::new(0)),
		None,
		None,
		None,
		response,
	));

	fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\ngoals=true\nguardian_approval=true\ncontent_item_kinds=true\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("Project and Work qualification fixture");

	let session =
		NativeSession::with_capabilities(&binary, home.path(), InitializeCapabilities::for_agent());
	let root = DecodexRoot::new(
		home.path().canonicalize().expect("Project and Work qualification fixture").join("data"),
	)
	.expect("Project and Work qualification fixture");
	let store = SqliteStore::open(&root.paths()).expect("Project and Work qualification fixture");
	let mut config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.path().display().to_string());

	config.sandbox = "read-only".into();

	let coordinator = AgentCoordinator::new(store.clone(), session.client.clone(), config)
		.expect("Project and Work qualification fixture");

	(home, session, store, coordinator, backend)
}

pub(super) async fn finish(session: &mut NativeSession, agent: &mut AgentCoordinator, root: &str) {
	loop {
		let event = time::timeout(Duration::from_secs(30), session.events.recv())
			.await
			.expect("Project and Work qualification fixture")
			.expect("Project and Work qualification fixture");
		let done = matches!(&event,ServerEvent::Notification{method,params} if method=="turn/completed" && params["threadId"]==root);

		agent.handle_event(event).await.expect("Project and Work qualification fixture");

		if done {
			return;
		}
	}
}

fn output(serial: usize) -> Value {
	match serial {
		0 | 2 | 5 =>
			serde_json::json!({"type":"function_call","name":"exec_command","call_id":format!("approval-{serial}"),"arguments":serde_json::json!({"cmd":"exit 0","sandbox_permissions":"require_escalated","justification":"Isolated global review qualification"}).to_string()}),
		3 | 6 =>
			serde_json::json!({"type":"message","role":"assistant","id":format!("review-{serial}"),"content":[{"type":"output_text","text":"{\"outcome\":\"deny\"}"}]}),
		_ =>
			serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Synthetic result"}]}),
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; private review recovery after cold resume"]
async fn native_guardian_details_survive_cold_resume_without_raw_subscription() {
	let (home, mut session, store, mut agent, backend) = setup(|serial| match serial % 3 {
		0 => serde_json::json!({"type":"function_call","name":"exec_command","call_id":format!("approval-{serial}"),"arguments":"{\"cmd\":\"exit 0\",\"sandbox_permissions\":\"require_escalated\",\"justification\":\"Isolated review recovery\"}"}),
		1 => serde_json::json!({"type":"message","role":"assistant","id":format!("review-{serial}"),"content":[{"type":"output_text","text":"{\"outcome\":\"allow\"}"}]}),
		_ => serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Completed"}]}),
	}).await;
	let outcome = time::timeout(Duration::from_secs(90), async {
		store.set_guardian_settings(1, ReviewPreference::AutomaticReview, true).await.unwrap();
		agent.start_agent("sage", "Review recovery").await.unwrap();

		let thread =
			store.get_agent_work_item("sage".into()).await.unwrap().codex_thread_id.unwrap();

		finish(&mut session, &mut agent, &thread).await;

		let before = store.guardian_state().await.unwrap().details;

		assert!(!before.is_empty(), "fresh source has private events");

		drop(agent);
		drop(session);

		let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
		let mut session = NativeSession::with_capabilities(
			&binary,
			home.path(),
			InitializeCapabilities::for_agent(),
		);
		let mut config = AgentConfig::new(
			"gpt-5.6-sol".into(),
			"medium".into(),
			home.path().display().to_string(),
		);

		config.sandbox = "read-only".into();

		let mut agent =
			AgentCoordinator::new(store.clone(), session.client.clone(), config).unwrap();

		agent
			.continue_worker("sage", "Repeat the authorized harmless check after restart")
			.await
			.unwrap();

		finish(&mut session, &mut agent, &thread).await;

		let after = store.guardian_state().await.unwrap().details;

		assert!(after.len() > before.len(), "cold resume must retain the new private output");
		assert!(after.iter().all(|record| record.parent_thread_id.as_deref() == Some(&thread)
			&& record.review_id.is_none()));
	})
	.await;

	backend.abort();
	outcome.unwrap();
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native global review and Project qualification"]
async fn native_project_global_review_and_private_records_cross_retained_bridge() {
	let (home, mut session, store, mut agent, backend) = setup(output).await;

	time::timeout(Duration::from_secs(90),async{
		store.set_guardian_settings(1,ReviewPreference::AskMe,true).await.expect("Project and Work qualification fixture");

		let project=session.client.create_project("Community Market",home.path().to_str().expect("Project and Work qualification fixture"),"native-project-test").await.expect("Project and Work qualification fixture");
		let duplicate=session.client.create_project("Community Market",home.path().to_str().expect("Project and Work qualification fixture"),"native-project-test").await.expect("Project and Work qualification fixture");assert_eq!(project["project"]["id"],duplicate["project"]["id"]);
		let source=session.client.thread_start(serde_json::json!({"projectId":project["project"]["id"],"sandbox":"read-only","approvalPolicy":"never"})).await.expect("Project and Work qualification fixture");assert_eq!(source["thread"]["projectId"],project["project"]["id"]);

		agent.start_agent("sage","Exercise Ask Me").await.expect("Project and Work qualification fixture");let root=store.get_agent_work_item("sage".into()).await.expect("Project and Work qualification fixture").codex_thread_id.expect("Project and Work qualification fixture");

		let before=session.client.configured_task_permissions(&root).expect("Project and Work qualification fixture").0;
		let (id,method,params)=loop{let event=session.events.recv().await.expect("Project and Work qualification fixture");let request=match &event{ServerEvent::Request{id,method,params} if method=="item/commandExecution/requestApproval"=>Some((id.clone(),method.clone(),params.clone())),_=>None};agent.handle_event(event).await.expect("Project and Work qualification fixture");if let Some(r)=request{break r;}};

		store.set_guardian_settings(2,ReviewPreference::AutomaticReview,true).await.expect("Project and Work qualification fixture");agent.sync_global_guardian().await.expect("Project and Work qualification fixture");

		let queued=store.guardian_state().await.expect("Project and Work qualification fixture");assert!(matches!(queued.sources[0].outcome.as_str(),"pending"|"applied"),"{queued:?}");
		let pending=session.client.server_request_guard(&id,&method,&params).expect("global switch must not answer an existing human approval");let _=pending;agent.respond_permission(id,serde_json::json!({"decision":"decline"})).await.expect("Project and Work qualification fixture");finish(&mut session,&mut agent,&root).await;

		while session.client.configured_task_permissions(&root).expect("Project and Work qualification fixture").0.approvals_reviewer!="auto_review" {let event=time::timeout(Duration::from_secs(10),session.events.recv()).await.expect("Project and Work qualification fixture").expect("Project and Work qualification fixture");agent.handle_event(event).await.expect("Project and Work qualification fixture");}

		agent.continue_worker("sage","Exercise Automatic Review").await.expect("Project and Work qualification fixture");finish(&mut session,&mut agent,&root).await;agent.sync_global_guardian().await.expect("Project and Work qualification fixture");

		let state=store.guardian_state().await.expect("Project and Work qualification fixture");assert!(!state.details.is_empty(),"production observer must retain private records");assert!(state.details.iter().all(|d|d.parent_thread_id.as_deref()==Some(&root)&&d.reviewer_thread_id!=root&&d.review_id.is_none()));assert_eq!(state.sources[0].effective_reviewer.as_deref(),Some("auto_review"));
		let after=session.client.configured_task_permissions(&root).expect("Project and Work qualification fixture").0;assert_eq!(before.sandbox_policy,after.sandbox_policy);assert_eq!(before.approval_policy,after.approval_policy);
		let history=session.client.thread_read(serde_json::json!({"threadId":root,"includeTurns":true})).await.expect("Project and Work qualification fixture");assert!(!history.to_string().contains("outcome\\\":\\\"deny"));

		store.set_guardian_settings(3,ReviewPreference::AutomaticReview,false).await.expect("Project and Work qualification fixture");let count=state.details.len();agent.continue_worker("sage","Exercise observer opt out").await.expect("Project and Work qualification fixture");finish(&mut session,&mut agent,&root).await;assert_eq!(store.guardian_state().await.expect("Project and Work qualification fixture").details.len(),count);
	}).await.expect("bounded native qualification");

	backend.abort();
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; retained continuation after native restart"]
async fn native_legacy_continuation_preserves_history_and_permissions_after_restart() {
	let (home, mut session, store, mut agent, backend) = setup(|serial| {
		serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Retained answer"}]})
	})
	.await;
	let outcome = time::timeout(Duration::from_secs(60), async {
		let old = session
			.client
			.thread_start(serde_json::json!({
				"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"on-request",
				"approvalsReviewer":"user","sandbox":"read-only","model":"gpt-5.6-sol",
				"config":{"model_reasoning_effort":"medium"},
				"dynamicTools":[{"name":"agent_create_work","description":"Legacy execution",
					"inputSchema":{"type":"object","properties":{}}}]
			}))
			.await
			.unwrap();
		let thread = old["thread"]["id"].as_str().unwrap();
		let expected = NativeTaskPermissions::from_thread_response(&old).unwrap();

		assert!(expected.profile_id.is_none(), "retain the original unnamed sandbox");

		AgentCoordinator::reserve_root(&store, "sage", "Retain the source").await.unwrap();

		store.bind_agent_thread("sage".into(), thread.into()).await.unwrap();

		let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("data")).unwrap();

		Connection::open(root.paths().product_database_file())
			.unwrap()
			.execute("UPDATE agent_tool_versions SET version=3 WHERE work_id='sage'", [])
			.unwrap();

		let first = session
			.client
			.turn_start(serde_json::json!({
				"threadId":thread,"input":[{"type":"text","text":"Historical user decision"}]
			}))
			.await
			.unwrap();
		let first_turn = first["turn"]["id"].as_str().unwrap();

		finish(&mut session, &mut agent, thread).await;

		let history = session.client.thread_read_turn(thread, first_turn).await.unwrap();

		agent.continue_worker("sage", "Continue the same conversation").await.unwrap();

		finish(&mut session, &mut agent, thread).await;
		drop(agent);
		drop(session);

		let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
		let reopened = SqliteStore::open(&root.paths()).unwrap();
		let mut session = NativeSession::with_capabilities(
			&binary,
			home.path(),
			InitializeCapabilities::for_agent(),
		);
		let mut config = AgentConfig::new(
			"gpt-5.6-sol".into(),
			"medium".into(),
			home.path().display().to_string(),
		);

		config.sandbox = "read-only".into();

		let mut agent =
			AgentCoordinator::new(reopened.clone(), session.client.clone(), config).unwrap();

		agent.continue_worker("sage", "Continue after process restart").await.unwrap();

		finish(&mut session, &mut agent, thread).await;

		let current = reopened.get_agent_work_item("sage".into()).await.unwrap();

		assert_eq!(current.codex_thread_id.as_deref(), Some(thread));
		assert_eq!(reopened.agent_tool_version("sage".into()).await.unwrap(), 3);
		assert!(reopened.agent_previous_threads("sage".into()).await.unwrap().is_empty());
		assert_eq!(
			session.client.thread_archive_state(thread).await.unwrap(),
			ThreadArchiveState::Active
		);

		let resumed = session
			.client
			.thread_resume(serde_json::json!({
				"threadId":thread,"excludeTurns":true
			}))
			.await
			.unwrap();
		let actual = NativeTaskPermissions::from_thread_response(&resumed).unwrap();

		assert_eq!(actual.cwd, expected.cwd);
		assert_eq!(actual.profile_id, expected.profile_id);
		assert_eq!(actual.sandbox_policy, expected.sandbox_policy);
		assert_eq!(actual.approval_policy, expected.approval_policy);
		assert_eq!(actual.approvals_reviewer, expected.approvals_reviewer);
		assert_eq!(resumed["model"], old["model"]);
		assert_eq!(resumed["reasoningEffort"], old["reasoningEffort"]);

		let retained = session.client.thread_read_turn(thread, first_turn).await.unwrap();

		assert_eq!(retained["thread"]["turns"], history["thread"]["turns"]);
	})
	.await;

	backend.abort();
	outcome.expect("bounded retained native continuation");
}
