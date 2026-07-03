use serde_json::Value;

use crate::{
	mcp::{self, McpContext},
	state::StateStore,
};

pub(in crate::mcp) fn planning_mode(
	mode: Option<&str>,
	default_mode: &'static str,
	tool: &str,
) -> Result<&'static str, Value> {
	let mode = mode.map(str::trim).filter(|mode| !mode.is_empty()).unwrap_or(default_mode);

	match mode {
		"dry_run" => Ok("dry_run"),
		"apply" => Ok("apply"),
		_ => Err(mcp::invalid_tool_arguments(tool, "`mode` must be dry_run or apply.")),
	}
}

pub(in crate::mcp) fn planning_project_id(
	context: &McpContext,
	explicit_project_id: Option<&str>,
	tool: &str,
) -> Result<String, Value> {
	let project_id = explicit_project_id
		.and_then(|value| mcp::non_empty_string(Some(value)))
		.or_else(|| context.project_id())
		.ok_or_else(|| {
			mcp::tool_refusal(
				"missing_project_context",
				"Planning tools require a project-scoped MCP context or explicit projectId.",
			)
		})?;

	if mcp::safe_runtime_identifier(project_id) {
		Ok(project_id.to_owned())
	} else {
		Err(mcp::invalid_tool_arguments(
			tool,
			"`projectId` must be a safe Decodex runtime identifier.",
		))
	}
}

pub(in crate::mcp) fn planning_state_store<'a>(
	context: &'a McpContext,
	_tool: &str,
) -> Result<&'a StateStore, Value> {
	context.state_store.as_ref().ok_or_else(|| {
		mcp::tool_refusal(
			"missing_runtime_store",
			"Planning apply/readback requires the Decodex runtime store.",
		)
	})
}
