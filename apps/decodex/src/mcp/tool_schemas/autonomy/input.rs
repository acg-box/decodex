mod objective;
mod proposal;
mod runtime_policy;
mod signal;

pub(in crate::mcp) use self::{
	objective::{
		autonomy_accept_objective_tool_input_schema, autonomy_draft_objective_tool_input_schema,
	},
	proposal::{
		autonomy_challenge_proposal_tool_input_schema, autonomy_compile_proposal_tool_input_schema,
		autonomy_request_promotion_tool_input_schema,
	},
	runtime_policy::{
		autonomy_accept_runtime_policy_tool_input_schema,
		autonomy_apply_runtime_policy_tool_input_schema,
	},
	signal::autonomy_submit_signal_tool_input_schema,
};
