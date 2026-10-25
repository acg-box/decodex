//! Reconcile retained conversation membership through the actual native Project store.
use super::{NativeSession, project_work};
use crate::agent::{AgentConfig, AgentCoordinator, AgentError};
use decodex_codex::app_server_client::{InitializeCapabilities, NativeTaskPermissions};
use decodex_database::EnqueueAgentEvent;
use std::{env, time::Duration};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native Project reconciliation"]
async fn native_project_reconciles_retained_conversation_and_survives_restart() {
	let (home, mut session, store, mut agent, backend) = project_work::setup(|serial| {
		serde_json::json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Retained result"}]})
	})
	.await;
	let outcome = tokio::time::timeout(Duration::from_secs(60), async {
		let directory = home.path().canonicalize().unwrap().display().to_string();
		let native = session.client.create_project("Market", &directory, "existing-project").await.unwrap();
		let project = native["project"]["id"].as_str().unwrap();
		store.register_project("market".into(), "Old name".into(), directory.clone()).await.unwrap();
		AgentCoordinator::create_conversation(&store, "sage", EnqueueAgentEvent {
			source_event_id: "retained-input".into(),
			work_item_id: "sage".into(),
			event_kind: "user_message".into(),
			payload: "{\"text\":\"Continue the market plan\"}".into(),
		}, Some("market".into())).await.unwrap();
		let started = session.client.thread_start(serde_json::json!({"cwd":directory,"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only","historyMode":"paginated"})).await.unwrap();
		let thread = started["thread"]["id"].as_str().unwrap();
		assert!(started["thread"]["projectId"].is_null());
		session.client.name_thread(thread, "Retained conversation").await.unwrap();
		store.bind_agent_thread("sage".into(), thread.into()).await.unwrap();
		let before = NativeTaskPermissions::from_thread_response(&started).unwrap();
		agent.wake_pending().await.unwrap();
		project_work::finish(&mut session, &mut agent, thread).await;
		let read = session.client.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false})).await.unwrap();
		assert_eq!(read["thread"]["projectId"], project, "retained native membership must follow the registered Project");
		let local = store.projects().await.unwrap();
		assert_eq!(local[0].native_project_id.as_deref(), Some(project));
		assert_eq!(local[0].name, "Market");
		let mut config = AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), directory.clone());
		config.sandbox = "read-only".into();
		drop(agent);
		drop(session);
		let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").unwrap();
		let mut session = NativeSession::with_capabilities(&binary, home.path(), InitializeCapabilities::for_agent());
		let mut agent = AgentCoordinator::new(store.clone(), session.client.clone(), config).unwrap();
		agent.continue_worker("sage", "Resume after process restart").await.unwrap();
		project_work::finish(&mut session, &mut agent, thread).await;
		let resumed = session.client.thread_resume(serde_json::json!({"threadId":thread,"excludeTurns":true})).await.unwrap();
		assert_eq!(resumed["thread"]["projectId"], project);
		assert_eq!(store.get_agent_work_item("sage".into()).await.unwrap().codex_thread_id.as_deref(), Some(thread));
		let after = NativeTaskPermissions::from_thread_response(&resumed).unwrap();
		assert_eq!(before.cwd, after.cwd);
		assert_eq!(before.sandbox_policy, after.sandbox_policy);
		assert_eq!(before.approval_policy, after.approval_policy);
		let page = session.client.request("project/list", serde_json::json!({"cursor":null,"limit":100})).await.unwrap();
		assert_eq!(page["data"].as_array().unwrap().len(), 1, "reuse the native catalog entry");
		assert_eq!(page["data"][0]["id"], project);
		let other = session.client.create_project("Other", &directory, "external-project").await.unwrap();
		let other_id = other["project"]["id"].as_str().unwrap();
		session.client.assign_thread_project(thread, other_id).await.unwrap();
		let latest = session.client.thread_latest_turn_id(thread).await.unwrap();
		assert!(matches!(agent.continue_worker("sage", "Do not overwrite external membership").await, Err(AgentError::ProjectPending(_))));
		agent.enqueue_user_message("sage", "project-conflict", "Keep this request queued").await.unwrap();
		AgentCoordinator::create_conversation(&store, "iris", EnqueueAgentEvent {
			source_event_id: "independent-input".into(), work_item_id: "iris".into(), event_kind: "user_message".into(), payload: "{\"text\":\"Continue independent work\"}".into(),
		}, None).await.unwrap();
		agent.wake_pending().await.unwrap();
		let independent = store.get_agent_work_item("iris".into()).await.unwrap().codex_thread_id.unwrap();
		project_work::finish(&mut session, &mut agent, &independent).await;
		assert_eq!(session.client.thread_latest_turn_id(thread).await.unwrap(), latest);
		let preserved = session.client.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false})).await.unwrap();
		assert_eq!(preserved["thread"]["projectId"], other_id);
	}).await;
	backend.abort();
	outcome.unwrap();
}
