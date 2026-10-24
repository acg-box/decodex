//! Exact packaged runtime qualification through the retained process bridge.
use super::{NativeSession, ServerEvent, Value, serve_fixture};
use crate::agent::{AgentConfig, AgentCoordinator};
use decodex_codex::app_server_client::{
	InitializeCapabilities, NativeTaskPermissions, ThreadArchiveState,
};
use decodex_core::guardian::ReviewPreference;
use decodex_database::SqliteStore;
use std::{
	env, fs,
	sync::{Arc, atomic::AtomicUsize},
	time::Duration,
};
use tokio::{net::TcpListener, time};

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
async fn setup(
	response: impl Fn(usize) -> Value + Send + Sync + 'static,
) -> (tempfile::TempDir, NativeSession, SqliteStore, AgentCoordinator, tokio::task::JoinHandle<()>)
{
	let binary =
		env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit qualified native binary");
	let home = tempfile::tempdir().expect("Project and Work qualification fixture");
	let listener =
		TcpListener::bind("127.0.0.1:0").await.expect("Project and Work qualification fixture");
	let address = listener.local_addr().expect("Project and Work qualification fixture");
	let backend = tokio::spawn(serve_fixture(
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
	let root = decodex_core::DecodexRoot::new(
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
async fn finish(session: &mut NativeSession, agent: &mut AgentCoordinator, root: &str) {
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
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated immutable native tool migration"]
async fn native_legacy_tools_migrate_once_and_retire_the_old_context() {
	let (home,mut session,store,mut agent,backend)=setup(|serial|serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Retained answer"}]})).await;
	time::timeout(Duration::from_secs(60),async{
		let old=session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"on-request","approvalsReviewer":"user","permissions":":read-only","model":"gpt-5.6-sol","config":{"model_reasoning_effort":"medium"},"dynamicTools":[{"name":"agent_create_work","description":"Legacy managed execution","inputSchema":{"type":"object","properties":{}}}]})).await.expect("Project and Work qualification fixture");
		let old_id=old["thread"]["id"].as_str().expect("Project and Work qualification fixture");AgentCoordinator::reserve_root(&store,"sage","Retain the source").await.expect("Project and Work qualification fixture");store.bind_agent_thread("sage".into(),old_id.into()).await.expect("Project and Work qualification fixture");
		rusqlite::Connection::open(decodex_core::DecodexRoot::new(home.path().canonicalize().expect("Project and Work qualification fixture").join("data")).expect("Project and Work qualification fixture").paths().product_database_file()).expect("Project and Work qualification fixture").execute("UPDATE agent_tool_versions SET version=3 WHERE work_id='sage'",[]).expect("Project and Work qualification fixture");
		let turn=session.client.turn_start(serde_json::json!({"threadId":old_id,"input":[{"type":"text","text":"Historical user decision"}]})).await.expect("Project and Work qualification fixture");let _=turn;finish(&mut session,&mut agent,old_id).await;
		let goal=session.client.thread_goal(old_id).await;assert!(goal.is_ok(),"Native Goal read: {goal:?}");
		let background=session.client.request("thread/backgroundTerminals/list",serde_json::json!({"threadId":old_id,"limit":1})).await;assert!(background.is_ok(),"Native background read: {background:?}");
		agent.continue_worker("sage","Continue through the new Work contract").await.expect("Project and Work qualification fixture");let current=store.get_agent_work_item("sage".into()).await.expect("Project and Work qualification fixture").codex_thread_id.expect("Project and Work qualification fixture");assert_ne!(current,old_id);finish(&mut session,&mut agent,&current).await;
		assert_eq!(session.client.thread_archive_state(old_id).await.expect("Project and Work qualification fixture"),ThreadArchiveState::Archived);assert_eq!(store.agent_previous_threads("sage".into()).await.expect("Project and Work qualification fixture"),vec![old_id.to_owned()]);assert_eq!(store.agent_tool_version("sage".into()).await.expect("Project and Work qualification fixture"),4);
		let actual=session.client.thread_resume(serde_json::json!({"threadId":current,"excludeTurns":true})).await.expect("Project and Work qualification fixture");let expected=NativeTaskPermissions::from_thread_response(&old).expect("Project and Work qualification fixture");let migrated=NativeTaskPermissions::from_thread_response(&actual).expect("Project and Work qualification fixture");assert_eq!(expected.profile_id,migrated.profile_id);assert_eq!(expected.sandbox_policy,migrated.sandbox_policy);assert_eq!(actual["reasoningEffort"],"medium");
		agent.continue_worker("sage","Second continuation uses the same native context").await.expect("Project and Work qualification fixture");assert_eq!(store.get_agent_work_item("sage".into()).await.expect("Project and Work qualification fixture").codex_thread_id.as_deref(),Some(current.as_str()));finish(&mut session,&mut agent,&current).await;
	}).await.expect("bounded native migration");
	backend.abort();
}
