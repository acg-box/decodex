//! Agent-initiated actions on an existing native conversation.
use decodex_core::work::{AgentThreadCommand, ThreadRegistration};
use serde_json::{Value, json};

use super::{AgentCoordinator, AgentError, AgentWorkItem, exact};

impl AgentCoordinator {
	pub(super) async fn thread_command(
		&mut self,
		agent: &AgentWorkItem,
		args: &Value,
	) -> Result<Value, AgentError> {
		let command: AgentThreadCommand =
			serde_json::from_value(args["command"].clone()).map_err(|error| {
				AgentError::Invalid(format!("invalid conversation command: {error}"))
			})?;
		let turn = agent
			.active_turn_id
			.clone()
			.ok_or_else(|| AgentError::Invalid("Agent has no active turn".into()))?;
		let command_id = exact(args, "/commandId")?;
		let title = match &command {
			AgentThreadCommand::AdoptThread { thread_id, .. } => {
				if self
					.store
					.agent_thread_command_applied(
						agent.id.clone(),
						turn.clone(),
						command_id.clone(),
						command.clone(),
					)
					.await?
				{
					return Ok(json!({"recorded":true}));
				}
				let relation =
					ThreadRegistration::Assign { agent_id: agent.id.clone(), expected_owner: None };
				Some(self.admit_native_thread(thread_id, &relation).await?.title)
			},
			AgentThreadCommand::MessageThread { .. } => None,
		};
		self.store
			.apply_agent_thread_command(agent.id.clone(), turn, command_id, command, title)
			.await?;
		Ok(json!({"recorded":true}))
	}
}

#[cfg(test)]
mod tests {
	use crate::agent::tests::fixture_with_history;
	use decodex_core::work::ThreadRegistration;
	use decodex_database::EnqueueAgentEvent;
	use serde_json::json;

