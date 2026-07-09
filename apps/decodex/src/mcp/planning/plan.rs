use serde::Deserialize;
use serde_json::{self, Value};

use crate::mcp::{self, TOOL_PLAN};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanToolArgs {
	intent: String,
	issue: Option<String>,
	contract_id: Option<String>,
}

pub(in crate::mcp) fn call_plan_tool(arguments: Value) -> Value {
	let params = match serde_json::from_value::<PlanToolArgs>(arguments) {
		Ok(params) => params,
		Err(_) => {
			return mcp::invalid_tool_arguments(
				TOOL_PLAN,
				"`intent` is required and must be one of validation_ready, handoff, lane_control, or intake_goal.",
			);
		},
	};

	if !matches!(
		params.intent.as_str(),
		"validation_ready" | "handoff" | "lane_control" | "intake_goal"
	) {
		return mcp::invalid_tool_arguments(
			TOOL_PLAN,
			"`intent` must be one of validation_ready, handoff, lane_control, or intake_goal.",
		);
	}

	mcp::tool_success(plan_tool_result(&params))
}

fn plan_tool_result(params: &PlanToolArgs) -> Value {
	let (prompt, resource_hint, next_action) = match params.intent.as_str() {
		"intake_goal" => (
			"decodex_validation_ready",
			"decodex://openwiki/operations/commands-and-validation",
			"Use intake_goal dry_run first, then apply only with explicit accepted Decision Contract authority.",
		),
		"handoff" => (
			"decodex_handoff",
			"decodex://openwiki/workflows/runtime-operator-workflows",
			"Run bounded review and repo validation before PR-backed handoff.",
		),
		"lane_control" => (
			"decodex_lane_control",
			"decodex://openwiki/workflows/runtime-operator-workflows",
			"Inspect first; then call guarded MCP lane-control with explicit authority and current run/turn preconditions.",
		),
		_ => (
			"decodex_validation_ready",
			"decodex://openwiki/operations/commands-and-validation",
			"Implement locally, run targeted validation, record OpenWiki impact, and complete the phase goal.",
		),
	};

	serde_json::json!({
		"schema": "decodex.mcp.plan_result/1",
		"status": "ok",
		"intent": params.intent.as_str(),
		"prompt": prompt,
		"resource": resource_hint,
		"next_action": next_action,
		"issue": params.issue.as_deref(),
		"contract_id": params.contract_id.as_deref()
	})
}
