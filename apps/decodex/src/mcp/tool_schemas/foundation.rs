mod input;
mod output;

pub(in crate::mcp) use self::{
	input::{intake_goal_tool_input_schema, observe_tool_input_schema, plan_tool_input_schema},
	output::{intake_goal_tool_output_schema, observe_tool_output_schema, plan_tool_output_schema},
};
