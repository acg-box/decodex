use super::{
	hooks::{owner, setup},
	*,
};
use crate::AgentAppUiCallAttempt;
use serde_json::json;

fn attempt(token: char) -> AgentAppUiCallAttempt {
	AgentAppUiCallAttempt {
		owner: owner(1),
		turn: "turn".into(),
		item: "widget".into(),
		server: "widget-server".into(),
		tool: "update".into(),
		arguments: json!({"text":"x".repeat(70000)}),
		source_fingerprint: "a".repeat(64),
		review_token: token.to_string().repeat(64),
		attempt_id: format!("attempt-{token}"),
	}
}

// These rows were written by the retired embedded App UI executor.
async fn seed_call(
	store: &SqliteStore,
	attempt: AgentAppUiCallAttempt,
	outcome: Option<(String, Option<serde_json::Value>)>,
) -> i64 {
	store.run(move |connection| {
        let source = format!("app-ui-tool:{}", attempt.attempt_id);
        let summary = json!({"owner":attempt.owner,"turn":attempt.turn,"item":attempt.item,
            "server":attempt.server,"tool":attempt.tool,"attempt_id":attempt.attempt_id,
            "review_token":attempt.review_token,"detailsStored":true}).to_string();
        connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'app_ui_tool_attempt',?3,1,'resolved','reserved',1)",rusqlite::params![source,attempt.owner.work,summary]).unwrap();
        let id = connection.last_insert_rowid();
        connection.execute("INSERT INTO agent_request_payloads(event_id,payload) VALUES(?1,?2)",rusqlite::params![id,serde_json::to_string(&attempt).unwrap()]).unwrap();
        if let Some((state, result)) = outcome {
            connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'app_ui_tool_result','{}',2,'resolved',?3,2)",rusqlite::params![format!("{source}:result"),attempt.owner.work,state]).unwrap();
            connection.execute("INSERT INTO agent_request_payloads(event_id,payload) VALUES(?1,?2)",rusqlite::params![connection.last_insert_rowid(),json!({"result":result}).to_string()]).unwrap();
        }
        Ok(id)
    }).await.unwrap()
}

#[tokio::test]
async fn historical_unknown_app_ui_calls_preserve_evidence_and_acknowledgment() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("app-ui.sqlite3");
	let store = setup(&path).await;
	let a = attempt('b');
	let id = seed_call(&store, a.clone(), Some(("unknown".into(), None))).await;
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let unknown = store
		.agent_app_ui_call_receipt(a.owner.work.clone(), a.attempt_id.clone())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(unknown.attempt, a);
	assert_eq!(unknown.state, "unknown");
	assert!(!unknown.uncertainty_acknowledged);
	assert_eq!(
		store.pending_agent_app_ui_call(a.owner.work.clone()).await.unwrap().unwrap().id,
		id
	);
	for (work, operation) in
		[(owner(2).work, a.attempt_id.clone()), (a.owner.work.clone(), "wrong".into())]
	{
		assert!(!store.acknowledge_agent_app_ui_uncertainty(work, id, operation).await.unwrap());
	}
	assert!(
		store
			.acknowledge_agent_app_ui_uncertainty(a.owner.work.clone(), id, a.attempt_id.clone())
			.await
			.unwrap()
	);
	assert!(
		!store
			.acknowledge_agent_app_ui_uncertainty(a.owner.work.clone(), id, a.attempt_id.clone())
			.await
			.unwrap()
	);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let receipt = store
		.agent_app_ui_call_receipt(a.owner.work.clone(), a.attempt_id.clone())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(receipt.attempt, a);
	assert_eq!(receipt.state, "unknown");
	assert!(receipt.uncertainty_acknowledged);
	assert!(receipt.result.is_none());
	assert!(store.pending_agent_app_ui_call(a.owner.work).await.unwrap().is_none());
}

#[tokio::test]
async fn app_ui_results_preserve_large_content_and_exact_receipt_identity() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("app-ui-result.sqlite3");
	let store = setup(&path).await;
	let a = attempt('d');
	let result = json!({"content":[{"type":"text","text":"y".repeat(70000)}],"structuredContent":{"value":7},"_meta":{"view":"retained"}});
	let id = seed_call(&store, a.clone(), Some(("completed".into(), Some(result.clone())))).await;

	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let receipt = store
		.agent_app_ui_call_receipt(a.owner.work.clone(), a.attempt_id.clone())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(receipt.result, Some(result));
	assert_eq!(receipt.state, "completed");
	assert!(
		store
			.agent_app_ui_call_receipt(owner(2).work, a.attempt_id.clone())
			.await
			.unwrap()
			.is_none()
	);
	assert!(
		!store.acknowledge_agent_app_ui_uncertainty(a.owner.work, id, a.attempt_id).await.unwrap()
	);
}

#[tokio::test]
async fn unfinished_app_call_requires_positive_process_death_before_recovery() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("recovery.sqlite3");
	let store = setup(&path).await;
	let a = attempt('f');
	let id = seed_call(&store, a.clone(), None).await;
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let reserved = store
		.agent_app_ui_call_receipt(a.owner.work.clone(), a.attempt_id.clone())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(reserved.attempt, a);
	assert_eq!(reserved.state, "reserved");
	assert!(reserved.result.is_none());
	assert!(!reserved.uncertainty_acknowledged);
	assert!(
		!store.recover_agent_app_ui_call(a.owner.work.clone(), a.attempt_id.clone()).await.unwrap()
	);
	store
		.mark_process_generation_death_unknown(
			&generation_id(1),
			3,
			decodex_core::ProcessAuthorityLossReason::SupervisorRestarted,
		)
		.await
		.unwrap();
	assert!(
		!store.recover_agent_app_ui_call(a.owner.work.clone(), a.attempt_id.clone()).await.unwrap()
	);
	let evidence = ProcessDeathEvidence::new(
		ProcessDeathEvidenceId::new("50000000-0000-4000-8000-000000000001").unwrap(),
		generation_id(1),
		ProcessDeathEvidenceKind::OwnedChildExit,
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		Some(super::hooks::identity(123)),
		DIGEST,
	)
	.unwrap();
	store.record_process_generation_death(4, &evidence).await.unwrap();
	assert!(
		store.recover_agent_app_ui_call(a.owner.work.clone(), a.attempt_id.clone()).await.unwrap()
	);
	assert!(
		!store.recover_agent_app_ui_call(a.owner.work.clone(), a.attempt_id.clone()).await.unwrap()
	);
	let receipt =
		store.agent_app_ui_call_receipt(a.owner.work, a.attempt_id).await.unwrap().unwrap();
	assert_eq!(receipt.id, id);
	assert_eq!(receipt.state, "unknown");
	assert!(receipt.result.is_none());
}
