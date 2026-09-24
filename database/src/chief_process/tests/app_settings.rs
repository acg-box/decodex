//! Request-scoped app edits share native file arbitration with hooks, without approving tools.
use super::{
	hooks::{identity, owner, setup},
	*,
};
use crate::{
	ChiefAppSettingsAttempt, ChiefAppSettingsObservation, ChiefConfigReceipt, EnqueueChiefEvent,
};
use serde_json::json;

async fn request(store: &SqliteStore, n: u8, child: bool) -> i64 {
	let o = owner(n);
	store.enqueue_chief_event(EnqueueChiefEvent {
        source_event_id:format!("app-request-{n}-{child}"),work_item_id:o.work,
        event_kind:"server_request_pending".into(),
        payload:json!({"id":format!("approval-{n}"),"ownerThreadId":o.thread,
            "method":"mcpServer/elicitation/request","params":{"threadId":if child {"native-child"} else {&o.thread},
            "turnId":"previous-originating-turn","serverName":"codex_apps",
            "_meta":{"connector_id":"calendar","link_id":" work.link ","source":"connector"},
            "tool_params":{"link_id":"untrusted-other-link"}}}).to_string()
    }).await.unwrap().id
}
fn attempt(n: u8, event: i64, token: char, previous: Option<i64>) -> ChiefAppSettingsAttempt {
	ChiefAppSettingsAttempt {
		owner: owner(n),
		request_event_id: event,
		scope: DIGEST.into(),
		connector: "calendar".into(),
		link: " work.link ".into(),
		field: "default_tools_approval_mode".into(),
		value: Some(json!("approve")),
		previous_value: Some(json!("prompt")),
		config_version: "before".into(),
		review_token: token.to_string().repeat(64),
		attempt_id: format!("attempt-{token}"),
		previous_id: previous,
	}
}
fn observation(n: u8, value: Option<&str>) -> ChiefAppSettingsObservation {
	ChiefAppSettingsObservation {
		owner: owner(n),
		scope: DIGEST.into(),
		connector: "calendar".into(),
		link: " work.link ".into(),
		field: "default_tools_approval_mode".into(),
		value: value.map(|v| json!(v)),
		config_version: "after".into(),
	}
}

#[tokio::test]
async fn app_review_is_exact_single_use_and_does_not_answer_pending_request() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("apps.sqlite3");
	let store = setup(&path).await;
	let event = request(&store, 1, false).await;
	let mut wrong = attempt(1, event, 'a', None);
	wrong.link = "untrusted-other-link".into();
	assert!(store.reserve_chief_app_settings_attempt(wrong).await.is_err());
	let mut wrong = attempt(1, event, 'a', None);
	wrong.owner.account = owner(2).account;
	assert!(store.reserve_chief_app_settings_attempt(wrong).await.is_err());
	let (a, b) = tokio::join!(
		store.reserve_chief_app_settings_attempt(attempt(1, event, 'a', None)),
		store.reserve_chief_app_settings_attempt(attempt(1, event, 'a', None))
	);
	assert_ne!(a.as_ref().unwrap().is_some(), b.as_ref().unwrap().is_some());
	let id = a.unwrap().or(b.unwrap()).unwrap();
	assert!(
		!store
			.finish_chief_app_settings_attempt(
				id,
				"wrong".into(),
				"saved".into(),
				Some("after".into())
			)
			.await
			.unwrap()
	);
	assert!(
		store
			.finish_chief_app_settings_attempt(id, "attempt-a".into(), "saved".into(), None)
			.await
			.is_err()
	);
	assert!(
		store
			.finish_chief_app_settings_attempt(
				id,
				"attempt-a".into(),
				"saved".into(),
				Some("after".into())
			)
			.await
			.unwrap()
	);
	assert!(
		!store
			.finish_chief_app_settings_attempt(id, "attempt-a".into(), "unknown".into(), None)
			.await
			.unwrap()
	);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	let receipt = store.chief_app_settings_receipt(DIGEST.into()).await.unwrap().unwrap();
	assert_eq!(receipt.state, "saved");
	assert_eq!(receipt.saved_version.as_deref(), Some("after"));
	assert!(store.get_chief_inbox_event(event).await.unwrap().disposition.is_none());
	let mut replay = attempt(1, event, 'a', Some(id));
	replay.field = "approvals_reviewer".into();
	replay.value = Some(json!("user"));
	replay.previous_value = None;
	assert!(
		store.reserve_chief_app_settings_attempt(replay).await.unwrap().is_none(),
		"changing field does not renew consent"
	);
	let child = request(&store, 1, true).await;
	store.begin_chief_dispatch("root".into()).await.unwrap();
	store.acknowledge_chief_dispatch("root".into(), "successor-turn".into()).await.unwrap();
	let next = store
		.reserve_chief_app_settings_attempt(attempt(1, child, 'b', Some(id)))
		.await
		.unwrap()
		.unwrap();
	store
		.finish_chief_app_settings_attempt(next, "attempt-b".into(), "rejected".into(), None)
		.await
		.unwrap();
	store.acknowledge_chief_request_event(child).await.unwrap();
	assert!(
		store.reserve_chief_app_settings_attempt(attempt(1, child, 'c', Some(next))).await.is_err()
	);
	let (visible, _) = store.read_chief_transcript("root".into(), None, 1).await.unwrap();
	assert_eq!(visible[0].id, child);
	let rows = store.read_chief_work_events("root".into(), 1).await.unwrap();
	assert_eq!(rows[0].id, child);
	assert!(store.list_chief_wake_events("root".into(), 100).await.unwrap().is_empty());
}

