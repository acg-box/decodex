//! Recovery uses durable process death and the new owner's native observation, never RPC replay.
use super::*;
use serde_json::json;

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
fn facts(profile: Option<&str>) -> String {
	json!({"disabledPluginIds":profile.map(|p|vec![p]).unwrap_or_default()}).to_string()
}

#[tokio::test]
async fn plugin_recovery_requires_dead_old_process_and_current_complete_owner_facts() {
	for profile in [Some("scoped"), Some("different"), None] {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("plugins.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		let (reserved, attempt) = prepare_unknown_plugin_selection(&store, profile).await;
		confirm_original_process_death(&store).await;
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(1, 2),
					&binding(1),
					"root",
					"second"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Fresh(_)
		));
		store.bind_process_generation_identity(&generation_id(2), 1, &identity(124)).await.unwrap();
		store.mark_process_generation_ready(&generation_id(2), 2).await.unwrap();
		assert!(
			store
				.record_chief_task_plugins_publication(
					"task".into(),
					Some(generation_id(1).as_str().into()),
					Some(facts(Some("scoped"))),
					OTHER_DIGEST.into()
				)
				.await
				.unwrap()
				.is_none()
		);
		store
			.record_chief_task_plugins_publication(
				"task".into(),
				Some(generation_id(2).as_str().into()),
				None,
				OTHER_DIGEST.into(),
			)
			.await
			.unwrap();
		assert_eq!(
			store.chief_plugin_receipt("root".into(), "task".into()).await.unwrap().unwrap().state,
			"unknown"
		);
		store
			.record_chief_task_plugins_publication(
				"task".into(),
				Some(generation_id(2).as_str().into()),
				Some(json!({"disabledPluginIds":profile}).to_string()),
				OTHER_DIGEST.into(),
			)
			.await
			.unwrap();
		assert_eq!(
			store.chief_plugin_receipt("root".into(), "task".into()).await.unwrap().unwrap().state,
			"unknown"
		);
		assert!(store.begin_chief_dispatch("root".into()).await.is_err());
		store
			.record_chief_task_plugins_publication(
				"task".into(),
				Some(generation_id(2).as_str().into()),
				Some(facts(profile)),
				OTHER_DIGEST.into(),
			)
			.await
			.unwrap()
			.unwrap();
		let expected = if profile == Some("scoped") { "target_observed" } else { "superseded" };
		assert_eq!(
			store.chief_plugin_receipt("root".into(), "task".into()).await.unwrap().unwrap().state,
			expected
		);
		assert!(
			!store.finish_chief_plugin_selection(reserved, attempt, "queued".into()).await.unwrap()
		);
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert_eq!(
			store.chief_plugin_receipt("root".into(), "task".into()).await.unwrap().unwrap().state,
			expected
		);
		assert!(store.begin_chief_dispatch("root".into()).await.is_ok());
		assert!(store.list_pending_chief_events(100).await.unwrap().is_empty());
	}
}

async fn prepare_unknown_plugin_selection(
	store: &SqliteStore,
	profile: Option<&str>,
) -> (i64, crate::ChiefPluginAttempt) {
	seed(store).await;
	store.bind_chief_thread("root".into(), "task".into()).await.unwrap();
	store
		.prepare_chief_bound_process_generation(&intent(1, 1), &binding(1), "root", "first")
		.await
		.unwrap();
	store.bind_process_generation_identity(&generation_id(1), 1, &identity(123)).await.unwrap();
	store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
	let event = store
		.record_chief_task_plugins_publication(
			"task".into(),
			Some(generation_id(1).as_str().into()),
			Some(facts(Some("readonly"))),
			DIGEST.into(),
		)
		.await
		.unwrap()
		.unwrap();
	let attempt = crate::ChiefPluginAttempt {
		work: "root".into(),
		thread: "task".into(),
		generation: Some(generation_id(1).as_str().into()),
		settings_event: event,
		disabled_plugin_ids: vec!["scoped".into()],
		review_token: DIGEST.into(),
		attempt_id: "first".into(),
	};
	let reserved = store.reserve_chief_plugin_selection(attempt.clone()).await.unwrap().unwrap();
	assert!(
		store
			.finish_chief_plugin_selection(reserved, attempt.clone(), "unknown".into())
			.await
			.unwrap()
	);
	store
		.mark_process_generation_death_unknown(
			&generation_id(1),
			3,
			decodex_core::ProcessAuthorityLossReason::SupervisorRestarted,
		)
		.await
		.unwrap();
	assert!(matches!(
		store
			.prepare_chief_bound_process_generation(&intent(1, 2), &binding(1), "root", "too-early")
			.await
			.unwrap(),
		PrepareProcessGenerationOutcome::Rejected { .. }
	));
	assert!(
		store
			.record_chief_task_plugins_publication(
				"task".into(),
				Some(generation_id(2).as_str().into()),
				Some(facts(profile)),
				OTHER_DIGEST.into()
			)
			.await
			.unwrap()
			.is_none()
	);
	assert_eq!(
		store.chief_plugin_receipt("root".into(), "task".into()).await.unwrap().unwrap().state,
		"unknown"
	);
	(reserved, attempt)
}

async fn confirm_original_process_death(store: &SqliteStore) {
	let evidence = ProcessDeathEvidence::new(
		ProcessDeathEvidenceId::new("50000000-0000-4000-8000-000000000001").unwrap(),
		generation_id(1),
		ProcessDeathEvidenceKind::OwnedChildExit,
		ProcessBootIdentity::new("fixture-boot").unwrap(),
		Some(identity(123)),
		DIGEST,
	)
	.unwrap();
	assert!(matches!(
		store.record_process_generation_death(4, &evidence).await.unwrap(),
		crate::ProcessGenerationMutationOutcome::Applied(_)
	));
}
