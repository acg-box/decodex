mod autonomy;
mod entry;
mod foundation;
mod operator;
mod profiles;

pub(super) use self::profiles::tool_required_profile;

use crate::mcp::McpTool;

pub(super) fn mcp_tools() -> Vec<McpTool> {
	let mut tools = foundation::mcp_foundation_tools();

	tools.extend(autonomy::mcp_autonomy_tools());
	tools.extend(operator::mcp_operator_tools());

	tools
}
