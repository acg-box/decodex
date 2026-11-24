use decodex_core::work::ThreadRegistration::{self, Assign, Promote, Register, Release};

use crate::{
	AgentDispatchState, DatabaseError, EnqueueAgentEvent, SqliteStore, StoreError,
	agent::tests::item,
};

async fn change(
	store: &SqliteStore,
	relation: ThreadRegistration,
) -> Result<crate::AgentWorkItem, StoreError> {
	store.register_native_thread("thread".into(), "Shared history".into(), relation).await
}

fn assign(agent: &str, expected_owner: Option<&str>) -> ThreadRegistration {
	Assign { agent_id: agent.into(), expected_owner: expected_owner.map(str::to_owned) }
}

#[tokio::test]
async fn permanently_deleted_assistant_managed_threads_cannot_be_registered_again() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("registration.sqlite3")).unwrap();
	let mut assistant = item("assistant", None);
	assistant.status = crate::AgentWorkStatus::Wait;
	store.create_assistant(assistant).await.unwrap();
	let registered = change(&store, assign("assistant", None)).await.unwrap();
	assert!(store.assistant_settings(registered.id.clone()).await.unwrap().is_none());
	store.request_agent_thread_deletion(registered.id.clone(), "thread".into()).await.unwrap();
	assert!(change(&store, Register).await.is_err());
	store.confirm_agent_thread_deletion("thread".into()).await.unwrap();
	assert!(store.managed_conversation_ids("assistant".into()).await.unwrap().is_empty());
	assert!(change(&store, Register).await.is_err());
}

