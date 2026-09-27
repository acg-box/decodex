//! Automatic fallback shares the model journal without weakening explicit selection.
use super::*;
use serde_json::{Value, json};

fn identity() -> decodex_core::ProcessIdentity {
	decodex_core::ProcessIdentity::new(
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		123,
		decodex_core::ProcessStartIdentity::new("fixture-123").unwrap(),
		123,
		123,
	)
	.unwrap()
}

async fn publish(store: &SqliteStore, model: &str, tier: Value, digest: &str) -> i64 {
	store
		.record_chief_task_models_publication(
			"thread".into(),
			Some(generation_id(1).as_str().into()),
			Some(
				json!({"model":model,"modelProvider":"fixture","effort":"high","serviceTier":tier})
					.to_string(),
			),
			digest.into(),
		)
		.await
		.unwrap()
		.unwrap()
}

async fn ready(store: &SqliteStore) -> crate::ChiefModelAttempt {
	seed(store).await;
	store.bind_chief_thread("root".into(), "thread".into()).await.unwrap();
	store
		.prepare_chief_bound_process_generation(&intent(1, 1), &binding(1), "root", "first")
		.await
		.unwrap();
	store.bind_process_generation_identity(&generation_id(1), 1, &identity()).await.unwrap();
	store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
	let event = publish(store, "blocked", json!("default"), DIGEST).await;
	crate::ChiefModelAttempt {
		work: "root".into(),
		thread: "thread".into(),
		generation: Some(generation_id(1).as_str().into()),
		settings_event: event,
		model: "fallback".into(),
		model_provider: "fixture".into(),
		effort: Some("high".into()),
		review_token: DIGEST.into(),
		attempt_id: "first".into(),
		recovery: Some(crate::ChiefModelRecoveryContext {
			account: account_id(1).as_str().into(),
			account_revision: 1,
			banner_digest: DIGEST.into(),
			from_model: "blocked".into(),
			service_tier: Some("priority".into()),
		}),
	}
}

async fn receipt(store: &SqliteStore) -> crate::ChiefModelReceipt {
	store.chief_model_receipt("root".into(), "thread".into()).await.unwrap().unwrap()
}

#[tokio::test]
async fn automatic_model_reservation_binds_account_idle_and_explicit_input() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("automatic.sqlite3")).unwrap();
	let attempt = ready(&store).await;
	for change in ["account", "revision", "model"] {
		let mut stale = attempt.clone();
		let recovery = stale.recovery.as_mut().unwrap();
		match change {
			"account" => recovery.account = account_id(2).as_str().into(),
			"revision" => recovery.account_revision = 2,
			_ => recovery.from_model = "different".into(),
		}
		assert!(store.reserve_chief_model_selection(stale).await.unwrap().is_none());
	}
	let mut reserve = attempt.clone();
	reserve.model = "gpt-reserve".into();
	assert!(store.reserve_chief_model_selection(reserve).await.is_err());
	let mut incomplete = attempt.clone();
	incomplete.effort = None;
	assert!(store.reserve_chief_model_selection(incomplete).await.is_err());
	store.begin_chief_dispatch("root".into()).await.unwrap();
	assert!(store.reserve_chief_model_selection(attempt.clone()).await.unwrap().is_none());
	store.acknowledge_chief_dispatch("root".into(), "turn".into()).await.unwrap();
	assert!(store.reserve_chief_model_selection(attempt.clone()).await.unwrap().is_none());
	store.complete_chief_turn("root".into(), "turn".into()).await.unwrap();
	store
		.enqueue_chief_event(crate::EnqueueChiefEvent {
			source_event_id: "explicit-input".into(),
			work_item_id: "root".into(),
			event_kind: "user_message".into(),
			payload: json!({"text":"next","options":{"execution":{"reasoning_effort":"low"}}})
				.to_string(),
		})
		.await
		.unwrap();
	assert!(store.reserve_chief_model_selection(attempt).await.unwrap().is_none());
	assert!(store.chief_model_receipt("root".into(), "thread".into()).await.unwrap().is_none());
}

