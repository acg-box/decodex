//! Shared config reservations cross task/account boundaries and never wake model work.
use super::*;
use crate::{ChiefHookAttempt, ChiefHookObservation, ChiefHookOwner};
use serde_json::json;
fn identity(n: u32) -> decodex_core::ProcessIdentity {
	decodex_core::ProcessIdentity::new(
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		n,
		decodex_core::ProcessStartIdentity::new(format!("fixture-{n}")).unwrap(),
		n,
		n,
	)
	.unwrap()
}
fn owner(n: u8) -> ChiefHookOwner {
	ChiefHookOwner {
		work: if n == 1 { "root" } else { "second-root" }.into(),
		thread: format!("thread-{n}"),
		generation: generation_id(n).as_str().into(),
		account: account_id(n).as_str().into(),
	}
}
fn attempt(n: u8, token: char, previous_id: Option<i64>) -> ChiefHookAttempt {
	ChiefHookAttempt {
		owner: owner(n),
		scope: DIGEST.into(),
		hook: "plugin-hook".into(),
		field: "enabled".into(),
		value: json!(false),
		previous_value: Some(json!(true)),
		config_version: "before".into(),
		review_token: token.to_string().repeat(64),
		attempt_id: format!("attempt-{token}"),
		previous_id,
	}
}
fn observation(n: u8, value: Option<bool>) -> ChiefHookObservation {
	ChiefHookObservation {
		owner: owner(n),
		scope: DIGEST.into(),
		hook: "plugin-hook".into(),
		field: "enabled".into(),
		value: value.map(|v| json!(v)),
		config_version: "after".into(),
	}
}
async fn setup(path: &std::path::Path) -> SqliteStore {
	let store = SqliteStore::open_test(path).unwrap();
	seed(&store).await;
	for n in [1, 2] {
		let owner = owner(n);
		store.bind_chief_thread(owner.work.clone(), owner.thread).await.unwrap();
		store
			.prepare_chief_bound_process_generation(
				&intent(n, n),
				&binding(n),
				&owner.work,
				&format!("owner-{n}"),
			)
			.await
			.unwrap();
		store
			.bind_process_generation_identity(&generation_id(n), 1, &identity(122 + u32::from(n)))
			.await
			.unwrap();
		store.mark_process_generation_ready(&generation_id(n), 2).await.unwrap();
	}
	store
}
#[tokio::test]
async fn hook_receipts_serialize_shared_config_and_survive_restart_without_replay() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("hooks.sqlite3");
	let store = setup(&path).await;
	let visible = store
		.record_chief_observation(crate::EnqueueChiefEvent {
			source_event_id: "visible".into(),
			work_item_id: "root".into(),
			event_kind: "assistant_message".into(),
			payload: json!({"item":{"text":"visible"}}).to_string(),
		})
		.await
		.unwrap();
	let mut noop = attempt(1, 'f', None);
	noop.previous_value = Some(json!(false));
	assert!(store.reserve_chief_hook_setting(noop).await.is_err());
	let (a, b) = tokio::join!(
		store.reserve_chief_hook_setting(attempt(1, 'a', None)),
		store.reserve_chief_hook_setting(attempt(1, 'b', None))
	);
	assert_ne!(a.as_ref().unwrap().is_some(), b.as_ref().unwrap().is_some());
	let id = a.unwrap().or(b.unwrap()).unwrap();
	let mut independent = attempt(2, 'e', None);
	independent.scope = OTHER_DIGEST.into();
	let other = store.reserve_chief_hook_setting(independent).await.unwrap().unwrap();
	assert!(
		store.finish_chief_hook_setting(other, "attempt-e".into(), "saved".into()).await.unwrap()
	);
	let receipt = store.chief_hook_receipt(DIGEST.into()).await.unwrap().unwrap();
	assert!(
		store.reserve_chief_hook_setting(attempt(2, 'c', Some(id))).await.unwrap().is_none(),
		"other account shares config reservation"
	);
	assert!(!store.finish_chief_hook_setting(id, "wrong".into(), "saved".into()).await.unwrap());
	assert!(
		store
			.finish_chief_hook_setting(id, receipt.attempt.attempt_id.clone(), "unknown".into())
			.await
			.unwrap()
	);
	assert!(
		!store
			.finish_chief_hook_setting(id, receipt.attempt.attempt_id.clone(), "saved".into())
			.await
			.unwrap()
	);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_eq!(store.chief_hook_receipt(DIGEST.into()).await.unwrap().unwrap().state, "unknown");
	assert!(
		!store.observe_chief_hook_setting(id, observation(2, Some(false))).await.unwrap(),
		"another live owner cannot settle the first owner's uncertain write"
	);
	let mut unchanged = observation(1, Some(false));
	unchanged.config_version = "before".into();
	assert!(!store.observe_chief_hook_setting(id, unchanged).await.unwrap());
	assert!(!store.observe_chief_hook_setting(id, observation(1, None)).await.unwrap());
	assert!(store.observe_chief_hook_setting(id, observation(1, Some(false))).await.unwrap());
	assert_eq!(
		store.chief_hook_receipt(DIGEST.into()).await.unwrap().unwrap().state,
		"target_observed"
	);
	let mut replay = receipt.attempt.clone();
	replay.owner = owner(2);
	replay.previous_id = Some(id);
	replay.attempt_id = "another-client".into();
	assert!(
		store.reserve_chief_hook_setting(replay).await.unwrap().is_none(),
		"same review cannot be replayed through another task"
	);
	let mut wrong = attempt(2, 'd', Some(id));
	wrong.owner.account = owner(1).account;
	assert!(store.reserve_chief_hook_setting(wrong).await.is_err());
	let next = store.reserve_chief_hook_setting(attempt(2, 'd', Some(id))).await.unwrap().unwrap();
	assert!(
		store
			.finish_chief_hook_setting(next, "attempt-d".into(), "overridden".into())
			.await
			.unwrap()
	);
	assert_eq!(store.chief_hook_receipt(DIGEST.into()).await.unwrap().unwrap().state, "overridden");
	let (rows, _) = store.read_chief_transcript("root".into(), None, 1).await.unwrap();
	assert_eq!(rows.len(), 1);
	assert_eq!(rows[0].id, visible.id);
	assert!(store.list_pending_chief_events(100).await.unwrap().is_empty());
	assert!(store.list_chief_wake_events("root".into(), 100).await.unwrap().is_empty());
}
#[tokio::test]
async fn hook_recovery_requires_dead_writer_and_exact_config_scope() {
	let dir = tempfile::tempdir().unwrap();
	let store = setup(&dir.path().join("hooks.sqlite3")).await;
	let id = store.reserve_chief_hook_setting(attempt(1, 'a', None)).await.unwrap().unwrap();
	store
		.mark_process_generation_death_unknown(
			&generation_id(1),
			3,
			decodex_core::ProcessAuthorityLossReason::SupervisorRestarted,
		)
		.await
		.unwrap();
	assert!(!store.observe_chief_hook_setting(id, observation(2, None)).await.unwrap());
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
	let mut other = observation(2, None);
	other.scope = OTHER_DIGEST.into();
	assert!(!store.observe_chief_hook_setting(id, other).await.unwrap());
	assert!(store.observe_chief_hook_setting(id, observation(2, None)).await.unwrap());
	assert_eq!(store.chief_hook_receipt(DIGEST.into()).await.unwrap().unwrap().state, "superseded");
	assert!(
		!store.finish_chief_hook_setting(id, "attempt-a".into(), "saved".into()).await.unwrap()
	);
	assert!(store.reserve_chief_hook_setting(attempt(2, 'b', Some(id))).await.unwrap().is_some());
}
