//! Native observations only: never promote spawned threads into manager authority.
use std::time::Duration;

use serde_json::Value;
use tokio::time;

use crate::agent::native_subagents;
use decodex_codex::app_server_client::AppServerClient;
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
		can_input: value.pointer("/thread/canAcceptDirectInput").and_then(Value::as_bool)
			== Some(true),
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

		assert!(!can_input);
		assert_eq!(active_turn.as_deref(), Some("t"));
		assert!(native_agents::conversation(&v, "other").is_none());
	}
}
