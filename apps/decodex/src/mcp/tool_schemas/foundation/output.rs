mod core;
mod intake;
mod research;

pub(in crate::mcp) use self::{
	core::{observe_tool_output_schema, plan_tool_output_schema},
	intake::intake_goal_tool_output_schema,
	research::{research_compile_tool_output_schema, research_promote_tool_output_schema},
};
