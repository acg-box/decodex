use super::*;
use crate::{ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus, EnqueueChiefEvent};

#[tokio::test]
async fn canonical_prompt_input_is_durable_immutable_and_never_a_wake_event() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("input.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	seed(&store, "task", "native").await;
	seed(&store, "other", "other-thread").await;
	let receipt = store.reserve_chief_prompt_edit(attempt()).await.unwrap().unwrap();
	let content = vec![
		json!({"type":"text","text":"Edited — 保留","text_elements":[]}),
		json!({"type":"image","fileId":"native-file","detail":"original"}),
		json!({"type":"mention","name":"App","path":"app://exact-id"}),
	];
	assert!(
		store
			.retain_chief_prompt_input("task".into(), "native".into(), receipt, content.clone())
			.await
			.is_err()
	);
	assert!(store.observe_chief_prompt_edit(receipt, None, vec!["prefix".into()]).await.unwrap());
	let saved = store
		.retain_chief_prompt_input("task".into(), "native".into(), receipt, content.clone())
		.await
		.unwrap();
	assert_eq!(
		store
			.retain_chief_prompt_input("task".into(), "native".into(), receipt, content.clone())
			.await
			.unwrap(),
		saved
	);
	assert!(
		store
			.retain_chief_prompt_input(
				"other".into(),
				"other-thread".into(),
				receipt,
				content.clone()
			)
			.await
			.is_err()
	);
	assert!(
		store
			.chief_prompt_input(saved.id, "other".into(), "other-thread".into())
			.await
			.unwrap()
			.is_none()
	);
	assert!(store.read_chief_work_events("task".into(), 100).await.unwrap().is_empty());
	let id = saved.id;
	assert!(
		store
			.run(move |c| {
				Ok(c.execute("UPDATE chief_prompt_inputs SET content='[]' WHERE id=?1", [id])
					.is_err())
			})
			.await
			.unwrap()
	);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_eq!(
		store.chief_prompt_input(id, "task".into(), "native".into()).await.unwrap(),
		Some(saved)
	);
	assert!(store.release_chief_prompt_edit_draft(receipt, None).await.unwrap());
	let mut changed = content;
	changed[0]["text"] = json!("Another explicit edit");
	let next = store
		.retain_chief_prompt_input("task".into(), "native".into(), receipt, changed)
		.await
		.unwrap();
	assert_ne!(next.id, id);
	assert!(store.read_chief_work_events("task".into(), 100).await.unwrap().is_empty());
}

#[tokio::test]
async fn canonical_prompt_input_retains_large_media_and_rejects_oversized_content() {
	let directory = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("large-input.sqlite3")).unwrap();
	seed(&store, "task", "native").await;
	let receipt = store.reserve_chief_prompt_edit(attempt()).await.unwrap().unwrap();
	store.observe_chief_prompt_edit(receipt, None, vec!["prefix".into()]).await.unwrap();
	let content = vec![
		json!({"type":"image","url":format!("data:image/png;base64,{}", "A".repeat(1024 * 1024))}),
	];
	let saved = store
		.retain_chief_prompt_input("task".into(), "native".into(), receipt, content.clone())
		.await
		.unwrap();
	assert_eq!(saved.content, content);
	assert!(
		store
			.retain_chief_prompt_input(
				"task".into(),
				"native".into(),
				receipt,
				vec![
					json!({"type":"text","text":"x".repeat(decodex_core::MAX_NATIVE_MESSAGE_BYTES)})
				]
			)
			.await
			.is_err()
	);
	assert!(
		store.chief_prompt_input(saved.id, "task".into(), "native".into()).await.unwrap().is_some()
	);
}

fn attempt() -> ChiefPromptEditAttempt {
	ChiefPromptEditAttempt {
		work: "task".into(),
		thread: "native".into(),
		generation: None,
		review_token: "a".repeat(64),
		attempt_id: "attempt".into(),
		before_turn_id: "edit".into(),
		item_id: "input".into(),
		turn_ids: vec!["prefix".into(), "edit".into(), "suffix".into()],
		content: vec![
			json!({"type":"text","text":"Use $skill","text_elements":[{"byteRange":{"start":4,"end":10},"placeholder":"skill"}]}),
			json!({"type":"localImage","path":"/fixture/image.png"}),
		],
	}
}
async fn seed(store: &SqliteStore, id: &str, thread: &str) {
	store
		.create_chief_work_item(ChiefWorkItem {
			id: id.into(),
			parent_goal_id: None,
			kind: ChiefWorkKind::Goal,
			title: id.into(),
			instructions: "Work".into(),
			codex_thread_id: None,
			dispatch_state: ChiefDispatchState::Idle,
			active_turn_id: None,
			status: ChiefWorkStatus::Open,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
		})
		.await
		.unwrap();
	store.bind_chief_thread(id.into(), thread.into()).await.unwrap();
}
fn input(id: &str) -> EnqueueChiefEvent {
	EnqueueChiefEvent {
		source_event_id: id.into(),
		work_item_id: "task".into(),
		event_kind: "user_message".into(),
		payload: json!({"text":"New input"}).to_string(),
	}
}

