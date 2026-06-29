use serde_json::Value;

use super::{
	McpCapabilityProfile, McpTool, TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
	TOOL_AUTONOMY_CHALLENGE_PROPOSAL, TOOL_AUTONOMY_COMPILE_PROPOSAL,
	TOOL_AUTONOMY_DRAFT_OBJECTIVE, TOOL_AUTONOMY_REQUEST_PROMOTION, TOOL_AUTONOMY_SUBMIT_SIGNAL,
	TOOL_INTAKE_GOAL, TOOL_LANE_CONTROL, TOOL_OBSERVE, TOOL_PLAN, TOOL_PROJECT_CONTROL,
	TOOL_RESEARCH_COMPILE, TOOL_RESEARCH_PROMOTE,
	tool_schemas::{
		autonomy_accept_objective_tool_input_schema, autonomy_challenge_proposal_tool_input_schema,
		autonomy_challenge_tool_output_schema, autonomy_compile_proposal_tool_input_schema,
		autonomy_draft_objective_tool_input_schema, autonomy_objective_tool_output_schema,
		autonomy_promotion_request_tool_output_schema, autonomy_proposal_tool_output_schema,
		autonomy_request_promotion_tool_input_schema, autonomy_signal_tool_output_schema,
		autonomy_submit_signal_tool_input_schema, intake_goal_tool_input_schema,
		intake_goal_tool_output_schema, lane_control_tool_input_schema,
		lane_control_tool_output_schema, observe_tool_input_schema, observe_tool_output_schema,
		plan_tool_input_schema, plan_tool_output_schema, project_control_tool_input_schema,
		project_control_tool_output_schema, research_compile_tool_input_schema,
		research_compile_tool_output_schema, research_promote_tool_input_schema,
		research_promote_tool_output_schema,
	},
};

pub(super) fn mcp_tools() -> Vec<McpTool> {
	let mut tools = mcp_foundation_tools();

	tools.extend(mcp_autonomy_tools());
	tools.extend(mcp_operator_tools());

	tools
}

fn mcp_foundation_tools() -> Vec<McpTool> {
	vec![
		mcp_tool_entry(
			McpCapabilityProfile::Observe,
			TOOL_OBSERVE,
			"Decodex Observe",
			"Read public-safe local Decodex runtime observability without private evidence payloads.",
			observe_tool_input_schema(),
			observe_tool_output_schema(),
			true,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_PLAN,
			"Decodex Plan",
			"Return the Decodex prompt/resource route for a requested workflow intent.",
			plan_tool_input_schema(),
			plan_tool_output_schema(),
			true,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_RESEARCH_COMPILE,
			"Decodex Research Compile",
			"Validate or persist a latent Decodex Decision Contract from bounded research input.",
			research_compile_tool_input_schema(),
			research_compile_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_RESEARCH_PROMOTE,
			"Decodex Research Promote",
			"Inspect or explicitly promote a latent Decision Contract through Decodex authority checks.",
			research_promote_tool_input_schema(),
			research_promote_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_INTAKE_GOAL,
			"Decodex Goal Intake",
			"Dry-run or explicitly apply promoted-goal Program Intake through Decodex authority gates.",
			intake_goal_tool_input_schema(),
			intake_goal_tool_output_schema(),
			false,
		),
	]
}

fn mcp_autonomy_tools() -> Vec<McpTool> {
	vec![
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_DRAFT_OBJECTIVE,
			"Decodex Autonomy Draft Objective",
			"Validate or persist a draft Objective Contract without granting acceptance authority.",
			autonomy_draft_objective_tool_input_schema(),
			autonomy_objective_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
			"Decodex Autonomy Accept Objective",
			"Accept a draft Objective Contract version as project-level autonomy authority without starting execution.",
			autonomy_accept_objective_tool_input_schema(),
			autonomy_objective_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_SUBMIT_SIGNAL,
			"Decodex Autonomy Submit Signal",
			"Validate or persist proposal-only autonomy signal evidence under an accepted objective.",
			autonomy_submit_signal_tool_input_schema(),
			autonomy_signal_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_COMPILE_PROPOSAL,
			"Decodex Autonomy Compile Proposal",
			"Compile or persist non-executable autonomy proposal evidence from accepted objective-bound signals.",
			autonomy_compile_proposal_tool_input_schema(),
			autonomy_proposal_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
			"Decodex Autonomy Challenge Proposal",
			"Dry-run or record challenge evidence for an autonomy proposal without making it acceptance authority.",
			autonomy_challenge_proposal_tool_input_schema(),
			autonomy_challenge_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_REQUEST_PROMOTION,
			"Decodex Autonomy Request Promotion",
			"Inspect or explicitly accept an autonomy proposal into a latent Decision Contract candidate.",
			autonomy_request_promotion_tool_input_schema(),
			autonomy_promotion_request_tool_output_schema(),
			false,
		),
	]
}

fn mcp_operator_tools() -> Vec<McpTool> {
	vec![
		mcp_tool_entry(
			McpCapabilityProfile::Operate,
			TOOL_LANE_CONTROL,
			"Decodex Lane Control",
			"Inspect a lane or request guarded soft lane-control actions with explicit authority.",
			lane_control_tool_input_schema(),
			lane_control_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Admin,
			TOOL_PROJECT_CONTROL,
			"Decodex Project Control",
			"Pause or resume future project dispatch through the registered project enablement guard.",
			project_control_tool_input_schema(),
			project_control_tool_output_schema(),
			false,
		),
	]
}

fn mcp_tool_entry(
	profile: McpCapabilityProfile,
	name: &str,
	title: &str,
	description: &str,
	input_schema: Value,
	output_schema: Value,
	read_only: bool,
) -> McpTool {
	McpTool {
		required_profile: profile,
		value: mcp_tool_value(
			name,
			title,
			description,
			profile,
			input_schema,
			output_schema,
			read_only,
		),
	}
}

fn mcp_tool_value(
	name: &str,
	title: &str,
	description: &str,
	profile: McpCapabilityProfile,
	input_schema: Value,
	output_schema: Value,
	read_only: bool,
) -> Value {
	serde_json::json!({
		"name": name,
		"title": title,
		"description": description,
		"inputSchema": input_schema,
		"outputSchema": output_schema,
		"annotations": {
			"readOnlyHint": read_only,
			"destructiveHint": false,
			"idempotentHint": read_only,
			"openWorldHint": false
		},
		"_meta": {
			"decodex/capabilityProfile": profile.as_str()
		}
	})
}

pub(super) fn tool_required_profile(name: &str) -> Option<McpCapabilityProfile> {
	match name {
		TOOL_OBSERVE => Some(McpCapabilityProfile::Observe),
		TOOL_PLAN => Some(McpCapabilityProfile::Plan),
		TOOL_RESEARCH_COMPILE
		| TOOL_RESEARCH_PROMOTE
		| TOOL_INTAKE_GOAL
		| TOOL_AUTONOMY_DRAFT_OBJECTIVE
		| TOOL_AUTONOMY_ACCEPT_OBJECTIVE
		| TOOL_AUTONOMY_SUBMIT_SIGNAL
		| TOOL_AUTONOMY_COMPILE_PROPOSAL
		| TOOL_AUTONOMY_CHALLENGE_PROPOSAL
		| TOOL_AUTONOMY_REQUEST_PROMOTION => Some(McpCapabilityProfile::Plan),
		TOOL_LANE_CONTROL => Some(McpCapabilityProfile::Operate),
		TOOL_PROJECT_CONTROL => Some(McpCapabilityProfile::Admin),
		_ => None,
	}
}
