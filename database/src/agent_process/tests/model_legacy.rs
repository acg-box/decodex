//! Upgrade old journal records without replaying native model changes.
use serde_json::Value;

use crate::{
	AgentInboxEvent, AgentLegacyModelPending, AgentModelAttempt, AgentModelHistory,
	EnqueueAgentEvent, PrepareProcessGenerationOutcome, SqliteStore,
	agent_process::tests::{self, DIGEST, OTHER_DIGEST},
	error,
};
use decodex_core::{
	ProcessAuthorityLossReason, ProcessBootIdentity, ProcessDeathEvidence, ProcessDeathEvidenceId,
	ProcessDeathEvidenceKind, ProcessIdentity, ProcessStartIdentity,
};

fn identity(number: u32) -> ProcessIdentity {
	ProcessIdentity::new(
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		number,
		ProcessStartIdentity::new(format!("fixture-{number}")).unwrap(),
		number,
		number,
	)
	.unwrap()
}
fn facts(tier: Value) -> String {
	serde_json::json!({"model":"target","modelProvider":"fixture","effort":"high","serviceTier":tier})
		.to_string()
}

async fn ready(store: &SqliteStore, manual: bool, response: Option<&str>) -> String {
	ready_with_tier(store, manual, response, serde_json::json!("priority")).await
}

async fn ready_with_tier(
	store: &SqliteStore,
	manual: bool,
	response: Option<&str>,
	tier: Value,
) -> String {
	tests::seed(store).await;

	store.bind_agent_thread("root".into(), "thread".into()).await.unwrap();
	store
		.prepare_agent_bound_process_generation(
			&tests::intent(1, 1),
			&tests::binding(1),
			"root",
			"old",
		)
		.await
		.unwrap();
	store
		.bind_process_generation_identity(&tests::generation_id(1), 1, &identity(123))
		.await
		.unwrap();
	store.mark_process_generation_ready(&tests::generation_id(1), 2).await.unwrap();

	let attempt = serde_json::json!({"work":"root","thread":"thread","generation":tests::generation_id(1).as_str(),"account":tests::account_id(1).as_str(),"account_revision":1,"settings_event":1,"banner_digest":OTHER_DIGEST,"manual_review":manual.then_some(DIGEST),"from_model":"old","model":"target","effort":"high","service_tier":tier});
	let original = serde_json::json!({"attempt":attempt,"state":"claimed"}).to_string();
	let saved = original.clone();
	let response = response.map(str::to_owned);

	store.run(move |connection| {
		connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('model-recovery:old','root','model_recovery',?1,1,'resolved','Preserved fixture',1)", [saved]).map_err(error::sqlite_error)?;

		let reservation = connection.last_insert_rowid();

		if let Some(response) = response {
			connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('model-recovery:old:result','root','model_recovery_result',?1,2,'resolved','Preserved response',2)", [serde_json::json!({"reservation":reservation,"state":response}).to_string()]).map_err(error::sqlite_error)?;
		}

		Ok(())
	}).await.unwrap();

	original
}
#[tokio::test]
async fn current_manual_history_supersedes_legacy_history_without_rewriting_evidence() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("history.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	ready(&store, false, Some("queued")).await;
	publish(&store, 1, Some(facts(serde_json::json!("priority"))), DIGEST).await;

	let old = history(&store, 1).await;

	assert!(!old.manual && old.target_observed);

	let settings = store
		.agent_task_models(
			"root".into(),
			"thread".into(),
			Some(tests::generation_id(1).as_str().into()),
		)
		.await
		.unwrap()
		.unwrap();
	let attempt = AgentModelAttempt {
		work: "root".into(),
		thread: "thread".into(),
		generation: Some(tests::generation_id(1).as_str().into()),
		settings_event: settings.id,
		model: "next".into(),
		model_provider: "fixture".into(),
		effort: Some("high".into()),
		review_token: DIGEST.into(),
		attempt_id: "explicit".into(),
		manual_source: None,
		recovery: None,
	};
	let id = store.reserve_agent_model_selection(attempt.clone()).await.unwrap().unwrap();

	store.finish_agent_model_selection(id, attempt, "unknown".into()).await.unwrap();

	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();
	let current = history(&store, 1).await;

	assert!(current.id > old.id && current.manual);
	assert_eq!((current.model.as_str(), current.response.as_str()), ("next", "unknown"));
	assert!(!current.target_observed && !current.reconciled);

	let mut target: Value = serde_json::from_str(&facts(serde_json::json!("priority"))).unwrap();

	target["model"] = serde_json::json!("next");

	publish(&store, 1, Some(target.to_string()), OTHER_DIGEST).await;

	let confirmed = history(&store, 1).await;

	assert_eq!((confirmed.id, confirmed.response.as_str()), (current.id, "unknown"));
	assert!(confirmed.manual && confirmed.target_observed && !confirmed.reconciled);
	assert!(store.list_pending_agent_events(100).await.unwrap().is_empty());
}
async fn pending(store: &SqliteStore, generation: u8) -> Option<AgentLegacyModelPending> {
	store
		.pending_agent_legacy_model_change(
			"root".into(),
			"thread".into(),
			tests::generation_id(generation).as_str().into(),
		)
		.await
		.unwrap()
}
async fn history(store: &SqliteStore, generation: u8) -> AgentModelHistory {
	store
		.agent_model_history(
			"root".into(),
			"thread".into(),
			tests::generation_id(generation).as_str().into(),
		)
		.await
		.expect("read model history")
		.expect("preserved history")
}
async fn publish(store: &SqliteStore, generation: u8, value: Option<String>, digest: &str) {
	store
		.record_agent_task_models_publication(
			"thread".into(),
			Some(tests::generation_id(generation).as_str().into()),
			value,
			digest.into(),
		)
		.await
		.unwrap();
}