#[tokio::test]
async fn restart_keeps_unknown_edit_fenced_until_exact_prefix_and_draft_release() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("edit.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	seed(&store, "task", "native").await;
	seed(&store, "other", "other-thread").await;
	let a = attempt();
	let id = store.reserve_chief_prompt_edit(a.clone()).await.unwrap().unwrap();
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_eq!(
		store
			.chief_prompt_edit_receipt("task".into(), "native".into())
			.await
			.unwrap()
			.unwrap()
			.attempt,
		a
	);
	assert!(store.reserve_chief_prompt_edit(a.clone()).await.unwrap().is_none());
	assert!(store.begin_chief_dispatch_with_events("task".into(), vec![]).await.is_err());
	assert!(store.begin_chief_tool_upgrade("task".into(), "native".into()).await.is_err());
	assert!(store.enqueue_chief_event(input("new")).await.is_err());
	assert!(store.begin_chief_dispatch_with_events("other".into(), vec![]).await.is_ok());
	assert!(
		!store.observe_chief_prompt_edit(id, None, a.turn_ids.clone()).await.unwrap(),
		"unchanged history does not prove a live request cannot commit"
	);
	assert!(
		!store.observe_chief_prompt_edit(id, None, vec![]).await.unwrap(),
		"a different revert is not the reviewed boundary"
	);
	assert!(
		!store
			.observe_chief_prompt_edit(id, Some("foreign".into()), vec!["prefix".into()])
			.await
			.unwrap()
	);
	assert!(!store.release_chief_prompt_edit_draft(id, None).await.unwrap());
	assert!(store.observe_chief_prompt_edit(id, None, vec!["prefix".into()]).await.unwrap());
	assert!(
		store.begin_chief_dispatch_with_events("task".into(), vec![]).await.is_err(),
		"applied history still needs draft restoration"
	);
	assert!(!store.reject_chief_prompt_edit_without_mutation(id, a.clone()).await.unwrap());
	assert!(
		store.read_chief_work_events("task".into(), 100).await.unwrap().is_empty(),
		"journal cannot leak into transcript or consume its page"
	);
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_eq!(
		store
			.chief_prompt_edit_receipt("task".into(), "native".into())
			.await
			.unwrap()
			.unwrap()
			.state,
		"applied"
	);
	assert!(store.release_chief_prompt_edit_draft(id, None).await.unwrap());
	assert!(!store.release_chief_prompt_edit_draft(id, None).await.unwrap());
	let mut retry = a.clone();
	retry.attempt_id = "new-id-same-review".into();
	assert!(store.reserve_chief_prompt_edit(retry).await.unwrap().is_none());
	assert!(store.enqueue_chief_event(input("new")).await.is_ok());
}

#[tokio::test]
async fn reservation_and_prewrite_rejection_require_exact_idle_owned_evidence() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("edit.sqlite3")).unwrap();
	seed(&store, "task", "native").await;
	for invalid in ["duplicate", "boundary", "content", "foreign"] {
		let mut a = attempt();
		match invalid {
			"duplicate" => a.turn_ids.push("edit".into()),
			"boundary" => a.before_turn_id = "missing".into(),
			"content" => a.content.clear(),
			_ => a.generation = Some("foreign".into()),
		}
		assert!(!matches!(store.reserve_chief_prompt_edit(a).await, Ok(Some(_))));
	}
	let a = attempt();
	let id = store.reserve_chief_prompt_edit(a.clone()).await.unwrap().unwrap();
	let mut wrong = a.clone();
	wrong.attempt_id = "wrong".into();
	assert!(!store.reject_chief_prompt_edit_without_mutation(id, wrong).await.unwrap());
	assert!(store.reject_chief_prompt_edit_without_mutation(id, a.clone()).await.unwrap());
	assert!(store.reserve_chief_prompt_edit(a.clone()).await.unwrap().is_none());
	store.enqueue_chief_event(input("queued")).await.unwrap();
	let mut next = a;
	next.review_token = "b".repeat(64);
	assert!(
		store.reserve_chief_prompt_edit(next).await.unwrap().is_none(),
		"preserve queued input rather than silently discarding it"
	);
}