#[tokio::test]
async fn app_and_hook_writes_share_one_file_across_accounts() {
	let dir = tempfile::tempdir().unwrap();
	let store = setup(&dir.path().join("shared.sqlite3")).await;
	let event = request(&store, 2, false).await;
	let hook =
		store.reserve_chief_hook_setting(hooks::attempt(1, 'a', None)).await.unwrap().unwrap();
	assert!(matches!(
		store.chief_config_receipt(DIGEST.into()).await.unwrap(),
		Some(ChiefConfigReceipt::Hook(_))
	));
	assert!(
		store
			.reserve_chief_app_settings_attempt(attempt(2, event, 'b', None))
			.await
			.unwrap()
			.is_none()
	);
	let mut independent = attempt(2, event, 'c', None);
	independent.scope = OTHER_DIGEST.into();
	let independent = store.reserve_chief_app_settings_attempt(independent).await.unwrap().unwrap();
	store
		.finish_chief_app_settings_attempt(
			independent,
			"attempt-c".into(),
			"overridden".into(),
			Some("after".into()),
		)
		.await
		.unwrap();
	store.finish_chief_hook_setting(hook, "attempt-a".into(), "saved".into()).await.unwrap();
	assert!(
		store
			.reserve_chief_app_settings_attempt(attempt(2, event, 'a', None))
			.await
			.unwrap()
			.is_none(),
		"consumed review cannot cross setting kind"
	);
	let app = store
		.reserve_chief_app_settings_attempt(attempt(2, event, 'b', None))
		.await
		.unwrap()
		.unwrap();
	assert!(matches!(
		store.chief_config_receipt(DIGEST.into()).await.unwrap(),
		Some(ChiefConfigReceipt::App(_))
	));
	assert!(
		store
			.reserve_chief_hook_setting(hooks::attempt(1, 'd', Some(hook)))
			.await
			.unwrap()
			.is_none()
	);
	store
		.finish_chief_app_settings_attempt(app, "attempt-b".into(), "unknown".into(), None)
		.await
		.unwrap();
	assert!(
		store
			.reserve_chief_hook_setting(hooks::attempt(1, 'd', Some(hook)))
			.await
			.unwrap()
			.is_none()
	);
	assert!(!store.observe_chief_app_settings(app, observation(1, Some("approve"))).await.unwrap());
	let mut unchanged = observation(2, Some("approve"));
	unchanged.config_version = "before".into();
	assert!(!store.observe_chief_app_settings(app, unchanged).await.unwrap());
	assert!(!store.observe_chief_app_settings(app, observation(2, Some("prompt"))).await.unwrap());
	assert!(store.observe_chief_app_settings(app, observation(2, Some("approve"))).await.unwrap());
	assert!(
		store
			.reserve_chief_hook_setting(hooks::attempt(1, 'd', Some(hook)))
			.await
			.unwrap()
			.is_some()
	);
}