#[tokio::test]
async fn automatic_model_receipts_require_tier_and_never_replay_after_reopen() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("automatic.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	let attempt = ready(&store).await;
	let id = store.reserve_chief_model_selection(attempt.clone()).await.unwrap().unwrap();
	store.finish_chief_model_selection(id, attempt.clone(), "unknown".into()).await.unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert!(store.begin_chief_dispatch("root".into()).await.is_err());
	assert!(store.reserve_chief_model_selection(attempt.clone()).await.unwrap().is_none());
	publish(&store, "fallback", json!("default"), OTHER_DIGEST).await;
	assert_eq!(receipt(&store).await.state, "unknown");
	store
		.record_chief_task_models_publication(
			"thread".into(),
			Some(generation_id(1).as_str().into()),
			Some(json!({"model":"fallback","modelProvider":"fixture","effort":"high"}).to_string()),
			DIGEST.into(),
		)
		.await
		.unwrap();
	assert_eq!(receipt(&store).await.state, "unknown", "missing tier is not an observed absence");
	publish(&store, "fallback", json!("priority"), OTHER_DIGEST).await;
	assert_eq!(receipt(&store).await.state, "target_observed");
	let historical = store
		.chief_model_history("root".into(), "thread".into(), generation_id(1).as_str().into())
		.await
		.expect("automatic history")
		.expect("automatic receipt");
	assert_eq!(
		historical.response, "unknown",
		"native confirmation cannot rewrite the RPC outcome"
	);
	assert!(!historical.manual && historical.target_observed && !historical.reconciled);
	store
		.run(|connection| {
			connection
				.execute(
					"UPDATE accounts SET revision=2 WHERE account_id=?1",
					[account_id(1).as_str()],
				)
				.map_err(crate::error::sqlite_error)?;
			Ok(())
		})
		.await
		.unwrap();
	let mut replay = attempt;
	replay.settings_event = publish(&store, "blocked", json!("default"), DIGEST).await;
	replay.review_token = OTHER_DIGEST.into();
	replay.attempt_id = "fresh-request".into();
	replay.recovery.as_mut().unwrap().account_revision = 2;
	assert!(
		store.reserve_chief_model_selection(replay.clone()).await.unwrap().is_none(),
		"refresh and new review cannot replay one banner"
	);
	replay.recovery.as_mut().unwrap().banner_digest = OTHER_DIGEST.into();
	assert!(
		store.reserve_chief_model_selection(replay).await.unwrap().is_some(),
		"a genuinely new banner can reserve independently"
	);
}

#[tokio::test]
async fn automatic_model_confirmation_refuses_changed_account_revision() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("automatic.sqlite3")).unwrap();
	let attempt = ready(&store).await;
	let id = store.reserve_chief_model_selection(attempt.clone()).await.unwrap().unwrap();
	store.finish_chief_model_selection(id, attempt, "queued".into()).await.unwrap();
	store
		.run(|connection| {
			connection
				.execute(
					"UPDATE accounts SET revision=2 WHERE account_id=?1",
					[account_id(1).as_str()],
				)
				.map_err(crate::error::sqlite_error)?;
			Ok(())
		})
		.await
		.unwrap();
	publish(&store, "fallback", json!("priority"), OTHER_DIGEST).await;
	assert_eq!(receipt(&store).await.state, "queued");
	assert!(store.begin_chief_dispatch("root".into()).await.is_err());
}

#[tokio::test]
async fn automatic_model_new_owner_supersedes_unknown_delivery_without_claiming_success() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("automatic.sqlite3")).unwrap();
	let attempt = ready(&store).await;
	let id = store.reserve_chief_model_selection(attempt.clone()).await.unwrap().unwrap();
	store.finish_chief_model_selection(id, attempt, "unknown".into()).await.unwrap();
	store
		.mark_process_generation_death_unknown(
			&generation_id(1),
			3,
			decodex_core::ProcessAuthorityLossReason::SupervisorRestarted,
		)
		.await
		.unwrap();
	let evidence = ProcessDeathEvidence::new(
		ProcessDeathEvidenceId::new("50000000-0000-4000-8000-000000000001").unwrap(),
		generation_id(1),
		ProcessDeathEvidenceKind::OwnedChildExit,
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		Some(identity()),
		DIGEST,
	)
	.unwrap();
	store.record_process_generation_death(4, &evidence).await.unwrap();
	assert!(matches!(
		store
			.prepare_chief_bound_process_generation(&intent(1, 2), &binding(1), "root", "new")
			.await
			.unwrap(),
		PrepareProcessGenerationOutcome::Fresh(_)
	));
	let next = decodex_core::ProcessIdentity::new(
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		124,
		decodex_core::ProcessStartIdentity::new("fixture-124").unwrap(),
		124,
		124,
	)
	.unwrap();
	store.bind_process_generation_identity(&generation_id(2), 1, &next).await.unwrap();
	store.mark_process_generation_ready(&generation_id(2), 2).await.unwrap();
	store.record_chief_task_models_publication(
		"thread".into(), Some(generation_id(2).as_str().into()),
		Some(json!({"model":"fallback","modelProvider":"fixture","effort":"high","serviceTier":"priority"}).to_string()),
		OTHER_DIGEST.into(),
	).await.unwrap().unwrap();
	assert_eq!(receipt(&store).await.state, "superseded");
	let historical = store
		.chief_model_history("root".into(), "thread".into(), generation_id(2).as_str().into())
		.await
		.expect("reconciled history")
		.expect("automatic receipt");
	assert_eq!(historical.response, "unknown");
	assert!(!historical.manual && !historical.target_observed && historical.reconciled);
	assert!(store.begin_chief_dispatch("root".into()).await.is_ok());
}
