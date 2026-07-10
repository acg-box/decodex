mod objective;
mod proposal;
mod runtime_policy;
mod signal;

pub(in crate::mcp) use self::{
	objective::autonomy_objective_tool_output_schema,
	proposal::{
		autonomy_challenge_tool_output_schema, autonomy_promotion_request_tool_output_schema,
		autonomy_proposal_tool_output_schema,
	},
	runtime_policy::{
		autonomy_runtime_policy_acceptance_tool_output_schema,
		autonomy_runtime_policy_apply_tool_output_schema,
	},
	signal::autonomy_signal_tool_output_schema,
};
