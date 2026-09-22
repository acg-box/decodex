//! Native observations only: never promote spawned threads into manager authority.
use decodex_codex::app_server_client::AppServerClient;
use decodex_database::SqliteStore;
use decodex_protocol::{NativeAgentDto, NativeAgentMessage, NativeAgentsResult};
use serde_json::{Value, json};
fn clean(text: &str, limit: usize) -> String {
	if decodex_core::contains_credential_material(text) {
		return "[Private content omitted]".into();
	}
	text.chars().take(limit).collect()
}
pub(crate) async fn read(
	store: &SqliteStore,
	client: &AppServerClient,
	work: &str,
	thread: Option<&str>,
	cursor: Option<&str>,
) -> NativeAgentsResult {
	let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let owner = store.get_chief_work_item(work.into()).await.ok()?;
        let root = owner.codex_thread_id?;
        if let Some(thread) = thread {
            if thread == root { return None; }
            let verified = crate::chief::native_subagents::request_owner(store,client,thread).await.ok()?;
            if verified.id != work { return None; }
            let value = client.thread_read(json!({"threadId":thread,"includeTurns":true})).await.ok()?;
            return conversation(&value,thread);
        }
        let value = client.request("thread/list",json!({"ancestorThreadId":root,"sourceKinds":["subAgentThreadSpawn"],"limit":100,"cursor":cursor,"useStateDbOnly":true})).await.ok()?;
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
fn conversation(value: &Value, thread: &str) -> Option<NativeAgentsResult> {
	if value.pointer("/thread/id")?.as_str()? != thread {
		return None;
	}
	let turns = value.pointer("/thread/turns")?.as_array()?;
	let mut messages = Vec::new();
	let mut truncated = turns.len() > 12;
	let mut remaining = 48_000usize;
	for turn in turns.iter().skip(turns.len().saturating_sub(12)) {
		for item in turn["items"].as_array().into_iter().flatten() {
			let (role, text) = match item["type"].as_str()? {
				"agentMessage" => ("assistant", item["text"].as_str().unwrap_or("").to_owned()),
				"userMessage" => (
					"user",
					item["content"]
						.as_array()
						.into_iter()
						.flatten()
						.filter_map(|c| c["text"].as_str())
						.collect::<Vec<_>>()
						.join("\n"),
				),
				_ => continue,
			};
			if text.is_empty() {
				continue;
			}
			if remaining == 0 {
				truncated = true;
				break;
			}
			let bounded = clean(&text, remaining.min(12000));
			truncated |= bounded.chars().count() < text.chars().count();
			remaining = remaining.saturating_sub(bounded.chars().count());
			messages.push(NativeAgentMessage {
				id: item["id"].as_str()?.into(),
				role: role.into(),
				text: bounded,
			});
		}
	}
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
		messages,
		truncated,
	})
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn native_preview_keeps_roles_and_does_not_guess_input_capability() {
		let v = json!({"thread":{"id":"child","turns":[{"id":"t","status":"inProgress","items":[{"id":"u","type":"userMessage","content":[{"text":"Check"}]},{"id":"a","type":"agentMessage","text":"Result"}]}]}});
		let Some(NativeAgentsResult::Conversation { can_input, active_turn, messages, .. }) =
			conversation(&v, "child")
		else {
			panic!()
		};
		assert!(!can_input);
		assert_eq!(active_turn.as_deref(), Some("t"));
		assert_eq!(messages[0].role, "user");
		assert_eq!(messages[1].text, "Result");
		assert!(conversation(&v, "other").is_none());
	}
}
