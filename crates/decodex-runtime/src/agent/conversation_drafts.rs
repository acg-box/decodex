//! Reserve Live ownership only when media starts and discard unused conversation drafts.
use super::{AgentCoordinator, AgentError, AgentWorkItem, now_micros};
use decodex_database::{AgentDispatchState, AgentWorkKind, AgentWorkStatus};
use decodex_protocol::AgentExecutionOverrides;
use serde_json::{Value, json};

impl AgentCoordinator {
	pub(crate) fn new_conversation_work(id: &str) -> Result<AgentWorkItem, AgentError> {
		let now = now_micros()?;
		Ok(AgentWorkItem {
			id: id.into(),
			parent_goal_id: None,
			kind: AgentWorkKind::Goal,
			title: "New Agent".into(),
			instructions: "Help the user with their requests. Use other agents when useful.".into(),
			codex_thread_id: None,
			status: AgentWorkStatus::Wait,
			dispatch_state: AgentDispatchState::Idle,
			active_turn_id: None,
			next_check_at_micros: None,
			created_at_micros: now,
			updated_at_micros: now,
		})
	}

	/// Remove only positively empty drafts. Existing native turns always retain the chat.
	pub(crate) async fn discard_empty_conversation_drafts(&mut self) -> Result<(), AgentError> {
		for work in self.store.empty_agent_conversation_drafts().await? {
			if let Some(thread) = &work.codex_thread_id {
				// A completed native delete can outlive a lost reply or a host restart.
				// Confirm its saved intent before trying to resume a now-absent thread.
				if self
					.store
					.agent_thread_deletion_plan(work.id.clone(), thread.clone())
					.await?
					.is_some()
					&& self.client.archived_thread_absent(thread).await?
				{
					self.store.confirm_agent_thread_deletion(thread.clone()).await?;
					continue;
				}
				match self.client.thread_latest_turn_id(thread).await? {
					Some(turn) =>
						self.store
							.publish_agent_native_conversation(work.id, thread.clone(), turn)
							.await?,
					None => {
						// A restarted host must resume the exact saved thread before it can
						// attach that conversation to its current process generation.
						self.hydrate_dispatch_thread(thread, &json!({})).await?;
						if !self.store.agent_conversation_draft_is_empty(work.id.clone()).await? {
							continue;
						}
						// Codex 392ced7 can delete a persistent thread before its first turn.
						// These empty threads need not appear in either archive list.
						self.store
							.request_agent_thread_deletion(work.id.clone(), thread.clone())
							.await?;
						self.delete_archived_thread(&work.id, thread).await?;
					},
				}
			} else {
				self.store.discard_empty_agent_work(work.id).await?;
			}
		}
		Ok(())
	}
}

