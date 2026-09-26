use super::{
	hooks::{owner, setup},
	*,
};
use crate::ChiefAppUiCallAttempt;
use serde_json::json;

fn attempt(token: char) -> ChiefAppUiCallAttempt {
	ChiefAppUiCallAttempt {
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

#[tokio::test]
async fn app_ui_calls_reserve_once_and_preserve_unknown_after_reopen() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("app-ui.sqlite3");
	let store = setup(&path).await;
	let a = attempt('b');
	let mut wrong = a.clone();
	wrong.owner.account = owner(2).account;
	assert!(store.reserve_chief_app_ui_call(wrong).await.is_err());
	let (first, second) = tokio::join!(
		store.reserve_chief_app_ui_call(a.clone()),
		store.reserve_chief_app_ui_call(a.clone())
	);
	assert_ne!(first.as_ref().unwrap().is_some(), second.as_ref().unwrap().is_some());
	let id = first.unwrap().or(second.unwrap()).unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let receipt = store
		.chief_app_ui_call_receipt(a.owner.work.clone(), a.attempt_id.clone())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(receipt.attempt, a);
	assert_eq!(receipt.state, "reserved");
	assert!(!receipt.uncertainty_acknowledged);
	assert!(store.reserve_chief_app_ui_call(attempt('c')).await.unwrap().is_none());
	assert!(
		!store.finish_chief_app_ui_call(id, "wrong".into(), "unknown".into(), None).await.unwrap()
	);
	assert!(
		store
			.finish_chief_app_ui_call(id, a.attempt_id.clone(), "unknown".into(), None)
			.await
			.unwrap()
	);
	assert!(
		!store
			.finish_chief_app_ui_call(
				id,
				a.attempt_id.clone(),
				"completed".into(),
				Some(json!({"content":[]}))
			)
			.await
			.unwrap()
	);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let unknown = store
		.chief_app_ui_call_receipt(a.owner.work.clone(), a.attempt_id.clone())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(unknown.state, "unknown");
	assert_eq!(
		store.pending_chief_app_ui_call(a.owner.work.clone()).await.unwrap().unwrap().id,
		id
	);
	assert!(store.reserve_chief_app_ui_call(attempt('c')).await.unwrap().is_none());
	assert!(
		!store
			.acknowledge_chief_app_ui_uncertainty(owner(2).work, id, a.attempt_id.clone())
			.await
			.unwrap()
	);
	assert!(
		store
			.acknowledge_chief_app_ui_uncertainty(a.owner.work.clone(), id, a.attempt_id.clone())
			.await
			.unwrap()
	);
	assert!(
		store.reserve_chief_app_ui_call(a.clone()).await.unwrap().is_none(),
		"acknowledgment cannot authorize replay"
	);
	let receipt =
		store.chief_app_ui_call_receipt(a.owner.work, a.attempt_id).await.unwrap().unwrap();
	assert_eq!(receipt.state, "unknown");
	assert!(
		store
			.pending_chief_app_ui_call(receipt.attempt.owner.work.clone())
			.await
			.unwrap()
			.is_none()
	);
	assert!(receipt.uncertainty_acknowledged);
	assert!(receipt.result.is_none());
	assert!(store.reserve_chief_app_ui_call(attempt('c')).await.unwrap().is_some());
}

#[tokio::test]
async fn app_ui_results_preserve_large_content_and_exact_receipt_identity() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("app-ui-result.sqlite3");
	let store = setup(&path).await;
	let a = attempt('d');
	let id = store.reserve_chief_app_ui_call(a.clone()).await.unwrap().unwrap();
	let result = json!({"content":[{"type":"text","text":"y".repeat(70000)}],"structuredContent":{"value":7},"_meta":{"view":"retained"}});
	assert!(
		store
			.finish_chief_app_ui_call(
				id,
				a.attempt_id.clone(),
				"completed".into(),
				Some(result.clone())
			)
			.await
			.unwrap()
	);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let receipt = store
		.chief_app_ui_call_receipt(a.owner.work.clone(), a.attempt_id.clone())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(receipt.result, Some(result));
	assert_eq!(receipt.state, "completed");
	assert!(
		store
			.chief_app_ui_call_receipt(owner(2).work, a.attempt_id.clone())
			.await
			.unwrap()
			.is_none()
	);
	assert!(
		!store.acknowledge_chief_app_ui_uncertainty(a.owner.work, id, a.attempt_id).await.unwrap()
	);
	let mut replay = attempt('d');
	replay.attempt_id = "new-attempt-same-review".into();
	assert!(store.reserve_chief_app_ui_call(replay).await.unwrap().is_none());
	assert!(store.reserve_chief_app_ui_call(attempt('e')).await.unwrap().is_some());
}

#[tokio::test]
async fn unfinished_app_call_requires_positive_process_death_before_recovery() {
	let dir = tempfile::tempdir().unwrap();
	let store = setup(&dir.path().join("recovery.sqlite3")).await;
	let a = attempt('f');
	let id = store.reserve_chief_app_ui_call(a.clone()).await.unwrap().unwrap();
	assert!(
		!store.recover_chief_app_ui_call(a.owner.work.clone(), a.attempt_id.clone()).await.unwrap()
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
		!store.recover_chief_app_ui_call(a.owner.work.clone(), a.attempt_id.clone()).await.unwrap()
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
		store.recover_chief_app_ui_call(a.owner.work.clone(), a.attempt_id.clone()).await.unwrap()
	);
	assert!(
		!store.recover_chief_app_ui_call(a.owner.work.clone(), a.attempt_id.clone()).await.unwrap()
	);
	let receipt =
		store.chief_app_ui_call_receipt(a.owner.work, a.attempt_id).await.unwrap().unwrap();
	assert_eq!(receipt.id, id);
	assert_eq!(receipt.state, "unknown");
	assert!(receipt.result.is_none());
}
