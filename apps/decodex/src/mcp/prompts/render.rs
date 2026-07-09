use serde_json::Value;

use crate::mcp::prompts::arguments;

pub(super) fn mcp_prompt_result(name: &str, arguments: Value) -> Option<Value> {
	let text = match name {
		"decodex_validation_ready" => format!(
			"Work only to Decodex validation-ready state for issue {}. Implement the smallest coherent code and OpenWiki change, run targeted validation, record OpenWiki impact in a current-HEAD `issue_progress_checkpoint` using `openwiki_impact`, then complete the active phase goal without push or PR handoff.\n\nPhase: {}",
			arguments::prompt_argument(&arguments, "issue")?,
			arguments::prompt_argument(&arguments, "phase")
				.unwrap_or("implement_to_validation_ready")
		),
		"decodex_handoff" => format!(
			"Before handoff for issue {}, re-read the current diff and HEAD, run the repo-native bounded review method, require a clean current-head review checkpoint, then use the normal PR-backed Decodex handoff path.",
			arguments::prompt_argument(&arguments, "issue")?
		),
		"decodex_lane_control" => format!(
			"Inspect issue {} first. Mutating lane-control tool calls must include the observed run id, current turn preconditions when steering, and explicit authority fields; refuse instead of guessing missing authority.",
			arguments::prompt_argument(&arguments, "issue")?
		),
		_ => return None,
	};

	Some(serde_json::json!({
		"description": prompt_description(name),
		"messages": [
			{
				"role": "user",
				"content": {
					"type": "text",
					"text": text
				}
			}
		]
	}))
}

fn prompt_description(name: &str) -> &'static str {
	match name {
		"decodex_validation_ready" => "Decodex implementation-phase validation-ready prompt.",
		"decodex_handoff" => "Decodex verified review-handoff prompt.",
		"decodex_lane_control" => "Decodex inspect-first lane-control prompt.",
		_ => "Decodex prompt.",
	}
}
