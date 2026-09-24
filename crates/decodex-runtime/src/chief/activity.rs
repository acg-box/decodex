//! Public activity projection from native item events. Raw arguments and output stay private.
use decodex_protocol::ChiefActivityDto;
use serde_json::Value;

pub(super) fn project(params: &Value, completed: bool) -> Option<ChiefActivityDto> {
	let item = &params["item"];
	let kind = item["type"].as_str()?;
	let label = match kind {
		"reasoning" => "Thinking",
		"commandExecution" => match item["commandActions"]
			.as_array()
			.and_then(|a| a.first())
			.and_then(|a| a["type"].as_str())
		{
			Some("read") => "Reading files",
			Some("listFiles") => "Listing files",
			Some("search") => "Searching files",
			_ => "Running command",
		},
		"fileChange" => "Editing files",
		"mcpToolCall" | "dynamicToolCall" => "Using tool",
		"webSearch" => match item.pointer("/action/type").and_then(Value::as_str) {
			Some("openPage") => "Opening web page",
			Some("findInPage") => "Finding text on page",
			_ => "Searching the web",
		},
		"collabAgentToolCall" => "Coordinating agents",
		"subAgentActivity" => match item["kind"].as_str()? {
			"started" => "Subagent started",
			"interacted" => "Message sent to subagent",
			"interrupted" => "Subagent interrupted",
			"completed" => "Subagent completed a turn",
			_ => return None,
		},
		"contextCompaction" => "Compacting context",
		"imageView" => "Viewing image",
		"imageGeneration" => "Generating image",
		_ => return None,
	};
	let detail = match kind {
		"subAgentActivity" => {
			let agent = item["agentThreadId"].as_str()?;
			let path = item["agentPath"].as_str()?;
			if agent.is_empty()
				|| agent.len() > 512
				|| agent.chars().any(char::is_control)
				|| path.is_empty()
				|| path.len() > 512
				|| path.chars().any(char::is_control)
			{
				return None;
			}
			if decodex_core::contains_credential_material(path) {
				"Subagent".into()
			} else {
				path.to_owned()
			}
		},
		"mcpToolCall" | "dynamicToolCall" => {
			let tool = item["tool"].as_str().unwrap_or("Tool");
			let server = item["server"].as_str().or(item["namespace"].as_str());
			server.map_or_else(|| tool.to_owned(), |server| format!("{server} · {tool}"))
		},
		"commandExecution" =>
			item["exitCode"].as_i64().map_or_else(String::new, |code| format!("Exit code {code}")),
		"fileChange" => item["changes"]
			.as_array()
			.map_or_else(String::new, |changes| format!("{} files", changes.len())),
		_ => String::new(),
	};
	let failed = item["status"] == "failed"
		|| item["success"] == false
		|| item["exitCode"].as_i64().is_some_and(|code| code != 0);
	let status = if !completed {
		"running"
	} else if search_exit_without_failure(item) {
		"exited"
	} else if failed {
		"failed"
	} else if item["status"] == "declined" {
		"declined"
	} else {
		"completed"
	};
	Some(ChiefActivityDto {
		turn_id: params["turnId"].as_str()?.into(),
		item_id: item["id"].as_str()?.into(),
		kind: kind.into(),
		status: status.into(),
		label: label.into(),
		detail: detail.chars().filter(|c| !c.is_control()).take(160).collect(),
		duration_ms: item["durationMs"].as_u64(),
	})
}

// Upstream 71406edb: search exit 1 can mean no matches. Keep the code visible
// without classifying the whole command as a failure or claiming search success.
fn search_exit_without_failure(item: &Value) -> bool {
	item["type"] == "commandExecution"
		&& item["exitCode"] == 1
		&& item["commandActions"]
			.as_array()
			.is_some_and(|actions| actions.iter().any(|action| action["type"] == "search"))
}

#[cfg(test)]
mod tests {
	use super::project;
	use serde_json::json;
	#[test]
	fn subagent_projection_rejects_unknown_or_malformed_identity() {
		let good = json!({"turnId":"turn","item":{"id":"item","type":"subAgentActivity","kind":"started","agentThreadId":"child","agentPath":"/root/worker"}});
		for (field, bad) in [
			("kind", json!("future")),
			("agentThreadId", json!("")),
			("agentPath", json!("x".repeat(513))),
			("agentPath", json!("/root/\nworker")),
		] {
			let mut value = good.clone();
			value["item"][field] = bad;
			assert!(project(&value, true).is_none());
		}
		let mut value = good;
		value["item"]["agentPath"] = json!("Bearer fixture-private-access-token-123456789");
		assert_eq!(project(&value, true).unwrap().detail, "Subagent");
	}
	#[test]
	fn tool_projection_excludes_arguments_and_output() {
		let value = json!({"turnId":"turn", "item":{"id":"item", "type":"mcpToolCall", "server":"docs", "tool":"search", "arguments":{"secret":"DO_NOT_SHOW"}, "result":"DO_NOT_SHOW"}});
		let activity = project(&value, false).expect("activity");
		assert_eq!(activity.detail, "docs · search");
		assert!(!serde_json::to_string(&activity).expect("json").contains("DO_NOT_SHOW"));
	}
	#[test]
	fn compaction_and_command_failure_use_native_evidence() {
		let mut value = json!({"turnId":"turn", "item":{"id":"item", "type":"contextCompaction"}});
		assert_eq!(project(&value, false).expect("start").status, "running");
		assert_eq!(project(&value, true).expect("end").status, "completed");
		value["item"] = json!({"id":"command", "type":"commandExecution", "exitCode":1});
		assert_eq!(project(&value, true).expect("failed").status, "failed");
		value["item"]["type"] = json!("agentMessage");
		assert!(project(&value, true).is_none());
	}
	#[test]
	fn search_exit_one_keeps_the_code_without_marking_activity_failed() {
		for (actions, code, expected) in [
			(json!([{"type":"search"}]), 1, "exited"),
			(json!([{"type":"read"},{"type":"search"}]), 1, "exited"),
			(json!([{"type":"search"}]), 2, "failed"),
			(json!([{"type":"read"}]), 1, "failed"),
			(json!([]), 1, "failed"),
		] {
			let value = json!({"turnId":"turn","item":{"id":"command","type":"commandExecution","status":"failed","exitCode":code,"commandActions":actions}});
			let activity = project(&value, true).unwrap();
			assert_eq!(activity.status, expected);
			assert_eq!(activity.detail, format!("Exit code {code}"));
			assert_eq!(project(&value, false).unwrap().status, "running");
		}
	}
}