#[tokio::test]
async fn app_removal_recovery_requires_exact_target_or_dead_writer() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("recovery.sqlite3");
	let store = setup(&path).await;
	let event = request(&store, 1, false).await;
	let mut removal = attempt(1, event, 'a', None);
	removal.value = None;
	removal.previous_value = Some(json!("future-mode"));
	let id = store.reserve_chief_app_settings_attempt(removal.clone()).await.unwrap().unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_eq!(
		store.chief_app_settings_receipt(DIGEST.into()).await.unwrap().unwrap().state,
		"reserved"
	);
	removal.previous_id = Some(id);
	assert!(store.reserve_chief_app_settings_attempt(removal).await.unwrap().is_none());
	let mut wrong = observation(1, None);
	wrong.link = "personal".into();
	assert!(!store.observe_chief_app_settings(id, wrong).await.unwrap());
	assert!(store.observe_chief_app_settings(id, observation(1, None)).await.unwrap());
	let next = store
		.reserve_chief_app_settings_attempt(attempt(1, event, 'b', Some(id)))
		.await
		.unwrap()
		.unwrap();
	store
		.mark_process_generation_death_unknown(
			&generation_id(1),
			3,
			decodex_core::ProcessAuthorityLossReason::SupervisorRestarted,
		)
		.await
		.unwrap();
	assert!(
		!store.observe_chief_app_settings(next, observation(2, Some("future-mode"))).await.unwrap()
	);
	let evidence = ProcessDeathEvidence::new(
		ProcessDeathEvidenceId::new("50000000-0000-4000-8000-000000000001").unwrap(),
		generation_id(1),
		ProcessDeathEvidenceKind::OwnedChildExit,
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		Some(identity(123)),
		DIGEST,
	)
	.unwrap();
	store.record_process_generation_death(4, &evidence).await.unwrap();
	let mut wrong = observation(2, Some("future-mode"));
	wrong.scope = OTHER_DIGEST.into();
	assert!(!store.observe_chief_app_settings(next, wrong).await.unwrap());
	assert!(
		store.observe_chief_app_settings(next, observation(2, Some("future-mode"))).await.unwrap()
	);
	assert_eq!(
		store.chief_app_settings_receipt(DIGEST.into()).await.unwrap().unwrap().state,
		"superseded"
	);
	assert!(
		!store
			.finish_chief_app_settings_attempt(
				next,
				"attempt-b".into(),
				"saved".into(),
				Some("late".into())
			)
			.await
			.unwrap()
	);
	let second = request(&store, 2, false).await;
	assert!(
		store
			.reserve_chief_app_settings_attempt(attempt(2, second, 'c', Some(next)))
			.await
			.unwrap()
			.is_some()
	);
}

#[tokio::test]
async fn concurrent_app_and_hook_reservations_have_only_one_winner() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("concurrent.sqlite3");
	let store = setup(&path).await;
	let other = SqliteStore::open_test(&path).unwrap();
	let event = request(&store, 2, false).await;
	let (hook, app) = tokio::join!(
		store.reserve_chief_hook_setting(hooks::attempt(1, 'a', None)),
		other.reserve_chief_app_settings_attempt(attempt(2, event, 'b', None))
	);
	assert_ne!(hook.unwrap().is_some(), app.unwrap().is_some());
	assert_eq!(
		store.chief_config_receipt(DIGEST.into()).await.unwrap(),
		other.chief_config_receipt(DIGEST.into()).await.unwrap()
	);
}
