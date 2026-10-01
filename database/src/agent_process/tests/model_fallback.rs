//! Automatic fallback shares the model journal without weakening explicit selection.
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	AgentManualModelSource, AgentModelAttempt, AgentModelReceipt, AgentModelRecoveryContext,
	AgentPermissionAttempt, EnqueueAgentEvent, PrepareProcessGenerationOutcome, SqliteStore,
	agent_process::tests::{self, DIGEST, OTHER_DIGEST},
	error,
};
use decodex_core::{
	ProcessAuthorityLossReason, ProcessBootIdentity, ProcessDeathEvidence, ProcessDeathEvidenceId,
	ProcessDeathEvidenceKind, ProcessIdentity, ProcessStartIdentity,
};

fn identity() -> ProcessIdentity {
	ProcessIdentity::new(
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		123,
		ProcessStartIdentity::new("fixture-123").unwrap(),
		123,
		123,
	)
	.unwrap()
}

async fn publish(store: &SqliteStore, model: &str, tier: Value, digest: &str) -> i64 {
	store
		.record_agent_task_models_publication(
			"thread".into(),
			Some(tests::generation_id(1).as_str().into()),
			Some(
				serde_json::json!({"model":model,"modelProvider":"fixture","effort":"high","serviceTier":tier})
					.to_string(),
			),
			digest.into(),
		)
		.await
		.unwrap()
		.unwrap()
}

async fn ready(store: &SqliteStore) -> AgentModelAttempt {
	tests::seed(store).await;

	store.bind_agent_thread("root".into(), "thread".into()).await.unwrap();
	store
		.prepare_agent_bound_process_generation(
			&tests::intent(1, 1),
			&tests::binding(1),
			"root",
			"first",
		)
		.await
		.unwrap();
	store.bind_process_generation_identity(&tests::generation_id(1), 1, &identity()).await.unwrap();
	store.mark_process_generation_ready(&tests::generation_id(1), 2).await.unwrap();

	let event = publish(store, "blocked", serde_json::json!("default"), DIGEST).await;

	AgentModelAttempt {
		work: "root".into(),
		thread: "thread".into(),
		generation: Some(tests::generation_id(1).as_str().into()),
		settings_event: event,
		model: "fallback".into(),
		model_provider: "fixture".into(),
		effort: Some("high".into()),
		review_token: DIGEST.into(),
		attempt_id: "first".into(),
		manual_source: None,
		recovery: Some(AgentModelRecoveryContext {
			account: tests::account_id(1).as_str().into(),
			account_revision: 1,
			banner_digest: DIGEST.into(),
			from_model: "blocked".into(),
			service_tier: Some("priority".into()),
		}),
	}
}

async fn receipt(store: &SqliteStore) -> AgentModelReceipt {
	store.agent_model_receipt("root".into(), "thread".into()).await.unwrap().unwrap()
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
			"account" => recovery.account = tests::account_id(2).as_str().into(),
			"revision" => recovery.account_revision = 2,
			_ => recovery.from_model = "different".into(),
		}

		assert!(store.reserve_agent_model_selection(stale).await.unwrap().is_none());
	}

	let mut reserve = attempt.clone();

	reserve.model = "gpt-reserve".into();

	assert!(store.reserve_agent_model_selection(reserve).await.is_err());

	let mut incomplete = attempt.clone();

	incomplete.effort = None;

	assert!(store.reserve_agent_model_selection(incomplete).await.is_err());

	store.begin_agent_dispatch("root".into()).await.unwrap();

	assert!(store.reserve_agent_model_selection(attempt.clone()).await.unwrap().is_none());

	store.acknowledge_agent_dispatch("root".into(), "turn".into()).await.unwrap();

	assert!(store.reserve_agent_model_selection(attempt.clone()).await.unwrap().is_none());

	store.complete_agent_turn("root".into(), "turn".into()).await.unwrap();
	store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "explicit-input".into(),
			work_item_id: "root".into(),
			event_kind: "user_message".into(),
			payload: serde_json::json!({"text":"next","options":{"execution":{"reasoning_effort":"low"}}})
				.to_string(),
		})
		.await
		.unwrap();

	assert!(store.reserve_agent_model_selection(attempt).await.unwrap().is_none());
	assert!(store.agent_model_receipt("root".into(), "thread".into()).await.unwrap().is_none());
}

