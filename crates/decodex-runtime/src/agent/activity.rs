//! Public activity projection from native item events. Raw arguments and output stay private.
use serde_json::Value;

use crate::agent::image_generation;
use decodex_protocol::AgentActivityDto;

pub(super) fn project(params: &Value, completed: bool) -> Option<AgentActivityDto> {
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
		"mcpToolCall" if completed && authentication_required(item) => "Sign-in required",
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
		"functionCallOutput" => "Tool result",
		"contextCompaction" => "Compacting context",
		"imageView" => "Viewing image",
		"imageGeneration" if image_generation::is_quota_failure(item) =>
			"Image generation limit reached",
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
		"imageGeneration" => image_generation::quota_detail(item).unwrap_or_default(),
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

	Some(AgentActivityDto {
		turn_id: params["turnId"].as_str()?.into(),
		item_id: item["id"].as_str()?.into(),
		kind: kind.into(),
		status: status.into(),
		label: label.into(),
		detail: detail.chars().filter(|c| !c.is_control()).take(160).collect(),
		plugin_id: item["pluginId"]
			.as_str()
			.filter(|id| !id.is_empty() && !decodex_core::contains_credential_material(id))
			.map(|id| id.chars().filter(|c| !c.is_control()).take(160).collect()),
		read_only_hint: if kind == "mcpToolCall" { item["readOnlyHint"].as_bool() } else { None },
		native_timestamp_ms: params[if completed { "completedAtMs" } else { "startedAtMs" }]
			.as_u64(),
		duration_ms: item["durationMs"].as_u64(),
	})
}

