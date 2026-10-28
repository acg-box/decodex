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
	let input =
		events.iter().find(|e| e.event_kind == "user_message" && e.delivered_turn_id.is_none())?;
	let payload: Value = serde_json::from_str(&input.payload).ok()?;

	if payload["source"] != "user" {
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

	#[tokio::test]
	async fn first_project_input_restores_without_an_assistant_or_native_replay() {
		let temporary = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(temporary.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();

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

		assert_eq!(selected.model.as_ref().unwrap().as_str(), "selected-model");
		assert_eq!(selected.reasoning_effort.as_ref().unwrap().as_str(), "high");
		assert_eq!(store.list_agent_work_items().await.unwrap().len(), 1);

		drop(store);

		let store = SqliteStore::open(&root.paths()).unwrap();

		assert_eq!(initial_settings::pending_creation_input(&store, &item).await, Some(selected));

		let mut admitted = item.clone();

		admitted.codex_thread_id = Some("native-thread".into());

		assert!(initial_settings::pending_creation_input(&store, &admitted).await.is_none());

		let mut uncertain = item;

		uncertain.dispatch_state = AgentDispatchState::Unknown;

		assert!(initial_settings::pending_creation_input(&store, &uncertain).await.is_none());
	}
}
