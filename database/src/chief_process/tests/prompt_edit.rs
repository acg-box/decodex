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

#[tokio::test]
async fn prompt_edit_recovery_requires_dead_process_and_same_account_complete_history() {
	for applied in [false, true] {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("prompt.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		seed(&store).await;
		store.bind_chief_thread("root".into(), "task".into()).await.unwrap();
		store
			.prepare_chief_bound_process_generation(&intent(1, 1), &binding(1), "root", "first")
			.await
			.unwrap();
		store.bind_process_generation_identity(&generation_id(1), 1, &identity(123)).await.unwrap();
		store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
		let a = crate::ChiefPromptEditAttempt {
			work: "root".into(),
			thread: "task".into(),
			generation: Some(generation_id(1).as_str().into()),
			review_token: DIGEST.into(),
			attempt_id: "first".into(),
			before_turn_id: "edit".into(),
			item_id: "input".into(),
			turn_ids: vec!["prefix".into(), "edit".into()],
			content: vec![json!({"type":"text","text":"original"})],
		};
		let id = store.reserve_chief_prompt_edit(a.clone()).await.unwrap().unwrap();
		let voice = crate::ChiefVoiceCall {
			session_id: "voice".into(),
			work_id: "root".into(),
			thread_id: "task".into(),
			generation_id: generation_id(1).as_str().into(),
			baseline_turn_id: None,
		};
		assert!(store.begin_chief_voice_call(voice).await.is_err());
		store
			.mark_process_generation_death_unknown(
				&generation_id(1),
				3,
				decodex_core::ProcessAuthorityLossReason::SupervisorRestarted,
			)
			.await
			.unwrap();
		assert!(
			!store
				.observe_chief_prompt_edit(
					id,
					Some(generation_id(1).as_str().into()),
					vec!["prefix".into()]
				)
				.await
				.unwrap()
		);
		assert!(
			!store
				.observe_chief_prompt_edit(
					id,
					Some(generation_id(2).as_str().into()),
					a.turn_ids.clone()
				)
				.await
				.unwrap()
		);
		confirm_death(&store).await;
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(2, 2),
					&binding(2),
					"root",
					"wrong-account"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Rejected { .. }
		));
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
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		assert!(!store.reject_chief_prompt_edit_before_write(id, a.clone()).await.unwrap());
		let turns = if applied { vec!["prefix".into()] } else { a.turn_ids };
		assert!(
			store
				.observe_chief_prompt_edit(id, Some(generation_id(2).as_str().into()), turns)
				.await
				.unwrap()
		);
		assert_eq!(
			store
				.chief_prompt_edit_receipt("root".into(), "task".into())
				.await
				.unwrap()
				.unwrap()
				.state,
			if applied { "applied" } else { "unchanged" }
		);
		if applied {
			assert!(store.begin_chief_dispatch("root".into()).await.is_err());
			assert!(
				store
					.release_chief_prompt_edit_draft(id, Some(generation_id(2).as_str().into()))
					.await
					.unwrap()
			);
		}
		assert!(store.begin_chief_dispatch("root".into()).await.is_ok());
	}
}

async fn confirm_death(store: &SqliteStore) {
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
