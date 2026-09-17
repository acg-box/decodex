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
		"webSearch" => "Searching the web",
		"collabAgentToolCall" => "Coordinating agents",
		"contextCompaction" => "Compacting context",
		"imageView" => "Viewing image",
		"imageGeneration" => "Generating image",
		_ => return None,
	};
	let detail = match kind {
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

#[cfg(test)]
mod tests {
	use super::project;
	use serde_json::json;
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
}
