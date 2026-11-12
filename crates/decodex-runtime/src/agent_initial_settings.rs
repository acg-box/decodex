//! Bootstrap the shared native connection from a real first user conversation.
use std::{
	env,
	time::{SystemTime, UNIX_EPOCH},
};

use crate::agent_host::{AccountId, AgentConfig, AgentDispatchState, AgentHost, Value};
use decodex_database::{AgentWorkItem, SqliteStore};
use decodex_protocol::{
	AgentExecutionOverrides, ConversationWorkingDirectory, InitialModelCatalogRequest,
	InitialModelCatalogResult, ModelCatalogPurpose,
};

pub(super) async fn restore_candidate(
	store: &SqliteStore,
) -> Option<(AgentWorkItem, Option<String>)> {
	let deleted = store.deleted_agent_thread_ids().await.ok()?;
	for root in store.list_agent_work_items().await.ok()?.into_iter().filter(|work| {
		work.parent_goal_id.is_none()
			&& !work.codex_thread_id.as_ref().is_some_and(|id| deleted.contains(id))
	}) {
		let saved = store.read_agent_root_settings(&root.id).await.ok()?;
		if saved.is_some()
			|| root.codex_thread_id.is_some()
			|| root.dispatch_state != AgentDispatchState::Idle
			|| store.list_agent_wake_events(root.id.clone(), 100).await.ok()?.iter().any(|event| {
				matches!(event.event_kind.as_str(), "user_message" | "assistant_instruction")
					&& event.delivered_turn_id.is_none()
			}) {
			return Some((root, saved));
		}
	}
	None
}

/// A removed project must not prevent the shared read connection from starting.
/// This changes only the process directory, never the saved thread configuration.
pub(super) fn restore_process_directory(cwd: &str) -> Option<String> {
	if std::fs::metadata(cwd).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
		env::var("HOME").ok()
	} else {
		None
	}
}

impl AgentHost {
	pub(super) async fn initial_agent_settings(
		&self,
		root: &AgentWorkItem,
	) -> Option<(AgentConfig, Option<AccountId>)> {
		let execution = pending_creation_input(&self.store, root).await?;
		let cwd = self
			.store
			.work_directory(root.id.clone())
			.await
			.ok()?
			.or_else(|| env::var_os("HOME").map(|home| home.to_string_lossy().into_owned()))?;
		let directory = ConversationWorkingDirectory::new(cwd.clone()).ok()?;
		let mut effort = execution.reasoning_effort.clone();
		let catalog = self
			.runtime
			.initial_model_catalog(
				&format!("agent-initial-{}", root.id),
				InitialModelCatalogRequest {
					working_directory: directory,
					purpose: ModelCatalogPurpose::Agent,
					account_id: None,
				},
			)
			.await;
		let model =
			if let InitialModelCatalogResult::Available {
				defaults: Some(defaults), models, ..
			} = catalog
			{
				let managed = execution
					.reasoning_effort
					.is_none()
					.then(|| defaults.managed.model.clone())
					.flatten();
				let selected = execution
					.model
					.or(managed)
					.or(defaults.configured.model)
					.or_else(|| defaults.catalog_model.clone())?;
				let now = SystemTime::now()
					.duration_since(UNIX_EPOCH)
					.ok()
					.and_then(|time| i64::try_from(time.as_secs()).ok());
				if let Some((target, _)) = now.and_then(|now| {
					decodex_protocol::AgentModelDto::retired_default(
						selected.as_str(),
						&models,
						defaults.catalog_model.as_ref(),
						now,
					)
				}) {
					if effort.as_ref().is_some_and(|value| !target.efforts.contains(value)) {
						effort = target.default_effort.clone();
					}
					target.model.clone()
				} else {
					selected
				}
			} else {
				execution.model?
			};
		let effort = effort.map(|effort| effort.as_str().to_owned());
		let mut config = AgentConfig::with_optional_effort(model.as_str().into(), effort, cwd);

		config.worker_effort.clone_from(&config.agent_effort);

		let encoded = serde_json::to_string(&config).ok()?;

		self.store.bind_agent_root_settings(&root.id, &encoded).await.ok()?;

		Some((config, None))
	}
}