fn authentication_required(item: &Value) -> bool {
	if item["status"] != "failed" {
		return false;
	}

	// Native MCP uses a string for local expiry and an array for HTTP challenges.
	// Only project the reconnect signal, never challenge URLs or transport details.
	let valid = |value: &Value| value.as_str().is_some_and(|text| !text.trim().is_empty());
	let challenge = &item["result"]["_meta"]["mcp/www_authenticate"];

	valid(challenge)
		|| challenge.as_array().is_some_and(|values| !values.is_empty() && values.iter().all(valid))
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

	use crate::agent::activity;

	#[test]
	fn mcp_attribution_and_advisory_hint_preserve_missing_history() {
		let mut value = serde_json::json!({"turnId":"turn","item":{"type":"mcpToolCall","id":"item","server":"docs","tool":"read","pluginId":"docs@example","readOnlyHint":true}});

		for completed in [false, true] {
			let activity = activity::project(&value, completed).unwrap();

			assert_eq!(activity.plugin_id.as_deref(), Some("docs@example"));
			assert_eq!(activity.read_only_hint, Some(true));
		}
		for (hint, expected) in [
			(serde_json::json!(false), Some(false)),
			(serde_json::Value::Null, None),
			(serde_json::json!("true"), None),
		] {
			value["item"]["readOnlyHint"] = hint;

			assert_eq!(activity::project(&value, true).unwrap().read_only_hint, expected);
		}

		value["item"].as_object_mut().unwrap().remove("pluginId");

		assert!(activity::project(&value, true).unwrap().plugin_id.is_none());

		let old = serde_json::json!({"turn_id":"turn","item_id":"item","kind":"mcpToolCall","status":"completed","label":"Using tool","detail":"docs","duration_ms":null});
		let old: decodex_protocol::AgentActivityDto = serde_json::from_value(old).unwrap();

		assert!(old.plugin_id.is_none() && old.read_only_hint.is_none());
	}

	#[test]
	fn web_action_labels_do_not_expose_raw_parameters() {
		for (kind, label) in [
			("search", "Searching the web"),
			("openPage", "Opening web page"),
			("findInPage", "Finding text on page"),
			("other", "Searching the web"),
		] {
			let params = serde_json::json!({"turnId":"t","item":{"id":"i","type":"webSearch","action":{"type":kind,"url":"private","pattern":"private"}}});
			let activity = activity::project(&params, true).unwrap();

			assert_eq!(activity.label, label);
			assert!(activity.detail.is_empty());
			assert_eq!(activity.status, "completed");
		}
	}

	#[test]
	fn mcp_authentication_challenge_is_visible_without_exposing_metadata() {
		let mut value = serde_json::json!({"turnId":"turn","item":{"id":"item","type":"mcpToolCall",
			"server":"docs","tool":"search","status":"failed","result":{"content":[],
			"_meta":{"mcp/www_authenticate":"Bearer PRIVATE"}}}});

		for challenge in [
			serde_json::json!("Bearer PRIVATE"),
			serde_json::json!(["Basic PRIVATE", "Bearer PRIVATE"]),
		] {
			value["item"]["result"]["_meta"]["mcp/www_authenticate"] = challenge;

			let activity = activity::project(&value, true).expect("activity");

			assert_eq!(activity.label, "Sign-in required");
			assert_eq!(activity.status, "failed");
			assert_eq!(activity.detail, "docs · search");
			assert!(!serde_json::to_string(&activity).expect("json").contains("PRIVATE"));
			assert_eq!(activity::project(&value, false).expect("started").label, "Using tool");
		}

		value["item"]["status"] = serde_json::json!("completed");

		assert_eq!(activity::project(&value, true).expect("success").label, "Using tool");

		value["item"]["status"] = serde_json::json!("failed");

		for challenge in [
			serde_json::json!(null),
			serde_json::json!(" "),
			serde_json::json!([]),
			serde_json::json!([1]),
			serde_json::json!({"url":"PRIVATE"}),
		] {
			value["item"]["result"]["_meta"]["mcp/www_authenticate"] = challenge;

			assert_eq!(activity::project(&value, true).expect("other failure").label, "Using tool");
		}
	}

	#[test]
	fn subagent_projection_rejects_unknown_or_malformed_identity() {
		let good = serde_json::json!({"turnId":"turn","item":{"id":"item","type":"subAgentActivity","kind":"started","agentThreadId":"child","agentPath":"/root/worker"}});

		for (field, bad) in [
			("kind", serde_json::json!("future")),
			("agentThreadId", serde_json::json!("")),
			("agentPath", serde_json::json!("x".repeat(513))),
			("agentPath", serde_json::json!("/root/\nworker")),
		] {
			let mut value = good.clone();

			value["item"][field] = bad;

			assert!(activity::project(&value, true).is_none());
		}

		let mut value = good;

		value["item"]["agentPath"] =
			serde_json::json!("Bearer fixture-private-access-token-123456789");

		assert_eq!(activity::project(&value, true).unwrap().detail, "Subagent");
	}

	#[test]
	fn tool_projection_excludes_arguments_and_output() {
		let value = serde_json::json!({"turnId":"turn", "item":{"id":"item", "type":"mcpToolCall", "server":"docs", "tool":"search", "arguments":{"secret":"DO_NOT_SHOW"}, "result":"DO_NOT_SHOW"}});
		let activity = activity::project(&value, false).expect("activity");

		assert_eq!(activity.detail, "docs · search");
		assert!(!serde_json::to_string(&activity).expect("json").contains("DO_NOT_SHOW"));
	}

	#[test]
	fn compaction_and_command_failure_use_native_evidence() {
		let mut value =
			serde_json::json!({"turnId":"turn", "item":{"id":"item", "type":"contextCompaction"}});

		assert_eq!(activity::project(&value, false).expect("start").status, "running");
		assert_eq!(activity::project(&value, true).expect("end").status, "completed");

		value["item"] =
			serde_json::json!({"id":"command", "type":"commandExecution", "exitCode":1});

		assert_eq!(activity::project(&value, true).expect("failed").status, "failed");

		value["item"]["type"] = serde_json::json!("agentMessage");

		assert!(activity::project(&value, true).is_none());
	}

	#[test]
	fn search_exit_one_keeps_the_code_without_marking_activity_failed() {
		for (actions, code, expected) in [
			(serde_json::json!([{"type":"search"}]), 1, "exited"),
			(serde_json::json!([{"type":"read"},{"type":"search"}]), 1, "exited"),
			(serde_json::json!([{"type":"search"}]), 2, "failed"),
			(serde_json::json!([{"type":"read"}]), 1, "failed"),
			(serde_json::json!([]), 1, "failed"),
		] {
			let value = serde_json::json!({"turnId":"turn","item":{"id":"command","type":"commandExecution","status":"failed","exitCode":code,"commandActions":actions}});
			let activity = activity::project(&value, true).unwrap();

			assert_eq!(activity.status, expected);
			assert_eq!(activity.detail, format!("Exit code {code}"));
			assert_eq!(activity::project(&value, false).unwrap().status, "running");
		}
	}
}