#[tokio::test]
async fn automatic_model_receipts_require_tier_and_never_replay_after_reopen() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("automatic.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	let attempt = ready(&store).await;
	let id = store.reserve_agent_model_selection(attempt.clone()).await.unwrap().unwrap();

	store.finish_agent_model_selection(id, attempt.clone(), "unknown".into()).await.unwrap();

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();

	assert!(store.begin_agent_dispatch("root".into()).await.is_err());
	assert!(store.reserve_agent_model_selection(attempt.clone()).await.unwrap().is_none());

	publish(&store, "fallback", serde_json::json!("default"), OTHER_DIGEST).await;

	assert_eq!(receipt(&store).await.state, "unknown");

	store
		.record_agent_task_models_publication(
			"thread".into(),
			Some(tests::generation_id(1).as_str().into()),
			Some(
				serde_json::json!({"model":"fallback","modelProvider":"fixture","effort":"high"})
					.to_string(),
			),
			DIGEST.into(),
		)
		.await
		.unwrap();

	assert_eq!(receipt(&store).await.state, "unknown", "missing tier is not an observed absence");

	publish(&store, "fallback", serde_json::json!("priority"), OTHER_DIGEST).await;

	assert_eq!(receipt(&store).await.state, "target_observed");

	let historical = store
		.agent_model_history(
			"root".into(),
			"thread".into(),
			tests::generation_id(1).as_str().into(),
		)
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
					[tests::account_id(1).as_str()],
				)
				.map_err(error::sqlite_error)?;

			Ok(())
		})
		.await
		.unwrap();

	let mut replay = attempt;

	replay.settings_event = publish(&store, "blocked", serde_json::json!("default"), DIGEST).await;
	replay.review_token = OTHER_DIGEST.into();
	replay.attempt_id = "fresh-request".into();
	replay.recovery.as_mut().unwrap().account_revision = 2;

	assert!(
		store.reserve_agent_model_selection(replay.clone()).await.unwrap().is_none(),
		"refresh and new review cannot replay one banner"
	);

	replay.recovery.as_mut().unwrap().banner_digest = OTHER_DIGEST.into();

	assert!(
		store.reserve_agent_model_selection(replay).await.unwrap().is_some(),
		"a genuinely new banner can reserve independently"
	);
}

#[tokio::test]
async fn automatic_model_confirmation_refuses_changed_account_revision() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("automatic.sqlite3")).unwrap();
	let attempt = ready(&store).await;
	let id = store.reserve_agent_model_selection(attempt.clone()).await.unwrap().unwrap();

	store.finish_agent_model_selection(id, attempt, "queued".into()).await.unwrap();
	store
		.run(|connection| {
			connection
				.execute(
					"UPDATE accounts SET revision=2 WHERE account_id=?1",
					[tests::account_id(1).as_str()],
				)
				.map_err(error::sqlite_error)?;

			Ok(())
		})
		.await
		.unwrap();

	publish(&store, "fallback", serde_json::json!("priority"), OTHER_DIGEST).await;

	assert_eq!(receipt(&store).await.state, "queued");
	assert!(store.begin_agent_dispatch("root".into()).await.is_err());
}

#[tokio::test]
async fn automatic_model_new_owner_supersedes_unknown_delivery_without_claiming_success() {
	for manual in [false, true] {
		for model in ["blocked", "fallback"] {
			qualify_new_owner(manual, model).await;
		}
	}
}

async fn qualify_new_owner(manual: bool, observed_model: &str) {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("automatic.sqlite3")).unwrap();
	let mut attempt = ready(&store).await;

	if manual {
		attempt.recovery = None;
		attempt.manual_source = Some(AgentManualModelSource {
			account: tests::account_id(1).as_str().into(),
			account_revision: 1,
		});
	}

	let id = store.reserve_agent_model_selection(attempt.clone()).await.unwrap().unwrap();

	store.finish_agent_model_selection(id, attempt, "unknown".into()).await.unwrap();
	store
		.mark_process_generation_death_unknown(
			&tests::generation_id(1),
			3,
			ProcessAuthorityLossReason::SupervisorRestarted,
		)
		.await
		.unwrap();

	let evidence = ProcessDeathEvidence::new(
		ProcessDeathEvidenceId::new("50000000-0000-4000-8000-000000000001").unwrap(),
		tests::generation_id(1),
		ProcessDeathEvidenceKind::OwnedChildExit,
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		Some(identity()),
		DIGEST,
	)
	.unwrap();

	store.record_process_generation_death(4, &evidence).await.unwrap();

	assert!(matches!(
		store
			.prepare_agent_bound_process_generation(
				&tests::intent(1, 2),
				&tests::binding(1),
				"root",
				"new"
			)
			.await
			.unwrap(),
		PrepareProcessGenerationOutcome::Fresh(_)
	));

	let next = ProcessIdentity::new(
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		124,
		ProcessStartIdentity::new("fixture-124").unwrap(),
		124,
		124,
	)
	.unwrap();

	store.bind_process_generation_identity(&tests::generation_id(2), 1, &next).await.unwrap();
	store.mark_process_generation_ready(&tests::generation_id(2), 2).await.unwrap();

	for enabled in [false, true] {
		store
			.run(move |connection| {
				connection
					.execute(
						"UPDATE accounts SET enabled=?1 WHERE account_id=?2",
						rusqlite::params![enabled, tests::account_id(1).as_str()],
					)
					.map_err(error::sqlite_error)?;

				Ok(())
			})
			.await
			.unwrap();
		store.record_agent_task_models_publication(
			"thread".into(), Some(tests::generation_id(2).as_str().into()),
			Some(serde_json::json!({"model":observed_model,"modelProvider":"fixture","effort":"high","serviceTier":"priority"}).to_string()),
			OTHER_DIGEST.into(),
		).await.unwrap().unwrap();

		if !enabled {
			assert_eq!(
				receipt(&store).await.state,
				"unknown",
				"a disabled account cannot reconcile old delivery"
			);
			assert!(store.begin_agent_dispatch("root".into()).await.is_err());
		}
	}

	assert_eq!(receipt(&store).await.state, "superseded");

	let historical = store
		.agent_model_history(
			"root".into(),
			"thread".into(),
			tests::generation_id(2).as_str().into(),
		)
		.await
		.expect("reconciled history")
		.expect("automatic receipt");

	assert_eq!(historical.response, "unknown");
	assert_eq!(historical.manual, manual);
	assert!(!historical.target_observed && historical.reconciled);

	assert_permissions_after_reconciliation(&store).await;

	assert!(store.begin_agent_dispatch("root".into()).await.is_ok());
}

