//! Attribute plaintext native delivery without inspecting executable Code Mode text.
use super::{AgentCoordinator, AgentError, Value};
use decodex_core::work::NativeWorkDelivery;

impl AgentCoordinator {
	pub(super) async fn observe_native_work_delivery(
		&self,
		params: &Value,
	) -> Result<(), AgentError> {
		let (Some(child), Some(turn), Some(message)) =
			(params["threadId"].as_str(), params["turnId"].as_str(), params["item"]["id"].as_str())
		else {
			return Ok(());
		};
		let Some(recipient) = params["item"]["recipient"].as_str() else { return Ok(()) };
		let Some(prompt) = assignment_payload(&params["item"], recipient) else { return Ok(()) };
		let Ok(history) = self.client.thread_read_turn(child, turn).await else { return Ok(()) };
		let source = &history["thread"]["source"]["subAgent"]["thread_spawn"];
		let (Some(parent), Some(path)) =
			(source["parent_thread_id"].as_str(), source["agent_path"].as_str())
		else {
			return Ok(());
		};
		if history["thread"]["id"] != child
			|| history["thread"]["parentThreadId"] != parent
			|| path != recipient
		{
			return Ok(());
		}
		let Some(parent_turn) = history["thread"]["turns"].as_array().and_then(|turns| {
			turns.iter().find(|t| t["id"] == turn).and_then(|t| t["rootTurnId"].as_str())
		}) else {
			return Ok(());
		};
		let Some(owner) = self
			.store
			.list_agent_work_items()
			.await?
			.into_iter()
			.find(|a| a.codex_thread_id.as_deref() == Some(parent))
		else {
			return Ok(());
		};
		let portfolio = self.store.agent_work_portfolio(owner.id.clone(), None).await?;
		let mut matches = portfolio.native_contributions.iter().filter(|c| {
			c.native_prompt() == prompt && c.child_thread_id.as_deref().is_none_or(|id| id == child)
		});
		let Some(contribution) = matches.next() else { return Ok(()) };
		if matches.next().is_some() {
			return Ok(());
		}
		self.store
			.record_native_work_delivery(
				contribution.id.clone(),
				NativeWorkDelivery {
					parent_thread_id: parent.into(),
					parent_turn_id: parent_turn.into(),
					child_thread_id: child.into(),
					child_turn_id: turn.into(),
					message_id: message.into(),
				},
			)
			.await?;
		if let Some(activity) =
			self.native_delivery_activity(&contribution.id, parent, parent_turn, child).await?
		{
			self.recover_native_contribution(parent, parent_turn, &activity).await?;
		}
		Ok(())
	}

	pub(super) async fn native_delivery_activity(
		&self,
		id: &str,
		parent: &str,
		turn: &str,
		child: &str,
	) -> Result<Option<Value>, AgentError> {
		let Some(delivery) = self.store.native_work_delivery(id.into()).await? else {
			return Ok(None);
		};
		if delivery.parent_thread_id != parent
			|| delivery.parent_turn_id != turn
			|| delivery.child_thread_id != child
		{
			return Ok(None);
		}
		let Ok(history) = self.client.thread_read_turn(parent, turn).await else { return Ok(None) };
		let Some(activity) = assignment_activity(&history, parent, turn, child) else {
			return Ok(None);
		};
		let Some(call) = activity["id"].as_str() else { return Ok(None) };
		self.store
			.record_native_work_dispatch(id.into(), parent.into(), turn.into(), call.into())
			.await?;
		Ok(Some(activity.clone()))
	}
}

fn assignment_activity<'a>(
	history: &'a Value,
	parent: &str,
	turn: &str,
	child: &str,
) -> Option<&'a Value> {
	if history["thread"]["id"] != parent {
		return None;
	}
	let turn = history["thread"]["turns"].as_array()?.iter().find(|t| t["id"] == turn)?;
	let mut activities = turn["items"].as_array()?.iter().filter(|i| {
		i["type"] == "subAgentActivity"
			&& matches!(i["kind"].as_str(), Some("started" | "interacted"))
			&& i["agentThreadId"] == child
	});
	let selected = activities.next()?;
	activities.next().is_none().then_some(selected)
}

fn assignment_payload<'a>(item: &'a Value, recipient: &str) -> Option<&'a str> {
	if item["type"] != "agent_message"
		|| item["author"] != "/root"
		|| item["recipient"] != recipient
	{
		return None;
	}
	let content = item["content"].as_array().filter(|c| c.len() == 1)?;
	if content[0]["type"] != "input_text" {
		return None;
	}
	let provenance = item
		.pointer("/internal_chat_message_metadata_passthrough/content_item_metadata")?
		.as_array()?;
	if provenance.len() != 1 || provenance[0]["provenance"]["type"] != "tool" {
		return None;
	}
	let text = content[0]["text"].as_str()?;
	["NEW_TASK", "MESSAGE"].into_iter().find_map(|kind| {
		text.strip_prefix(&format!(
			"Message Type: {kind}\nTask name: {recipient}\nSender: /root\nPayload:\n"
		))
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn native_assignment_requires_exact_input_identity_and_one_activity() {
		let item = serde_json::json!({"type":"agent_message","author":"/root","recipient":"/root/worker","content":[{"type":"input_text","text":"Message Type: NEW_TASK\nTask name: /root/worker\nSender: /root\nPayload:\nEXACT"}],"internal_chat_message_metadata_passthrough":{"content_item_metadata":[{"provenance":{"type":"tool","namespace":"collaboration"}}]}});
		assert_eq!(assignment_payload(&item, "/root/worker"), Some("EXACT"));
		for (field, value) in
			[("type", "message"), ("author", "/root/other"), ("recipient", "/root/other")]
		{
			let mut invalid = item.clone();
			invalid[field] = serde_json::json!(value);
			assert!(assignment_payload(&invalid, "/root/worker").is_none());
		}
		let activity = serde_json::json!({"type":"subAgentActivity","id":"native-call","kind":"started","agentThreadId":"child"});
		let mut history = serde_json::json!({"thread":{"id":"parent","turns":[{"id":"turn","items":[activity]}]}});
		assert!(assignment_activity(&history, "parent", "turn", "child").is_some());
		history["thread"]["turns"][0]["items"].as_array_mut().unwrap().push(activity);
		assert!(assignment_activity(&history, "parent", "turn", "child").is_none());
	}
}
