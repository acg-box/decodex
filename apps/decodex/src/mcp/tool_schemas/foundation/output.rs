mod core;
mod intake;

pub(in crate::mcp) use self::{
	core::{observe_tool_output_schema, plan_tool_output_schema},
	intake::intake_goal_tool_output_schema,
};