#[tokio::test]
async fn legacy_manual_and_automatic_receipts_keep_tier_and_publication_rules() {
	for manual in [false, true] {
		for (response, expected) in
			[(None, "reserved"), (Some("queued"), "queued"), (Some("uncertain"), "unknown")]
		{
			let dir = tempfile::tempdir().unwrap();
			let path = dir.path().join("legacy.sqlite3");
			let store = SqliteStore::open_test(&path).unwrap();
			let original = ready(&store, manual, response).await;

			drop(store);

			let store = SqliteStore::open_test(&path).unwrap();
			let receipt = pending(&store, 1).await.unwrap();

			assert_eq!(receipt.state, expected);
			assert_eq!(receipt.model, "target");

			let before = history(&store, 1).await;

			assert_eq!((before.manual, before.response.as_str()), (manual, expected));
			assert!(!before.target_observed && !before.reconciled);
			assert!(
				store
					.agent_model_history(
						"root".into(),
						"foreign".into(),
						tests::generation_id(1).as_str().into()
					)
					.await
					.expect("foreign history")
					.is_none()
			);
			assert!(
				store
					.agent_model_history(
						"root".into(),
						"thread".into(),
						tests::generation_id(2).as_str().into()
					)
					.await
					.expect("foreign owner history")
					.is_none()
			);
			assert!(store.has_pending_agent_model_change("root".into()).await.unwrap());
			assert!(store.begin_agent_dispatch("root".into()).await.is_err());
			assert!(
				store
					.pending_agent_legacy_model_change(
						"root".into(),
						"foreign".into(),
						tests::generation_id(1).as_str().into()
					)
					.await
					.unwrap()
					.is_none()
			);
			assert!(pending(&store, 2).await.is_none());

			store
				.record_agent_task_models(
					"thread".into(),
					Some(tests::generation_id(1).as_str().into()),
					Some(facts(serde_json::json!("priority"))),
					DIGEST.into(),
				)
				.await
				.unwrap();

			assert!(pending(&store, 1).await.is_some(), "historical facts cannot confirm delivery");

			publish(
				&store,
				1,
				Some(
					serde_json::json!({"model":"target","modelProvider":"fixture","effort":"high"})
						.to_string(),
				),
				OTHER_DIGEST,
			)
			.await;

			assert!(pending(&store, 1).await.is_some(), "missing tier stays unknown");

			publish(&store, 1, Some(facts(Value::Null)), DIGEST).await;

			assert_eq!(
				pending(&store, 1).await.is_none(),
				manual,
				"only manual edits preserve any reported native tier"
			);

			publish(&store, 1, Some(facts(serde_json::json!("priority"))), OTHER_DIGEST).await;
			publish(&store, 1, Some(facts(serde_json::json!("priority"))), OTHER_DIGEST).await;

			assert!(!store.has_pending_agent_model_change("root".into()).await.unwrap());

			let after = history(&store, 1).await;

			assert_eq!(
				(after.id, after.manual, after.response.as_str()),
				(before.id, manual, expected)
			);
			assert!(
				after.target_observed && !after.reconciled,
				"confirmation preserves the original response"
			);
			assert!(store.begin_agent_dispatch("root".into()).await.is_ok());

			assert_preserved_legacy_bytes(&store, original).await;
		}
	}
}