pub(super) fn apply_initial_execution(params: &mut Value, execution: &AgentExecutionOverrides) {
	if let Some(model) = &execution.model {
		params["model"] = json!(model);
	}
	if let Some(effort) = &execution.reasoning_effort {
		params["config"]["model_reasoning_effort"] = json!(effort);
	}
	if let Some(tier) = &execution.service_tier {
		params["serviceTier"] = json!(tier.thread_value());
	}
	if let Some(mode) = execution.permission_mode {
		params["sandbox"] = json!(mode.sandbox());
		params["approvalPolicy"] = json!(mode.approval_policy());
		params["approvalsReviewer"] = json!(mode.reviewer());
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{agent::tests, agent_model_settings::tests::OwnedReviewer};

	#[tokio::test]
	async fn cold_empty_voice_draft_recovers_native_deletion_before_resuming_or_deleting() {
		for already_deleted in [false, true] {
			let history = json!({"_not_listed":true,"_deleted":already_deleted,"opaque thread/1":{"thread":{"id":"opaque thread/1","historyMode":"legacy","turns":[]}}});
			let (mut agent, mut sent, directory) = tests::fixture_with_history(history).await;
			let owner =
				OwnedReviewer::new(directory.path(), &agent.client, "host", "host-turn").await;
			agent.store = owner.store;
			agent.bind_native_generation(owner.key.generation.clone());
			agent
				.store
				.create_assistant(AgentCoordinator::new_conversation_work("assistant").unwrap())
				.await
				.unwrap();
			agent
				.store
				.reserve_agent_voice_draft(
					AgentCoordinator::new_conversation_work("cold-draft").unwrap(),
					None,
					Some("assistant".into()),
				)
				.await
				.unwrap();
			agent
				.store
				.bind_agent_thread("cold-draft".into(), "opaque thread/1".into())
				.await
				.unwrap();
			assert!(
				!agent
					.store
					.agent_thread_is_restorable(
						"cold-draft".into(),
						"opaque thread/1".into(),
						Some(owner.key.generation.as_str().into())
					)
					.await
					.unwrap()
			);
			// Recover an earlier cleanup plan without publishing it as conversation content.
			agent
				.store
				.request_agent_thread_deletion("cold-draft".into(), "opaque thread/1".into())
				.await
				.unwrap();
			assert_eq!(
				agent.store.unpublished_agent_conversation_drafts().await.unwrap(),
				["cold-draft"]
			);
			agent.discard_empty_conversation_drafts().await.unwrap();
			assert!(agent.store.get_agent_work_item("cold-draft".into()).await.is_err());
			assert!(
				agent
					.store
					.agent_thread_is_deleted("cold-draft".into(), "opaque thread/1".into())
					.await
					.unwrap()
			);
			let requests = std::iter::from_fn(|| sent.try_recv().ok()).collect::<Vec<_>>();
			let resumed = requests.iter().position(|r| r["method"] == "thread/resume");
			let deleted = requests.iter().position(|r| r["method"] == "thread/delete");
			if already_deleted {
				assert!(resumed.is_none() && deleted.is_none());
			} else {
				assert!(resumed.unwrap() < deleted.unwrap());
			}
			assert!(
				!requests
					.iter()
					.any(|r| r["method"] == "turn/start" || r["method"] == "thread/start")
			);
		}
	}

	#[tokio::test]
	async fn empty_voice_drafts_delete_native_and_local_history_but_preserve_native_turns() {
		for content in [false, true] {
			let history = json!({"_not_listed":true,"opaque thread/1":{"thread":{"id":"opaque thread/1","historyMode":"legacy",
				"turns":if content {json!([{"id":"existing-turn","status":"completed","items":[]}])} else {json!([])} }}});
			let (mut agent, mut sent, _directory) = tests::fixture_with_history(history).await;
			agent
				.store
				.create_assistant(AgentCoordinator::new_conversation_work("assistant").unwrap())
				.await
				.unwrap();
			let draft = agent
				.store
				.reserve_agent_voice_draft(
					AgentCoordinator::new_conversation_work("voice-draft").unwrap(),
					None,
					Some("assistant".into()),
				)
				.await
				.unwrap();
			agent.ensure_thread(&draft).await.unwrap();
			while sent.try_recv().is_ok() {}
			agent.discard_empty_conversation_drafts().await.unwrap();
			assert_eq!(
				agent.store.get_agent_work_item("voice-draft".into()).await.is_ok(),
				content
			);
			assert!(agent.store.unpublished_agent_conversation_drafts().await.unwrap().is_empty());
			assert!(agent.store.get_agent_work_item("assistant".into()).await.is_ok());
			let requests = std::iter::from_fn(|| sent.try_recv().ok()).collect::<Vec<_>>();
			assert_eq!(
				requests.iter().filter(|r| r["method"] == "thread/delete").count(),
				usize::from(!content)
			);
			assert!(!requests.iter().any(|r| r["method"] == "turn/start"));
			assert!(
				!requests.iter().any(|r| r["method"] == "thread/archive"),
				"an empty draft does not need an archive entry"
			);
		}
	}
}
