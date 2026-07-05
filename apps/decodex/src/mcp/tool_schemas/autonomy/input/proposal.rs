mod challenge;
mod compile;
mod promotion;
mod schema;

pub(in crate::mcp) use self::{
	challenge::autonomy_challenge_proposal_tool_input_schema,
	compile::autonomy_compile_proposal_tool_input_schema,
	promotion::autonomy_request_promotion_tool_input_schema,
};