#[tokio::test]
async fn legacy_unknown_reconciles_only_after_death_and_complete_new_owner_facts() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("legacy.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	ready(&store, false, Some("uncertain")).await;

	let message = visible_message(&store).await;

	store
		.mark_process_generation_death_unknown(
			&tests::generation_id(1),
			3,
			ProcessAuthorityLossReason::SupervisorRestarted,
		)
		.await
		.unwrap();

	publish(&store, 2, Some(facts(serde_json::json!("priority"))), DIGEST).await;

	assert!(store.has_pending_agent_model_change("root".into()).await.unwrap());
	assert!(matches!(
		store
			.prepare_agent_bound_process_generation(
				&tests::intent(1, 2),
				&tests::binding(1),
				"root",
				"early"
			)
			.await
			.unwrap(),
		PrepareProcessGenerationOutcome::Rejected { .. }
	));

	let evidence = ProcessDeathEvidence::new(
		ProcessDeathEvidenceId::new("50000000-0000-4000-8000-000000000001").unwrap(),
		tests::generation_id(1),
		ProcessDeathEvidenceKind::OwnedChildExit,
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		Some(identity(123)),
		DIGEST,
	)
	.unwrap();

	store.record_process_generation_death(4, &evidence).await.unwrap();
	store
		.prepare_agent_bound_process_generation(
			&tests::intent(1, 2),
			&tests::binding(1),
			"root",
			"new",
		)
		.await
		.unwrap();
	store
		.bind_process_generation_identity(&tests::generation_id(2), 1, &identity(124))
		.await
		.unwrap();
	store.mark_process_generation_ready(&tests::generation_id(2), 2).await.unwrap();

	publish(&store, 1, Some(facts(serde_json::json!("priority"))), DIGEST).await;
	publish(&store, 2, None, DIGEST).await;

	assert!(pending(&store, 2).await.is_some());

	let current =
		serde_json::json!({"model":"different","modelProvider":"fixture","effort":null,"serviceTier":null})
			.to_string();

	store
		.record_agent_task_models(
			"thread".into(),
			Some(tests::generation_id(2).as_str().into()),
			Some(current.clone()),
			DIGEST.into(),
		)
		.await
		.unwrap();

	assert!(pending(&store, 2).await.is_some());

	publish(&store, 2, Some(current), OTHER_DIGEST).await;

	assert!(pending(&store, 2).await.is_none());

	drop(store);

	let reopened = SqliteStore::open_test(&path).unwrap();

	assert!(!reopened.has_pending_agent_model_change("root".into()).await.unwrap());

	let receipt = history(&reopened, 2).await;

	assert_eq!(receipt.response, "unknown");
	assert!(!receipt.manual && !receipt.target_observed && receipt.reconciled);

	assert_visible_history(&reopened, &message).await;

	assert!(reopened.begin_agent_dispatch("root".into()).await.is_ok());

	reopened.run(|connection| {
		let (observed,reconciled):(i64,i64)=connection.query_row("SELECT count(*) FILTER(WHERE event_kind='model_recovery_observation'),count(*) FILTER(WHERE event_kind='model_selection_reconciled') FROM agent_inbox_events",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(error::sqlite_error)?;

		assert_eq!((observed,reconciled),(0,1),"reconciliation must not claim old delivery succeeded");

		Ok(())
	}).await.unwrap();
}

#[tokio::test]
async fn legacy_rejection_and_changed_account_do_not_gain_confirmation() {
	for rejected in [false, true] {
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("legacy.sqlite3")).unwrap();

		ready(&store, false, Some(if rejected { "rejected" } else { "queued" })).await;

		if !rejected {
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
		}

		publish(&store, 1, Some(facts(serde_json::json!("priority"))), DIGEST).await;

		assert_eq!(pending(&store, 1).await.is_none(), rejected);

		let receipt = history(&store, 1).await;

		assert_eq!(receipt.response, if rejected { "rejected" } else { "queued" });
		assert!(!receipt.target_observed && !receipt.reconciled);

		store
			.run(|connection| {
				let count: i64 = connection
					.query_row(
						"SELECT count(*) FROM agent_inbox_events WHERE event_kind='model_recovery_observation'",
						[],
						|r| r.get(0),
					)
					.map_err(error::sqlite_error)?;

				assert_eq!(count, 0);

				Ok(())
			})
			.await
			.unwrap();
	}
}