async fn assert_permissions_after_reconciliation(store: &SqliteStore) {
	let generation = Some(tests::generation_id(2).as_str().to_owned());
	let observed = store.record_agent_task_permissions_publication(
        "thread".into(), generation.clone(),
        Some(serde_json::json!({"profileId":":read-only","cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"}}).to_string()),
        OTHER_DIGEST.into()).await.unwrap().unwrap();
	let permission = AgentPermissionAttempt {
		work: "root".into(),
		thread: "thread".into(),
		generation,
		settings_event: observed,
		profile: ":workspace".into(),
		review_token: OTHER_DIGEST.into(),
		attempt_id: "after-reconciliation".into(),
	};
	let id = store
		.reserve_agent_permission_selection(permission.clone())
		.await
		.unwrap()
		.expect("reconciled model request must not block permission changes");

	assert!(
		store.finish_agent_permission_selection(id, permission, "rejected".into()).await.unwrap()
	);
}

#[tokio::test]
async fn manual_model_selection_respects_queued_execution_choices() {
	for running in [false, true] {
		for execution in [
			serde_json::json!({"model":"queued-model"}),
			serde_json::json!({"reasoning_effort":"high"}),
			serde_json::json!({}),
		] {
			let dir = tempfile::tempdir().unwrap();
			let store = SqliteStore::open_test(&dir.path().join("manual-queue.sqlite3")).unwrap();
			let mut attempt = ready(&store).await;

			attempt.recovery = None;
			attempt.model = "blocked".into();
			attempt.effort = Some("low".into());

			if running {
				store.begin_agent_dispatch("root".into()).await.unwrap();
				store
					.acknowledge_agent_dispatch("root".into(), "active-turn".into())
					.await
					.unwrap();
			}

			store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: "queued-input".into(),
					work_item_id: "root".into(),
					event_kind: "user_message".into(),
					payload:
						serde_json::json!({"text":"Next input", "options":{"execution":execution}})
							.to_string(),
				})
				.await
				.unwrap();

			let reserved = store.reserve_agent_model_selection(attempt).await.unwrap();

			assert_eq!(
				reserved.is_some(),
				execution == serde_json::json!({}),
				"running={running}, execution={execution}"
			);

			let pending = store.list_pending_agent_events(100).await.unwrap();

			assert_eq!(pending.len(), 1);
			assert_eq!(
				pending[0].source_event_id, "queued-input",
				"the user's input remains queued"
			);
		}
	}
}

#[tokio::test]
async fn manual_model_confirmation_keeps_the_reviewed_account_revision() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("manual-source.sqlite3")).unwrap();
	let mut attempt = ready(&store).await;

	attempt.recovery = None;
	attempt.manual_source = Some(AgentManualModelSource {
		account: tests::account_id(1).as_str().into(),
		account_revision: 1,
	});

	let id = store.reserve_agent_model_selection(attempt.clone()).await.unwrap().unwrap();

	store.finish_agent_model_selection(id, attempt, "queued".into()).await.unwrap();
	store
		.run(|connection| {
			connection
				.execute(
					"UPDATE accounts SET revision=revision+1 WHERE account_id=?1",
					[tests::account_id(1).as_str()],
				)
				.map_err(error::sqlite_error)?;

			Ok(())
		})
		.await
		.unwrap();

	publish(&store, "fallback", serde_json::json!("priority"), OTHER_DIGEST).await;

	assert_eq!(
		receipt(&store).await.state,
		"queued",
		"a later account revision cannot confirm the old request"
	);
	assert!(store.begin_agent_dispatch("root".into()).await.is_err());
}

