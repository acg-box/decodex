//! Upgrade old journal records without replaying native model changes.
use super::*;
use serde_json::{Value, json};

fn identity(number: u32) -> decodex_core::ProcessIdentity {
	decodex_core::ProcessIdentity::new(
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		number,
		decodex_core::ProcessStartIdentity::new(format!("fixture-{number}")).unwrap(),
		number,
		number,
	)
	.unwrap()
}
async fn ready(store: &SqliteStore, manual: bool, response: Option<&str>) -> String {
	seed(store).await;
	store.bind_chief_thread("root".into(), "thread".into()).await.unwrap();
	store
		.prepare_chief_bound_process_generation(&intent(1, 1), &binding(1), "root", "old")
		.await
		.unwrap();
	store.bind_process_generation_identity(&generation_id(1), 1, &identity(123)).await.unwrap();
	store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
	let attempt = json!({"work":"root","thread":"thread","generation":generation_id(1).as_str(),"account":account_id(1).as_str(),"account_revision":1,"settings_event":1,"banner_digest":OTHER_DIGEST,"manual_review":manual.then_some(DIGEST),"from_model":"old","model":"target","effort":"high","service_tier":"priority"});
	let original = json!({"attempt":attempt,"state":"claimed"}).to_string();
	let saved = original.clone();
	let response = response.map(str::to_owned);
	store.run(move |connection| {
		connection.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('model-recovery:old','root','model_recovery',?1,1,'resolved','Preserved fixture',1)", [saved]).map_err(crate::error::sqlite_error)?;
		let reservation = connection.last_insert_rowid();
		if let Some(response) = response {
			connection.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES('model-recovery:old:result','root','model_recovery_result',?1,2,'resolved','Preserved response',2)", [json!({"reservation":reservation,"state":response}).to_string()]).map_err(crate::error::sqlite_error)?;
		}
		Ok(())
	}).await.unwrap();
	original
}
fn facts(tier: Value) -> String {
	json!({"model":"target","modelProvider":"fixture","effort":"high","serviceTier":tier})
		.to_string()
}
async fn pending(store: &SqliteStore, generation: u8) -> Option<crate::ChiefLegacyModelPending> {
	store
		.pending_chief_legacy_model_change(
			"root".into(),
			"thread".into(),
			generation_id(generation).as_str().into(),
		)
		.await
		.unwrap()
}
async fn publish(store: &SqliteStore, generation: u8, value: Option<String>, digest: &str) {
	store
		.record_chief_task_models_publication(
			"thread".into(),
			Some(generation_id(generation).as_str().into()),
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
			assert!(store.has_pending_chief_model_change("root".into()).await.unwrap());
			assert!(store.begin_chief_dispatch("root".into()).await.is_err());
			assert!(
				store
					.pending_chief_legacy_model_change(
						"root".into(),
						"foreign".into(),
						generation_id(1).as_str().into()
					)
					.await
					.unwrap()
					.is_none()
			);
			assert!(pending(&store, 2).await.is_none());
			store
				.record_chief_task_models(
					"thread".into(),
					Some(generation_id(1).as_str().into()),
					Some(facts(json!("priority"))),
					DIGEST.into(),
				)
				.await
				.unwrap();
			assert!(pending(&store, 1).await.is_some(), "historical facts cannot confirm delivery");
			publish(
				&store,
				1,
				Some(
					json!({"model":"target","modelProvider":"fixture","effort":"high"}).to_string(),
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
			publish(&store, 1, Some(facts(json!("priority"))), OTHER_DIGEST).await;
			publish(&store, 1, Some(facts(json!("priority"))), OTHER_DIGEST).await;
			assert!(!store.has_pending_chief_model_change("root".into()).await.unwrap());
			assert!(store.begin_chief_dispatch("root".into()).await.is_ok());
			store
				.run(move |connection| {
					let raw: String = connection
						.query_row(
							"SELECT payload FROM chief_inbox_events WHERE source_event_id='model-recovery:old'",
							[],
							|r| r.get(0),
						)
						.map_err(crate::error::sqlite_error)?;
					assert_eq!(raw, original, "upgrade preserves old bytes");
					let count: i64 = connection
						.query_row(
							"SELECT count(*) FROM chief_inbox_events WHERE event_kind='model_recovery_observation'",
							[],
							|r| r.get(0),
						)
						.map_err(crate::error::sqlite_error)?;
					assert_eq!(count, 1);
					Ok(())
				})
				.await
				.unwrap();
		}
	}
}

#[tokio::test]
async fn legacy_unknown_reconciles_only_after_death_and_complete_new_owner_facts() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("legacy.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	ready(&store, false, Some("uncertain")).await;
	store
		.mark_process_generation_death_unknown(
			&generation_id(1),
			3,
			decodex_core::ProcessAuthorityLossReason::SupervisorRestarted,
		)
		.await
		.unwrap();
	publish(&store, 2, Some(facts(json!("priority"))), DIGEST).await;
	assert!(store.has_pending_chief_model_change("root".into()).await.unwrap());
	assert!(matches!(
		store
			.prepare_chief_bound_process_generation(&intent(1, 2), &binding(1), "root", "early")
			.await
			.unwrap(),
		PrepareProcessGenerationOutcome::Rejected { .. }
	));
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
	store
		.prepare_chief_bound_process_generation(&intent(1, 2), &binding(1), "root", "new")
		.await
		.unwrap();
	store.bind_process_generation_identity(&generation_id(2), 1, &identity(124)).await.unwrap();
	store.mark_process_generation_ready(&generation_id(2), 2).await.unwrap();
	publish(&store, 1, Some(facts(json!("priority"))), DIGEST).await;
	publish(&store, 2, None, DIGEST).await;
	assert!(pending(&store, 2).await.is_some());
	let current =
		json!({"model":"different","modelProvider":"fixture","effort":null,"serviceTier":null})
			.to_string();
	store
		.record_chief_task_models(
			"thread".into(),
			Some(generation_id(2).as_str().into()),
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
	assert!(!reopened.has_pending_chief_model_change("root".into()).await.unwrap());
	assert!(reopened.begin_chief_dispatch("root".into()).await.is_ok());
	reopened.run(|connection| {
		let (observed,reconciled):(i64,i64)=connection.query_row("SELECT count(*) FILTER(WHERE event_kind='model_recovery_observation'),count(*) FILTER(WHERE event_kind='model_selection_reconciled') FROM chief_inbox_events",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(crate::error::sqlite_error)?;
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
							[account_id(1).as_str()],
						)
						.map_err(crate::error::sqlite_error)?;
					Ok(())
				})
				.await
				.unwrap();
		}
		publish(&store, 1, Some(facts(json!("priority"))), DIGEST).await;
		assert_eq!(pending(&store, 1).await.is_none(), rejected);
		store
			.run(|connection| {
				let count: i64 = connection
					.query_row(
						"SELECT count(*) FROM chief_inbox_events WHERE event_kind='model_recovery_observation'",
						[],
						|r| r.get(0),
					)
					.map_err(crate::error::sqlite_error)?;
				assert_eq!(count, 0);
				Ok(())
			})
			.await
			.unwrap();
	}
}