async fn pending_creation_input(
	store: &SqliteStore,
	root: &AgentWorkItem,
) -> Option<AgentExecutionOverrides> {
	// Never reconstruct creation settings for an admitted or uncertain native effect.
	if root.codex_thread_id.is_some() || root.dispatch_state != AgentDispatchState::Idle {
		return None;
	}

	let events = store.list_agent_wake_events(root.id.clone(), 100).await.ok()?;
	let input = events.iter().find(|e| {
		matches!(e.event_kind.as_str(), "user_message" | "assistant_instruction")
			&& e.delivered_turn_id.is_none()
	})?;
	let payload: Value = serde_json::from_str(&input.payload).ok()?;

	if payload["source"] != "user"
		&& !(input.event_kind == "assistant_instruction" && payload["source"] == "assistant")
	{
		return None;
	}

	let execution: AgentExecutionOverrides = payload
		.pointer("/options/execution")
		.map(|value| serde_json::from_value(value.clone()))
		.transpose()
		.ok()?
		.unwrap_or_default();

	Some(execution)
}

#[cfg(test)]
mod tests {
	use crate::{
		AgentCoordinator,
		agent_host::initial_settings::{self, AgentDispatchState},
	};
	use decodex_core::DecodexRoot;
	use decodex_database::{EnqueueAgentEvent, SqliteStore};

	#[test]
	fn a_removed_project_only_changes_the_shared_connection_directory() {
		let directory = tempfile::tempdir().unwrap();
		let removed = directory.path().join("removed-project");
		let cwd = removed.to_str().unwrap();
		assert_eq!(initial_settings::restore_process_directory(cwd), std::env::var("HOME").ok());
		std::fs::create_dir(&removed).unwrap();
		assert_eq!(initial_settings::restore_process_directory(cwd), None);
		std::fs::remove_dir(&removed).unwrap();
		std::fs::write(&removed, "Not a directory").unwrap();
		assert_eq!(
			initial_settings::restore_process_directory(cwd),
			None,
			"invalid existing targets still pass through native directory admission"
		);
	}

	#[tokio::test]
	async fn first_project_input_restores_past_an_idle_assistant_without_native_replay() {
		let temporary = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(temporary.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		store
			.create_assistant(decodex_database::AgentWorkItem {
				id: "assistant".into(),
				parent_goal_id: None,
				kind: decodex_database::AgentWorkKind::Goal,
				title: "Assistant".into(),
				instructions: "Help the user.".into(),
				codex_thread_id: None,
				dispatch_state: AgentDispatchState::Idle,
				active_turn_id: None,
				status: decodex_database::AgentWorkStatus::Wait,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		assert!(initial_settings::restore_candidate(&store).await.is_none());
		assert!(store.list_agent_wake_events("assistant".into(), 100).await.unwrap().is_empty());

		store
			.register_project(
				"market".into(),
				"Market".into(),
				temporary.path().display().to_string(),
			)
			.await
			.unwrap();

		let input = EnqueueAgentEvent {
   source_event_id: "first-input".into(), work_item_id: "sage".into(), event_kind: "user_message".into(),
   payload: serde_json::json!({"source":"user","options":{"execution":{"model":"selected-model","reasoning_effort":"high"}}}).to_string(),
  };
		let item =
			AgentCoordinator::create_conversation(&store, "sage", input, Some("market".into()))
				.await
				.unwrap();
		let selected = initial_settings::pending_creation_input(&store, &item).await.unwrap();
		assert_eq!(initial_settings::restore_candidate(&store).await.unwrap().0.id, "sage");

		assert_eq!(selected.model.as_ref().unwrap().as_str(), "selected-model");
		assert_eq!(selected.reasoning_effort.as_ref().unwrap().as_str(), "high");
		assert_eq!(store.list_agent_work_items().await.unwrap().len(), 2);

		drop(store);

		let store = SqliteStore::open(&root.paths()).unwrap();

		assert_eq!(initial_settings::pending_creation_input(&store, &item).await, Some(selected));
		assert_eq!(initial_settings::restore_candidate(&store).await.unwrap().0.id, "sage");

		let mut admitted = item.clone();

		admitted.codex_thread_id = Some("native-thread".into());

		assert!(initial_settings::pending_creation_input(&store, &admitted).await.is_none());

		let mut uncertain = item;

		uncertain.dispatch_state = AgentDispatchState::Unknown;

		assert!(initial_settings::pending_creation_input(&store, &uncertain).await.is_none());
	}
}
