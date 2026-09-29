use super::*;
use serde_json::json;

#[tokio::test]
async fn acknowledged_fork_keeps_the_live_source_owner_without_adopting_other_managers() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("fork.sqlite3")).unwrap();
	seed(&store).await;
	store.bind_agent_thread("root".into(), "native-source".into()).await.unwrap();
	store
		.prepare_agent_bound_process_generation(&intent(1, 1), &binding(1), "root", "first")
		.await
		.unwrap();
	store.bind_process_generation_identity(&generation_id(1), 1, &identity(123)).await.unwrap();
	store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
	let generation = Some(generation_id(1).as_str().to_owned());
	let attempt = crate::AgentForkAttempt {
		source: crate::AgentPromptEditAttempt {
			work: "root".into(),
			thread: "native-source".into(),
			generation: generation.clone(),
			review_token: DIGEST.into(),
			attempt_id: "fork-review".into(),
			before_turn_id: "first".into(),
			item_id: "input".into(),
			turn_ids: vec!["first".into()],
			content: vec![json!({"type":"text","text":"preserve"})],
		},
		target_work: "branch".into(),
		boundary: crate::AgentForkBoundary::BeforeInput,
	};
	store.reserve_agent_fork(attempt.clone()).await.unwrap().unwrap();
	assert!(
		!store
			.agent_thread_is_owned("branch".into(), "native-fork".into(), generation.clone())
			.await
			.unwrap()
	);
	store.record_agent_fork_identity(attempt.clone(), "native-fork".into()).await.unwrap().unwrap();
	let fork =
		store.acknowledge_agent_fork(attempt, "native-fork".into(), vec![]).await.unwrap().unwrap();
	assert!(
		store
			.agent_thread_is_owned("branch".into(), "native-fork".into(), generation.clone())
			.await
			.unwrap()
	);
	assert!(
		!store.agent_thread_is_owned("branch".into(), "native-fork".into(), None).await.unwrap()
	);
	assert!(
		store
			.release_agent_prompt_edit_draft(fork.edit_receipt_id.unwrap(), generation.clone())
			.await
			.unwrap()
	);
	let mut other = store.get_agent_work_item("branch".into()).await.unwrap();
	other.id = "unrelated-manager".into();
	other.codex_thread_id = None;
	store.create_agent_manager(other, None).await.unwrap();
	store.bind_agent_thread("unrelated-manager".into(), "unrelated-native".into()).await.unwrap();
	assert!(
		!store
			.agent_thread_is_owned(
				"unrelated-manager".into(),
				"unrelated-native".into(),
				generation.clone()
			)
			.await
			.unwrap()
	);
	for (work, thread, allowed) in [
		("unrelated-manager", "unrelated-native", false),
		("root", "native-source", true),
		("branch", "native-fork", true),
	] {
		let result = store
			.begin_agent_voice_call(crate::AgentVoiceCall {
				session_id: format!("voice-{work}"),
				work_id: work.into(),
				thread_id: thread.into(),
				generation_id: generation_id(1).as_str().into(),
				baseline_turn_id: None,
			})
			.await;
		assert_eq!(result.is_ok(), allowed, "voice admission for {work}");
		if allowed {
			store.close_agent_voice_call(format!("voice-{work}")).await.unwrap();
		}
	}
	assert!(
		store
			.agent_thread_is_owned("root".into(), "native-source".into(), generation)
			.await
			.unwrap()
	);
}
