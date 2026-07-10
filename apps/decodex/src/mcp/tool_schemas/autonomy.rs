mod input;
mod output;

pub(in crate::mcp) use self::{
	input::{
		autonomy_accept_objective_tool_input_schema,
		autonomy_accept_runtime_policy_tool_input_schema,
		autonomy_apply_runtime_policy_tool_input_schema,
		autonomy_challenge_proposal_tool_input_schema, autonomy_compile_proposal_tool_input_schema,
		autonomy_draft_objective_tool_input_schema, autonomy_request_promotion_tool_input_schema,
		autonomy_submit_signal_tool_input_schema,
	},
	output::{
		autonomy_challenge_tool_output_schema, autonomy_objective_tool_output_schema,
		autonomy_promotion_request_tool_output_schema, autonomy_proposal_tool_output_schema,
		autonomy_runtime_policy_acceptance_tool_output_schema,
		autonomy_runtime_policy_apply_tool_output_schema, autonomy_signal_tool_output_schema,
	},
};
