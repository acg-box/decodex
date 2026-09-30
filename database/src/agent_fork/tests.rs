use super::*;
use crate::{AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus};

async fn seed(store: &SqliteStore) {
	store
		.create_agent_work_item(AgentWorkItem {
			id: "main".into(),
			parent_goal_id: None,
			kind: AgentWorkKind::Goal,
			title: "Main".into(),
			instructions: "Original objective".into(),
			codex_thread_id: None,
			status: AgentWorkStatus::Open,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
			active_turn_id: None,
			dispatch_state: AgentDispatchState::Idle,
		})
		.await
		.unwrap();
	store.bind_agent_thread("main".into(), "source-native".into()).await.unwrap();
}

fn attempt(boundary: AgentForkBoundary) -> AgentForkAttempt {
	AgentForkAttempt {
		source: AgentPromptEditAttempt {
			work: "main".into(),
			thread: "source-native".into(),
			generation: None,
			review_token: "a".repeat(64),
			attempt_id: "review".into(),
			before_turn_id: "first".into(),
			item_id: "input".into(),
			turn_ids: vec!["first".into(), "second".into()],
			content: vec![
				json!({"type":"text","text":"Original input"}),
				json!({"type":"image","fileId":"native-file"}),
				json!({"type":"skill","name":"review","path":"/skills/review (local)/SKILL.md"}),
			],
		},
		target_work: "branch".into(),
		boundary,
	}
}

#[tokio::test]
async fn fork_receipt_survives_restart_without_repeating_creation_or_changing_source() {
	for (boundary, before, expected) in [
		(AgentForkBoundary::BeforeInput, "first", vec![]),
		(AgentForkBoundary::AfterTurn, "first", vec!["first"]),
		(AgentForkBoundary::BeforeInput, "second", vec!["first"]),
		(AgentForkBoundary::AfterTurn, "second", vec!["first", "second"]),
	] {
		let expected: Vec<String> = expected.into_iter().map(String::from).collect();
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("fork.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		seed(&store).await;

		let original = store.get_agent_work_item("main".into()).await.unwrap();
		let mut a = attempt(boundary);

		a.source.before_turn_id = before.into();

		assert_eq!(a.expected_turns(), Some(expected.as_slice()));

		let saved = store.reserve_agent_fork(a.clone()).await.unwrap().unwrap();

		assert_eq!(saved.state, "reserved");
		assert!(
			store
				.agent_prompt_edit_receipt("main".into(), "source-native".into())
				.await
				.unwrap()
				.is_none()
		);

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert_eq!(
			store.agent_fork_receipt("main".into(), "a".repeat(64)).await.unwrap().unwrap(),
			saved
		);
		assert!(store.reserve_agent_fork(a.clone()).await.unwrap().is_none());
		assert_eq!(
			store.get_agent_work_item("branch".into()).await.unwrap().dispatch_state,
			AgentDispatchState::Dispatching
		);
		assert!(
			store
				.acknowledge_agent_fork(a.clone(), "source-native".into(), expected.clone())
				.await
				.is_err()
		);

		let acknowledged = store
			.record_agent_fork_identity(a.clone(), "fork-native".into())
			.await
			.unwrap()
			.unwrap();

		assert_eq!(acknowledged.state, "acknowledged");

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();

		assert_eq!(
			store
				.agent_fork_receipt("main".into(), a.source.review_token.clone())
				.await
				.unwrap()
				.unwrap(),
			acknowledged
		);
		assert!(
			store
				.record_agent_fork_identity(a.clone(), "different-fork".into())
				.await
				.unwrap()
				.is_none()
		);
		assert!(
			store
				.acknowledge_agent_fork(a.clone(), "fork-native".into(), vec!["wrong".into()])
				.await
				.is_err()
		);

		let result = store
			.acknowledge_agent_fork(a.clone(), "fork-native".into(), expected.clone())
			.await
			.unwrap()
			.unwrap();

		assert_eq!(result.state, "forked");
		assert_eq!(store.get_agent_work_item("main".into()).await.unwrap(), original);

		let branch = store.get_agent_work_item("branch".into()).await.unwrap();

		assert_eq!(branch.parent_goal_id.as_deref(), Some("main"));
		assert_eq!(branch.codex_thread_id.as_deref(), Some("fork-native"));
		assert_eq!(branch.dispatch_state, AgentDispatchState::Idle);
		assert!(store.agent_manager_ids().await.unwrap().contains(&"branch".into()));
		assert_eq!(
			store
				.acknowledge_agent_fork(a.clone(), "fork-native".into(), expected.clone())
				.await
				.unwrap()
				.unwrap(),
			result
		);

		if boundary == AgentForkBoundary::BeforeInput {
			let edit = store
				.agent_prompt_edit_receipt("branch".into(), "fork-native".into())
				.await
				.unwrap()
				.unwrap();

			assert_eq!(edit.id, result.edit_receipt_id.unwrap());
			assert_eq!(edit.state, "applied");
			assert_eq!(edit.attempt.content, a.source.content);
			assert!(store.release_agent_prompt_edit_draft(edit.id, None).await.unwrap());
		} else {
			assert!(result.edit_receipt_id.is_none());
		}
	}
}

#[tokio::test]
async fn definite_fork_refusal_resolves_only_the_new_unbound_work() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("fork.sqlite3")).unwrap();

	seed(&store).await;

	let original = store.get_agent_work_item("main".into()).await.unwrap();
	let a = attempt(AgentForkBoundary::BeforeInput);

	store.reserve_agent_fork(a.clone()).await.unwrap().unwrap();

	assert!(store.reject_agent_fork(a.clone()).await.unwrap());
	assert!(!store.reject_agent_fork(a.clone()).await.unwrap());
	assert_eq!(
		store
			.agent_fork_receipt("main".into(), a.source.review_token.clone())
			.await
			.unwrap()
			.unwrap()
			.state,
		"rejected"
	);
	assert_eq!(store.get_agent_work_item("main".into()).await.unwrap(), original);

	let branch = store.get_agent_work_item("branch".into()).await.unwrap();

	assert_eq!(branch.status, AgentWorkStatus::Resolved);
	assert!(branch.codex_thread_id.is_none());
	assert!(store.acknowledge_agent_fork(a, "late-native".into(), vec![]).await.unwrap().is_none());
}