#[tokio::test]
async fn first_registration_saves_its_connection_configuration_in_the_same_transaction() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("registration.sqlite3")).unwrap();
	let apply = |settings: &str| {
		store.register_native_thread_with_role_change(
			"first".into(),
			"First".into(),
			Promote { expected_owner: None },
			Some(settings.to_owned()),
		)
	};
	assert!(apply("invalid").await.is_err());
	assert!(store.agent_work_item_for_thread("first".into()).await.unwrap().is_none());
	let (work, promoted) = apply(r#"{"model":"fixture"}"#).await.unwrap();
	assert!(promoted);
	assert_eq!(store.agent_connection_root().await.unwrap().unwrap().id, work.id);
	assert_eq!(
		store.read_agent_root_settings(&work.id).await.unwrap().as_deref(),
		Some(r#"{"model":"fixture"}"#)
	);
	assert!(!apply(r#"{"model":"fixture"}"#).await.unwrap().1);
	assert!(apply(r#"{"model":"different"}"#).await.is_err());
}

#[tokio::test]
async fn superseded_scoped_instructions_stay_unsent_and_release_the_conversation() {
	use decodex_core::work::{AgentThreadCommand, ThreadInstructionWork, WorkCommand};
	use serde_json::json;
	for change_kind in ["revise", "cancel", "hold", "owner_changed"] {
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("instruction.sqlite3")).unwrap();
		for id in ["actor", "peer"] {
			store.create_agent_manager(item(id, None)).await.unwrap();
			store.bind_agent_thread(id.into(), format!("native-{id}")).await.unwrap();
		}
		change(&store, assign("actor", None)).await.unwrap();
		store.begin_agent_dispatch("actor".into()).await.unwrap();
		store.acknowledge_agent_dispatch("actor".into(), "actor-turn".into()).await.unwrap();
		let requirements = |id| json!({"id":id,"goal":"Check the result","criteria":"Record the outcome","source":"user"});
		let create:WorkCommand=serde_json::from_value(json!({"operation":"create","work":{"id":"job","owner_agent_id":"actor","title":"Check","state":"open","user_hold":false,"requirements":requirements("r1")}})).unwrap();
		store.apply_work_command("actor".into(), "create".into(), create, true).await.unwrap();
		let send = |key: &str, scoped: bool| {
			store.apply_agent_thread_command(
				"actor".into(),
				"actor-turn".into(),
				key.into(),
				AgentThreadCommand::MessageThread {
					thread_id: "thread".into(),
					text: key.into(),
					work: scoped.then(|| ThreadInstructionWork {
						work_id: "job".into(),
						requirements_id: "r1".into(),
					}),
				},
				None,
			)
		};
		send("sent", true).await.unwrap();
		let sent = store.list_agent_wake_events("thread".into(), 2).await.unwrap().remove(0);
		store.begin_agent_dispatch_with_events("thread".into(), vec![sent.id]).await.unwrap();
		store.acknowledge_agent_dispatch("thread".into(), "already-sent".into()).await.unwrap();
		store.complete_agent_turn("thread".into(), "already-sent".into()).await.unwrap();
		let sent = store.get_agent_inbox_event(sent.id).await.unwrap();
		send("now-stale", true).await.unwrap();
		send("ordinary", false).await.unwrap();
		let batch = store.list_agent_wake_events("thread".into(), 10).await.unwrap();
		let (stale, ordinary) = (batch[0].id, batch[1].id);
		if change_kind == "owner_changed" {
			// Retained input can predate current validation. The claim itself must recheck the
			// owner.
			store
				.with_connection(|db| {
					db.execute("UPDATE works SET owner_agent_id='peer' WHERE id='job'", [])
						.unwrap();
					Ok(())
				})
				.unwrap();
		} else {
			let update = match change_kind {
				"revise" =>
					json!({"operation":"revise","work_id":"job","expected_requirements":"r1","requirements":requirements("r2")}),
				"cancel" =>
					json!({"operation":"set_state","work_id":"job","expected_requirements":"r1","state":"cancelled","reason":"Withdrawn","user_hold":false}),
				_ =>
					json!({"operation":"set_state","work_id":"job","expected_requirements":"r1","state":"blocked","reason":"User hold","resume_condition":"User releases","user_hold":true}),
			};
			store
				.apply_work_command(
					"actor".into(),
					"change".into(),
					serde_json::from_value(update).unwrap(),
					true,
				)
				.await
				.unwrap();
			assert_eq!(
				store.list_agent_wake_events("thread".into(), 10).await.unwrap()[0].id,
				ordinary
			);
		}
		assert!(matches!(
			store.begin_agent_dispatch_with_events("thread".into(), vec![stale, ordinary]).await,
			Err(StoreError::AgentInstructionSuperseded)
		));
		let stale = store.get_agent_inbox_event(stale).await.unwrap();
		assert!(stale.disposition.is_some() && stale.delivered_turn_id.is_none());
		assert_eq!(store.get_agent_inbox_event(sent.id).await.unwrap(), sent);
		assert_eq!(store.list_agent_wake_events("thread".into(), 10).await.unwrap().len(), 1);
		store.begin_agent_dispatch_with_events("thread".into(), vec![ordinary]).await.unwrap();
		store.acknowledge_agent_dispatch("thread".into(), "ordinary-turn".into()).await.unwrap();
		store.complete_agent_turn("thread".into(), "ordinary-turn".into()).await.unwrap();
		change(&store, Release { expected_owner: "actor".into() }).await.unwrap();
	}
}

#[tokio::test]
async fn chat_adoption_requires_live_user_authority_and_preserves_a_racing_owner() {
	use decodex_core::work::AgentThreadCommand::AdoptThread;
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("authority.sqlite3")).unwrap();
	for id in ["actor", "another"] {
		store.create_agent_manager(item(id, None)).await.unwrap();
		store.bind_agent_thread(id.into(), format!("native-{id}")).await.unwrap();
	}
	let decision = store
		.enqueue_agent_event(EnqueueAgentEvent {
			work_item_id: "actor".into(),
			source_event_id: "decision".into(),
			event_kind: "user_message".into(),
			payload: serde_json::json!({"text":"Adopt thread.","source":"user"}).to_string(),
		})
		.await
		.unwrap();
	let command = AdoptThread { thread_id: "thread".into(), authority_event_id: decision.id };
	let apply = |turn: &str| {
		store.apply_agent_thread_command(
			"actor".into(),
			turn.into(),
			"adopt".into(),
			command.clone(),
			Some("Existing".into()),
		)
	};
	assert!(apply("inactive").await.is_err());
	store.begin_agent_dispatch_with_events("actor".into(), vec![decision.id]).await.unwrap();
	store.acknowledge_agent_dispatch("actor".into(), "active".into()).await.unwrap();
	assert!(apply("stale").await.is_err());
	assert!(
		store
			.agent_thread_command_applied(
				"actor".into(),
				"active".into(),
				"adopt".into(),
				command.clone()
			)
			.await
			.is_ok()
	);
	change(&store, assign("another", None)).await.unwrap();
	assert!(apply("active").await.is_err());
	assert_eq!(store.managed_conversation_ids("another".into()).await.unwrap(), ["thread"]);
	store.complete_agent_turn("actor".into(), "active".into()).await.unwrap();
	store.begin_agent_dispatch_with_events("actor".into(), vec![]).await.unwrap();
	store.acknowledge_agent_dispatch("actor".into(), "later".into()).await.unwrap();
	assert!(
		store
			.agent_thread_command_applied(
				"actor".into(),
				"later".into(),
				"adopt".into(),
				command.clone()
			)
			.await
			.is_err()
	);
}

#[tokio::test]
async fn registration_preserves_one_conversation_through_adoption_and_promotion() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("registration.sqlite3");
	let store = SqliteStore::open_test(&path).unwrap();
	let work = change(&store, Register).await.unwrap();
	assert_eq!(work.id, "thread");
	assert_eq!(work.codex_thread_id.as_deref(), Some("thread"));
	assert!(store.agents().await.unwrap().is_empty());
	assert!(store.agent_manager_ids().await.unwrap().is_empty());
	assert!(store.list_agent_wake_events(work.id.clone(), 10).await.unwrap().is_empty());
	for agent in ["one", "two"] {
		store.create_agent_manager(item(agent, None)).await.unwrap();
		store.bind_agent_thread(agent.into(), format!("primary-{agent}")).await.unwrap();
	}
	let (first, duplicate) =
		tokio::join!(change(&store, assign("one", None)), change(&store, assign("one", None)));
	assert_eq!(first.unwrap(), work);
	assert_eq!(duplicate.unwrap(), work);
	assert!(matches!(
		change(&store, assign("two", None)).await,
		Err(StoreError::Database(DatabaseError::Conflict))
	));
	assert!(change(&store, Release { expected_owner: "two".into() }).await.is_err());
	assert!(change(&store, Promote { expected_owner: None }).await.is_err());
	assert_eq!(store.agents().await.unwrap()[0].managed_conversation_ids, ["thread"]);
	assert_eq!(store.agents().await.unwrap()[0].native_thread_id.as_deref(), Some("primary-one"));
	assert_eq!(store.agent_tool_version("one".into()).await.unwrap(), 5);
	for _ in 0..2 {
		change(&store, Release { expected_owner: "one".into() }).await.unwrap();
	}
	change(&store, assign("one", None)).await.unwrap();
	change(&store, assign("two", Some("one"))).await.unwrap();
	assert!(store.managed_conversation_ids("one".into()).await.unwrap().is_empty());
	for _ in 0..2 {
		assert_eq!(
			change(&store, Promote { expected_owner: Some("two".into()) }).await.unwrap(),
			work
		);
	}
	assert!(change(&store, assign("one", None)).await.is_err());
	drop(store);
	let store = SqliteStore::open_test(&path).unwrap();
	assert_eq!(store.agent_manager_ids().await.unwrap(), ["one", "thread", "two"]);
	assert_eq!(store.agent_tool_version("thread".into()).await.unwrap(), 5);
	assert!(store.managed_conversation_ids("two".into()).await.unwrap().is_empty());
	assert_eq!(change(&store, Register).await.unwrap(), work);
}

#[tokio::test]
async fn registration_protects_existing_bindings_and_refuses_busy_changes() {
	let dir = tempfile::tempdir().unwrap();
	let store = SqliteStore::open_test(&dir.path().join("bindings.sqlite3")).unwrap();
	store.create_agent_manager(item("existing", None)).await.unwrap();
	store.bind_agent_thread("existing".into(), "primary".into()).await.unwrap();
	let existing =
		store.register_native_thread("primary".into(), "New title".into(), Register).await.unwrap();
	assert_eq!(existing.id, "existing");
	assert_eq!(existing.title, "existing");
	assert!(
		store
			.register_native_thread("existing".into(), "Collision".into(), Register)
			.await
			.is_err()
	);
	assert!(change(&store, assign("missing", None)).await.is_err());
	assert!(store.get_agent_work_item("thread".into()).await.is_err());
	store.with_connection(|c| {
		c.execute("INSERT INTO agent_thread_revisions(work_id,old_thread_id,new_thread_id,created_at_micros) VALUES('existing','retired','primary',1)", []).unwrap();
		Ok(())
	}).unwrap();
	assert!(
		store
			.register_native_thread("retired".into(), "Old assistant".into(), Register)
			.await
			.is_err()
	);
	change(&store, Register).await.unwrap();
	let event = store
		.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: "user-input".into(),
			work_item_id: "thread".into(),
			event_kind: "user_message".into(),
			payload: r#"{"text":"Continue"}"#.into(),
		})
		.await
		.unwrap();
	assert!(matches!(
		change(&store, Promote { expected_owner: None }).await,
		Err(StoreError::AgentThreadInUse)
	));
	let claim =
		store.begin_agent_dispatch_with_events("thread".into(), vec![event.id]).await.unwrap();
	assert_eq!(claim.dispatch_state, AgentDispatchState::Dispatching);
	assert!(matches!(
		change(&store, assign("existing", None)).await,
		Err(StoreError::AgentThreadInUse)
	));
	assert_eq!(change(&store, Register).await.unwrap(), claim);
}
