//! Native inspection and preparation without promoting spawned threads into manager authority.
use std::time::Duration;

use serde_json::Value;
use tokio::time;

use crate::agent::native_subagents;
use decodex_codex::app_server_client::{AppServerClient, ClientError};
use decodex_database::SqliteStore;
use decodex_protocol::{NativeAgentDto, NativeAgentsResult};

pub(crate) async fn read(
	store: &SqliteStore,
	client: &AppServerClient,
	work: &str,
	thread: Option<&str>,
	cursor: Option<&str>,
) -> NativeAgentsResult {
	let result = time::timeout(Duration::from_secs(10), async {
        let owner = store.get_agent_work_item(work.into()).await.ok()?;
        let root = owner.codex_thread_id?;

        if let Some(thread) = thread {
            let verified = native_subagents::request_owner(store,client,thread).await.ok()?;

            if verified.id != work { return None; }

            let value = client.thread_read(serde_json::json!({"threadId":thread,"includeTurns":true})).await.ok()?;

            return conversation(&value,thread);
        }

        let value = client.request("thread/list",serde_json::json!({"ancestorThreadId":root,"sourceKinds":["subAgentThreadSpawn"],"sortKey":"recency_at","sortDirection":"desc","limit":100,"cursor":cursor,"useStateDbOnly":true})).await.ok()?;
        let data = value["data"].as_array()?;

        if data.len()>100 {return None;}

        let agents = data.iter().filter_map(|item| {
            let id = item["id"].as_str()?;
            let parent = item["parentThreadId"].as_str()?;
            let source = item.pointer("/source/subAgent/thread_spawn/parent_thread_id")?.as_str()?;

            if parent != source || id.len()>512 || parent.len()>512 {return None;}

            Some(NativeAgentDto {thread_id:id.into(),parent_thread_id:parent.into(),title:clean(item["name"].as_str().filter(|s|!s.is_empty()).or(item["agentNickname"].as_str()).unwrap_or("Agent"),120),status:clean(item.pointer("/status/type").and_then(Value::as_str).unwrap_or("unknown"),32)})
        }).collect();

        Some(NativeAgentsResult::Available { agents, next_cursor: value["nextCursor"].as_str().map(str::to_owned) })
    }).await;

	result.ok().flatten().unwrap_or(NativeAgentsResult::Unavailable)
}

/// Prepare a conversation, not a turn. Keep inspection read-only and send no queued input.
pub(crate) async fn prepare(
	store: &SqliteStore,
	client: &AppServerClient,
	work: &str,
	thread: &str,
) -> Result<(), &'static str> {
	time::timeout(Duration::from_secs(20), async {
		match read(store, client, work, Some(thread), None).await {
			NativeAgentsResult::Conversation { can_input: Some(_), .. } => return Ok(()),
			NativeAgentsResult::Conversation { can_input: None, .. } => {},
			_ => return Err("Could not connect to this conversation."),
		}
		// Use persisted native configuration; do not override parent-owned child settings.
		let value = client
			.thread_resume(serde_json::json!({"threadId": thread, "excludeTurns": true}))
			.await
			.map_err(|error| match error {
				ClientError::Remote(ref error) if error.message.contains("archived") =>
					"This conversation is archived. Restore it in Codex before continuing.",
				ClientError::Remote(ref error)
					if error.message.contains("resume the parent first") =>
					"Open the parent agent to reconnect this conversation.",
				ClientError::Remote(ref error)
					if error.message.contains("in use") || error.message.contains("locked") =>
					"This conversation is in use by another app.",
				_ => "Could not connect to this conversation.",
			})?;
		if value.pointer("/thread/id").and_then(Value::as_str) != Some(thread) {
			return Err("Could not verify this conversation.");
		}
		Ok(())
	})
	.await
	.unwrap_or(Err("Connecting took too long. Try again."))
}

fn clean(text: &str, limit: usize) -> String {
	if decodex_core::contains_credential_material(text) {
		return "[Private content omitted]".into();
	}

	text.chars().take(limit).collect()
}

fn conversation(value: &Value, thread: &str) -> Option<NativeAgentsResult> {
	if value.pointer("/thread/id")?.as_str()? != thread {
		return None;
	}

	let turns = value.pointer("/thread/turns")?.as_array()?;
	Some(NativeAgentsResult::Conversation {
		thread_id: thread.into(),
		can_input: value.pointer("/thread/canAcceptDirectInput").and_then(Value::as_bool),
		active_turn: turns
			.iter()
			.rev()
			.find(|t| t["status"] == "inProgress")
			.and_then(|t| t["id"].as_str())
			.map(str::to_owned),
	})
}
#[cfg(test)]
mod tests {
	use crate::native_agents::{self, NativeAgentsResult};

	#[test]
	fn native_input_metadata_keeps_turn_identity_and_does_not_guess_capability() {
		let v = serde_json::json!({"thread":{"id":"child","turns":[{"id":"t","status":"inProgress","items":[{"id":"u","type":"userMessage","content":[{"text":"Check"}]},{"id":"a","type":"agentMessage","text":"Result"}]}]}});
		let Some(NativeAgentsResult::Conversation { can_input, active_turn, .. }) =
			native_agents::conversation(&v, "child")
		else {
			panic!()
		};

		assert_eq!(can_input, None);
		assert_eq!(active_turn.as_deref(), Some("t"));
		assert!(native_agents::conversation(&v, "other").is_none());
	}
}