	#[tokio::test]
	async fn chat_adopts_only_the_selected_thread_and_queues_one_agent_instruction() {
		let thread = json!({"id":"conversation-1","ephemeral":false,"source":"cli","status":{"type":"idle"},"turns":[
			{"id":"target-turn","status":"completed","items":[{"type":"agentMessage","id":"report","text":"The test passed."},{"type":"reasoning","id":"private"}]},
			{"id":"other-turn","status":"completed","items":[{"type":"agentMessage","id":"other","text":"Independent work"}]}
		]});
		let (mut coordinator, _sent, _dir) = fixture_with_history(
			json!({"_threadCatalog":[thread],"conversation-1":{"thread":thread}}),
		)
		.await;
		let store = coordinator.store.clone();
		store
			.register_native_thread(
				"actor".into(),
				"Owner".into(),
				ThreadRegistration::Promote { expected_owner: None },
			)
			.await
			.unwrap();
		let decision = store
			.enqueue_agent_event(EnqueueAgentEvent {
				work_item_id: "actor".into(),
				source_event_id: "input".into(),
				event_kind: "user_message".into(),
				payload: json!({"text":"Please adopt conversation-1 and track the failing test as Work.","source":"user"}).to_string(),
			})
			.await
			.unwrap();
		store.begin_agent_dispatch_with_events("actor".into(), vec![decision.id]).await.unwrap();
		let agent =
			store.acknowledge_agent_dispatch("actor".into(), "actor-turn".into()).await.unwrap();
		let command = |thread: &str| json!({"commandId":"adoption","command":{"operation":"adopt_thread","thread_id":thread,"authority_event_id":decision.id}});
		assert!(
			coordinator.work_tool(&agent, "work_manage", &command("conversation")).await.is_err()
		);
		coordinator.work_tool(&agent, "work_manage", &command("conversation-1")).await.unwrap();
		assert_eq!(
			coordinator.list_work(&agent).await.unwrap()["managedConversations"][0]["threadId"],
			"conversation-1"
		);
		let args = json!({"id":"conversation-1","threadId":"conversation-1"});
		assert_eq!(
			coordinator.read_work_history(&agent, &args).await.unwrap()["threadId"],
			"conversation-1"
		);
		let create = json!({"commandId":"create","command":{"operation":"create","work":{"id":"fix","owner_agent_id":"actor","title":"Fix the failing test","state":"open","user_hold":false,"requirements":{"id":"r1","goal":"Fix the test","criteria":"The test passes","source":format!("event:{}",decision.id)}}}});
		coordinator.work_tool(&agent, "work_manage", &create).await.unwrap();
		let message = json!({"commandId":"follow-up","command":{"operation":"message_thread","thread_id":"conversation-1","text":"Check the failing test.","work":{"work_id":"fix","requirements_id":"r1"}}});
		let result = |turn: &str, item: &str| json!({"commandId":"result","command":{"operation":"record_result","result":{"id":"verified","work_id":"fix","requirements_id":"r1","agent_id":"actor","summary":"The test passed.","source":{"kind":"native_item","thread_id":"conversation-1","turn_id":turn,"item_id":item},"limits":"Single test"}}});
		assert!(
			coordinator
				.work_tool(&agent, "work_manage", &result("target-turn", "report"))
				.await
				.is_err()
		);
		for _ in 0..2 {
			coordinator.work_tool(&agent, "work_manage", &message).await.unwrap();
		}
		let mut messages = store.list_agent_wake_events("conversation-1".into(), 10).await.unwrap();
		assert_eq!(messages.len(), 1);
		let event = messages.pop().unwrap();
		assert_eq!(event.event_kind, "work_instruction");
		let payload: serde_json::Value = serde_json::from_str(&event.payload).unwrap();
		assert_eq!(payload["source"], "agent");
		assert_eq!(payload["agentId"], "actor");
		assert_eq!(
			super::super::wake_message(std::slice::from_ref(&event)).unwrap(),
			"Check the failing test."
		);
		let target = store.get_agent_work_item("conversation-1".into()).await.unwrap();
		let (params, _) = coordinator
			.dispatch_input(&target, "Check the failing test.", &[event.id], false)
			.await
			.unwrap();
		assert_eq!(params["input"], json!([]));
		assert_eq!(params["toolOutput"]["name"], "work_instruction");
		store.begin_agent_dispatch_with_events(target.id.clone(), vec![event.id]).await.unwrap();
		store.acknowledge_agent_dispatch(target.id.clone(), "target-turn".into()).await.unwrap();
		store.complete_agent_turn(target.id, "target-turn".into()).await.unwrap();
		for (actor, work, revision) in
			[("actor", "other", "r1"), ("actor", "fix", "r2"), ("other", "fix", "r1")]
		{
			assert!(
				!store
					.managed_work_turn(
						actor.into(),
						"conversation-1".into(),
						"target-turn".into(),
						work.into(),
						revision.into()
					)
					.await
					.unwrap()
			);
		}
		store
			.register_native_thread(
				"conversation-1".into(),
				"Existing".into(),
				ThreadRegistration::Release { expected_owner: "actor".into() },
			)
			.await
			.unwrap();
		coordinator.work_tool(&agent, "work_manage", &command("conversation-1")).await.unwrap();
		assert!(store.managed_conversation_ids("actor".into()).await.unwrap().is_empty());
		assert!(coordinator.read_work_history(&agent, &args).await.is_err());
		let mut second = message;
		second["commandId"] = json!("later");
		assert!(coordinator.work_tool(&agent, "work_manage", &second).await.is_err());
		store
			.register_native_thread(
				"conversation-1".into(),
				"Independent Agent".into(),
				ThreadRegistration::Promote { expected_owner: None },
			)
			.await
			.unwrap();
		assert!(
			coordinator
				.work_tool(&agent, "work_manage", &result("other-turn", "other"))
				.await
				.is_err()
		);
		assert!(
			coordinator
				.work_tool(&agent, "work_manage", &result("target-turn", "private"))
				.await
				.is_err()
		);
		coordinator
			.work_tool(&agent, "work_manage", &result("target-turn", "report"))
			.await
			.unwrap();
		let complete = json!({"commandId":"complete","command":{"operation":"complete","work_id":"fix","expected_requirements":"r1","assessment":{"requirements_id":"r1","result_ids":["verified"],"summary":"The test now passes.","limits":"Single test"}}});
		coordinator.work_tool(&agent, "work_manage", &complete).await.unwrap();
		assert_eq!(
			store.work_portfolio(Some("fix".into())).await.unwrap().works[0].state,
			decodex_core::work::WorkState::Completed
		);
	}
}