#[tokio::test]
async fn manual_model_reservation_rechecks_its_account_and_retains_old_payload_shape() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("manual-reserve.sqlite3")).unwrap();
	let mut attempt = ready(&store).await;

	attempt.recovery = None;

	let old = serde_json::to_value(&attempt).unwrap();

	assert!(old.get("manual_source").is_none());
	assert_eq!(serde_json::from_value::<AgentModelAttempt>(old).unwrap(), attempt);

	for (account, revision) in [
		(tests::account_id(2).as_str().to_owned(), 1),
		(tests::account_id(1).as_str().to_owned(), 2),
	] {
		attempt.manual_source =
			Some(AgentManualModelSource { account, account_revision: revision });

		assert!(store.reserve_agent_model_selection(attempt.clone()).await.unwrap().is_none());
	}

	attempt.manual_source = Some(AgentManualModelSource {
		account: tests::account_id(1).as_str().into(),
		account_revision: 1,
	});

	let id = store.reserve_agent_model_selection(attempt.clone()).await.unwrap().unwrap();

	store.finish_agent_model_selection(id, attempt.clone(), "unknown".into()).await.unwrap();

	publish(&store, "fallback", serde_json::json!("priority"), OTHER_DIGEST).await;

	let receipt = receipt(&store).await;

	assert_eq!(receipt.state, "target_observed");
	assert_eq!(receipt.attempt.manual_source, attempt.manual_source);
}

#[tokio::test]
async fn ordinary_model_selection_rejects_reserve() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("ordinary-model.sqlite3")).unwrap();
	let mut attempt = ready(&store).await;

	attempt.recovery = None;
	attempt.model = "gpt-reserve".into();

	assert!(store.reserve_agent_model_selection(attempt).await.is_err());
	assert!(store.agent_model_receipt("root".into(), "thread".into()).await.unwrap().is_none());
}

#[tokio::test]
async fn completed_legacy_fallback_cannot_replay_in_the_current_journal() {
	for terminal in ["rejected", "observed", "reconciled"] {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("legacy-identity.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		let attempt = ready(&store).await;
		let recovery = attempt.recovery.as_ref().unwrap();
		let identity = serde_json::json!([
			attempt.work,
			attempt.thread,
			recovery.account,
			recovery.banner_digest,
			recovery.from_model,
			attempt.model,
			attempt.effort,
			recovery.service_tier
		]);
		let digest: String = Sha256::digest(identity.to_string().as_bytes())
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect();
		let key = format!("model-recovery:{digest}");
		let original = serde_json::json!({"attempt":{
            "work":attempt.work,"thread":attempt.thread,"generation":attempt.generation,
            "account":recovery.account,"account_revision":recovery.account_revision,
            "settings_event":attempt.settings_event,"banner_digest":recovery.banner_digest,
            "from_model":recovery.from_model,"model":attempt.model,"effort":attempt.effort,
            "service_tier":recovery.service_tier},"state":"claimed"})
		.to_string();

		store.run(move |connection| {
            let insert = |key: &str, kind: &str, payload: &str| {
                connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,'root',?2,?3,1,'resolved','Preserved legacy fixture',1)",rusqlite::params![key,kind,payload]).map_err(error::sqlite_error)
            };

            insert(&key,"model_recovery",&original)?;

            let id=connection.last_insert_rowid();
            let (suffix, kind, state)=match terminal {
                "rejected" => ("result","model_recovery_result","rejected"),
                "observed" => ("observation","model_recovery_observation","target_observed"),
                _ => ("reconciliation","model_selection_reconciled","superseded"),
            };

            insert(&format!("{key}:{suffix}"),kind,&serde_json::json!({"reservation":id,"state":state}).to_string())?;

            Ok(())
        }).await.unwrap();

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert!(!store.has_pending_agent_model_change("root".into()).await.unwrap());
		assert!(
			store.reserve_agent_model_selection(attempt.clone()).await.unwrap().is_none(),
			"a completed legacy {terminal} occurrence must remain single-use"
		);
		assert!(store.agent_model_receipt("root".into(), "thread".into()).await.unwrap().is_none());

		let mut next = attempt;

		next.recovery.as_mut().unwrap().banner_digest = OTHER_DIGEST.into();

		assert!(
			store.reserve_agent_model_selection(next).await.unwrap().is_some(),
			"a distinct banner occurrence may reserve"
		);
	}
}
