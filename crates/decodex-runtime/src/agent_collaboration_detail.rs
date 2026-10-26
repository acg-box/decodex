//! Expand native collaboration evidence on demand. Compact events have no reply body.
use serde_json::Value;

use decodex_codex::app_server_client::{AppServerClient, ClientError};

pub(super) fn parts(item: &Value) -> Vec<String> {
	let mut parts = Vec::new();

	if item["type"] == "subAgentActivity" {
		if let Some(path) = item["agentPath"].as_str() {
			parts.push(format!("Agent: {path}"));
		}
		if let Some(kind) = item["kind"].as_str() {
			parts.push(format!("Event: {kind}"));
		}
	} else {
		for (field, label) in [("tool", "Action"), ("status", "Status"), ("prompt", "Request")] {
			if let Some(text) = item[field].as_str().filter(|text| !text.is_empty()) {
				parts.push(format!("{label}: {text}"));
			}
		}
		for receiver in item["receiverThreadIds"].as_array().into_iter().flatten() {
			let Some(receiver) = receiver.as_str() else { continue };
			let state = &item["agentsStates"][receiver];
			let status = state["status"].as_str().unwrap_or("unknown");

			parts.push(format!("Agent {receiver}: {status}"));

			if let Some(message) = state["message"].as_str().filter(|text| !text.is_empty()) {
				parts.push(message.into());
			}
		}
	}

	parts
}

pub(super) async fn latest_reply(
	client: &AppServerClient,
	parent: &str,
	item: &Value,
) -> Result<Option<String>, ClientError> {
	let Some(child) = item["agentThreadId"].as_str() else { return Ok(None) };
	let page = client.thread_history_page(child, None, 1).await?;

	// Use only the native child named by this event. Reading never resumes the child.
	if page["thread"]["parentThreadId"] != parent
		|| page["thread"]["source"]["subAgent"]["thread_spawn"]["parent_thread_id"] != parent
	{
		return Ok(None);
	}

	let Some(turn) = page["turns"].as_array().and_then(|turns| turns.first()) else {
		return Ok(None);
	};

	Ok(reply(turn).map(str::to_owned))
}

fn reply(turn: &Value) -> Option<&str> {
	let items = turn["items"].as_array()?;
	let messages = || items.iter().rev().filter(|item| item["type"] == "agentMessage");

	messages().find(|item| item["phase"] == "final_answer").or_else(|| messages().next())?["text"]
		.as_str()
		.filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
	use crate::agent_detail::collaboration_detail::{self};

	#[test]
	fn latest_reply_prefers_final_output_and_never_exposes_reasoning() {
		let mut turn = serde_json::json!({"items":[
			{"type":"agentMessage","phase":"final_answer","text":"Use Markdown."},
			{"type":"agentMessage","phase":"commentary","text":"Finishing"},
			{"type":"reasoning","text":"private"}
		]});

		assert_eq!(collaboration_detail::reply(&turn), Some("Use Markdown."));

		turn["items"] = serde_json::json!([{"type":"reasoning","text":"private"}]);

		assert_eq!(collaboration_detail::reply(&turn), None);
	}

	#[test]
	fn legacy_collaboration_keeps_request_and_actual_results() {
		let item = serde_json::json!({"type":"collabAgentToolCall","tool":"wait","status":"completed","prompt":"Review the format","receiverThreadIds":["child"],"agentsStates":{"child":{"status":"completed","message":"Use Markdown."}}});
		let text = collaboration_detail::parts(&item).join("\n");

		assert!(text.contains("Request: Review the format"));
		assert!(text.contains("Use Markdown."));
		assert!(
			!collaboration_detail::parts(
				&serde_json::json!({"type":"subAgentActivity","kind":"completed","agentPath":"/root/reviewer"})
			)
			.join("\n")
			.contains("reply")
		);
	}
}