async fn visible_message(store: &SqliteStore) -> AgentInboxEvent {
	store
		.record_agent_observation(EnqueueAgentEvent {
			source_event_id: "visible-message".into(),
			work_item_id: "root".into(),
			event_kind: "assistant_message".into(),
			payload: serde_json::json!({"text":"Visible answer"}).to_string(),
		})
		.await
		.unwrap()
}

async fn assert_visible_history(store: &SqliteStore, message: &AgentInboxEvent) {
	for limit in [1, 20] {
		assert_eq!(
			store.read_agent_work_events("root".into(), limit).await.unwrap(),
			vec![message.clone()]
		);
		assert_eq!(
			store.read_agent_transcript("root".into(), None, limit).await.unwrap().0,
			vec![message.clone()]
		);
	}
}

#[tokio::test]
async fn legacy_model_journal_never_consumes_transcript_pages() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("visible-history.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();

	ready(&store, false, Some("queued")).await;

	let message = visible_message(&store).await;

	publish(&store, 1, Some(facts(serde_json::json!("priority"))), DIGEST).await;
	assert_visible_history(&store, &message).await;
	drop(store);

	let store = SqliteStore::open_test(&path).unwrap();

	assert_visible_history(&store, &message).await;

	let receipt = history(&store, 1).await;

	assert!(receipt.target_observed);
	assert_eq!(receipt.response, "queued");
}

#[tokio::test]
async fn legacy_automatic_unset_tier_requires_an_explicit_null_publication() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("unset-legacy.sqlite3")).unwrap();

	ready_with_tier(&store, false, Some("queued"), Value::Null).await;

	for tier in [None, Some(serde_json::json!(false)), Some(serde_json::json!("priority"))] {
		let mut settings =
			serde_json::json!({"model":"target","modelProvider":"fixture","effort":"high"});

		if let Some(tier) = tier {
			settings["serviceTier"] = tier;
		}

		publish(&store, 1, Some(settings.to_string()), DIGEST).await;

		assert!(pending(&store, 1).await.is_some());
		assert!(!history(&store, 1).await.target_observed);
	}

	publish(&store, 1, Some(facts(Value::Null)), OTHER_DIGEST).await;

	assert!(pending(&store, 1).await.is_none());

	let receipt = history(&store, 1).await;

	assert!(receipt.target_observed && !receipt.manual && !receipt.reconciled);
	assert_eq!(receipt.response, "queued");
}

async fn assert_preserved_legacy_bytes(store: &SqliteStore, original: String) {
	store
		.run(move |connection| {
			let raw: String = connection
				.query_row(
					"SELECT payload FROM agent_inbox_events WHERE source_event_id='model-recovery:old'",
					[],
					|r| r.get(0),
				)
				.map_err(error::sqlite_error)?;

			assert_eq!(raw, original, "upgrade preserves old bytes");

			let count: i64 = connection
				.query_row(
					"SELECT count(*) FROM agent_inbox_events WHERE event_kind='model_recovery_observation'",
					[],
					|r| r.get(0),
				)
				.map_err(error::sqlite_error)?;

			assert_eq!(count, 1);

			Ok(())
		})
		.await
		.unwrap();
}
