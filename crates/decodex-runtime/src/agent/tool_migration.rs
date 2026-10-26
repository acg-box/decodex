//! Recover an already-started tool replacement. Ordinary input keeps its native context.
use std::{collections::HashSet, time::Duration};

use tokio::time;

use crate::agent::{AgentCoordinator, AgentDispatchState, AgentError, AgentWorkItem, Value};
use decodex_codex::app_server_client::{
	NativeTaskPermissions, NativeThreadGoalStatus, ThreadArchiveState,
};

impl AgentCoordinator {
	pub(super) async fn recover_tool_migration(
		&mut self,
		item: &AgentWorkItem,
	) -> Result<AgentWorkItem, AgentError> {
		let Some(old) = &item.codex_thread_id else { return Ok(item.clone()) };

		if !self.is_manager(&item.id).await?
			|| self.store.agent_tool_version(item.id.clone()).await? >= 4
		{
			return Ok(item.clone());
		}
		if item.dispatch_state != AgentDispatchState::Idle || item.active_turn_id.is_some() {
			return Err(AgentError::Busy);
		}

		let Some((original, new, parameters, outcome)) =
			self.store.agent_tool_migration(item.id.clone()).await?
		else {
			// Native resume retains the original dynamic tools. A newer local tool
			// revision does not authorize replacing a user's conversation.
			return Ok(item.clone());
		};

		if &original != old || matches!(outcome.as_str(), "reserved" | "unknown") {
			return Err(migration_pending(
				"Creation is unconfirmed. Preserve the original context and reconcile the exact native creation receipt before retrying.",
			));
		}

		let new = new.ok_or_else(|| migration_pending("Replacement identity is missing"))?;
		let parameters = serde_json::from_str::<Value>(&parameters)
			.map_err(|_| migration_pending("Saved migration parameters are unavailable"))?;
		// Metadata persistence is the supported native route for an empty paginated context.
		// A full-history read before materialization is not supported by the selected artifact.
		let metadata = self
			.client
			.thread_read(serde_json::json!({"threadId":new,"includeTurns":false}))
			.await?;

		if metadata["thread"]["name"] != item.title {
			self.client.name_thread(&new, &item.title).await?;
		}

		let resumed =
			self.client.thread_resume(self.resume_observer_params(&new, true).await?).await?;

		if !migration_matches(&resumed, &parameters) {
			return Err(migration_pending(
				"The replacement settings differ from the saved source. No input was sent and the original context remains retained.",
			));
		}

		let state = self.client.thread_archive_state(old).await?;

		if state == ThreadArchiveState::Active {
			self.hydrate_dispatch_thread(old).await?;
			self.migration_quiescent(old).await?;
			self.store.retire_agent_tool_context(item.id.clone()).await?;

			let _ = time::timeout(
				Duration::from_secs(15),
				self.client.request("thread/archive", serde_json::json!({"threadId":old})),
			)
			.await;
		} else if state == ThreadArchiveState::Archived {
			if self.store.agent_tool_migration_state(item.id.clone()).await?.as_deref()
				!= Some("retiring")
			{
				return Err(migration_pending(
					"The original context was archived outside this migration. Preserve that decision before selecting a replacement.",
				));
			}
		} else {
			return Err(migration_pending("Original archive membership is unconfirmed"));
		}
		if self.client.thread_archive_state(old).await? != ThreadArchiveState::Archived {
			return Err(migration_pending(
				"Native retirement is unconfirmed. Read exact archive membership before continuing.",
			));
		}

		self.store
			.finish_agent_tool_migration(item.id.clone(), old.clone(), Some(new.clone()))
			.await?;
		self.loaded_threads.remove(old);
		self.loaded_threads.insert(new.clone());
		self.persist_task_settings(&new).await?;

		self.store.get_agent_work_item(item.id.clone()).await.map_err(Into::into)
	}

	async fn migration_quiescent(&self, root: &str) -> Result<(), AgentError> {
		self.migration_thread_idle(root).await?;

		let mut cursor = Value::Null;
		let mut seen = HashSet::new();

		loop {
			let page=self.client.request("thread/list",serde_json::json!({"ancestorThreadId":root,"archived":false,"sourceKinds":["subAgentThreadSpawn"],"modelProviders":[],"useStateDbOnly":true,"limit":100,"cursor":cursor})).await?;
			let rows = page["data"]
				.as_array()
				.filter(|r| r.len() <= 100)
				.ok_or_else(|| migration_pending("Native descendant inventory is incomplete"))?;

			for child in rows {
				self.migration_thread_idle(&super::exact(child, "/id")?).await?;
			}

			if page["nextCursor"].is_null() {
				return Ok(());
			}

			let next = super::exact(&page, "/nextCursor")?;

			if !seen.insert(next.clone()) {
				return Err(migration_pending("Native descendant inventory did not advance"));
			}

			cursor = serde_json::json!(next);
		}
	}

	async fn migration_thread_idle(&self, thread: &str) -> Result<(), AgentError> {
		let read = self
			.client
			.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false}))
			.await?;

		if read["thread"]["id"] != thread
			|| !matches!(read["thread"]["status"]["type"].as_str(), Some("idle" | "notLoaded"))
		{
			return Err(migration_pending(
				"A native turn is active or its state is unknown; finish it before replacing tools",
			));
		}
		if self.client.thread_goal(thread).await.map_err(|_|migration_pending("Native Goal state is unavailable. Preserve this context until the native Goal can be inspected."))?.is_some_and(|g| {
			g.status != NativeThreadGoalStatus::Complete
		}) {
			return Err(migration_pending(
				"A native Goal remains active or paused; preserve its lifecycle before replacing tools",
			));
		}
		if read["thread"]["status"]["type"] == "idle" {
			let background = self
				.client
				.request(
					"thread/backgroundTerminals/list",
					serde_json::json!({"threadId":thread,"limit":1}),
				)
				.await?;

			if !background["data"].as_array().is_some_and(Vec::is_empty)
				|| !background["nextCursor"].is_null()
			{
				return Err(migration_pending(
					"A native background command remains; preserve it before replacing tools",
				));
			}
		}

		Ok(())
	}
}
fn migration_pending(reason: &str) -> AgentError {
	AgentError::MigrationPending(reason.into())
}
fn migration_matches(actual: &Value, expected: &Value) -> bool {
	let Some(permissions) = NativeTaskPermissions::from_thread_response(actual) else {
		return false;
	};

	permissions.cwd == expected["cwd"]
		&& serde_json::json!(permissions.profile_id) == expected["permissions"]
		&& permissions.approval_policy == expected["approvalPolicy"]
		&& permissions.approvals_reviewer == expected["approvalsReviewer"]
		&& actual["model"] == expected["model"]
		&& actual["modelProvider"] == expected["modelProvider"]
		&& actual["serviceTier"] == expected["serviceTier"]
		&& actual["reasoningEffort"] == expected["config"]["model_reasoning_effort"]
}
