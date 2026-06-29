use serde::Deserialize;
use serde_json::{self, Value};

use super::{McpError, McpServer};

#[derive(Deserialize)]
struct GetPromptParams {
	name: String,
	arguments: Option<Value>,
}

impl McpServer {
	pub(super) fn list_prompts(&self) -> Value {
		serde_json::json!({ "prompts": mcp_prompts() })
	}

	pub(super) fn get_prompt(
		&self,
		params: Option<Value>,
	) -> crate::prelude::Result<Value, McpError> {
		let params = params.ok_or_else(McpError::invalid_params)?;
		let params = serde_json::from_value::<GetPromptParams>(params)
			.map_err(|_| McpError::invalid_params())?;
		let arguments = params.arguments.unwrap_or_default();

		if !prompt_required_arguments_are_present(&params.name, &arguments) {
			return Err(McpError::invalid_params());
		}

		mcp_prompt_result(&params.name, arguments).ok_or_else(McpError::invalid_params)
	}
}

fn mcp_prompts() -> Vec<Value> {
	vec![
		serde_json::json!({
			"name": "decodex_research",
			"title": "Decodex Research",
			"description": "Frame bounded Decodex research as a latent Decision Contract candidate.",
			"arguments": [
				{
					"name": "intent",
					"description": "Natural-language research question or design uncertainty.",
					"required": true
				}
			]
		}),
		serde_json::json!({
			"name": "decodex_validation_ready",
			"title": "Decodex Validation Ready",
			"description": "Drive an implementation or repair lane to local validation-ready evidence.",
			"arguments": [
				{
					"name": "issue",
					"description": "Linear issue identifier for the lane.",
					"required": true
				},
				{
					"name": "phase",
					"description": "Current Decodex phase goal.",
					"required": false
				}
			]
		}),
		serde_json::json!({
			"name": "decodex_handoff",
			"title": "Decodex Handoff",
			"description": "Prepare a verified review handoff only after local validation and bounded review.",
			"arguments": [
				{
					"name": "issue",
					"description": "Linear issue identifier for the lane.",
					"required": true
				}
			]
		}),
		serde_json::json!({
			"name": "decodex_lane_control",
			"title": "Decodex Lane Control",
			"description": "Inspect first, then request guarded lane-control actions through existing Decodex authority gates.",
			"arguments": [
				{
					"name": "issue",
					"description": "Linear issue identifier or local tracker issue id.",
					"required": true
				},
				{
					"name": "runId",
					"description": "Current run id observed through lane inspect.",
					"required": false
				}
			]
		}),
	]
}

fn mcp_prompt_result(name: &str, arguments: Value) -> Option<Value> {
	let text = match name {
		"decodex_research" => format!(
			"Use Decodex research routing for this intent, keep the result latent until explicitly promoted, and preserve evidence, options, judgment, challenge, decision, validation expectations, and stop conditions.\n\nIntent: {}",
			prompt_argument(&arguments, "intent")?
		),
		"decodex_validation_ready" => format!(
			"Work only to Decodex validation-ready state for issue {}. Implement the smallest coherent code and docs change, run targeted validation, record a current-HEAD docs-impact checkpoint, then complete the active phase goal without push or PR handoff.\n\nPhase: {}",
			prompt_argument(&arguments, "issue")?,
			prompt_argument(&arguments, "phase").unwrap_or("implement_to_validation_ready")
		),
		"decodex_handoff" => format!(
			"Before handoff for issue {}, re-read the current diff and HEAD, run the repo-native bounded review method, require a clean current-head review checkpoint, then use the normal PR-backed Decodex handoff path.",
			prompt_argument(&arguments, "issue")?
		),
		"decodex_lane_control" => format!(
			"Inspect issue {} first. Mutating lane-control tool calls must include the observed run id, current turn preconditions when steering, and explicit authority fields; refuse instead of guessing missing authority.",
			prompt_argument(&arguments, "issue")?
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
		"decodex_research" => "Contract-first bounded Decodex research prompt.",
		"decodex_validation_ready" => "Decodex implementation-phase validation-ready prompt.",
		"decodex_handoff" => "Decodex verified review-handoff prompt.",
		"decodex_lane_control" => "Decodex inspect-first lane-control prompt.",
		_ => "Decodex prompt.",
	}
}

fn prompt_argument<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
	arguments.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
}

fn prompt_required_arguments_are_present(name: &str, arguments: &Value) -> bool {
	let required: &[&str] = match name {
		"decodex_research" => &["intent"],
		"decodex_validation_ready" | "decodex_handoff" | "decodex_lane_control" => &["issue"],
		_ => return true,
	};

	required.iter().all(|key| prompt_argument(arguments, key).is_some())
}
