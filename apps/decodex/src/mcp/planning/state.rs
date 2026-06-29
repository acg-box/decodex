use serde_json::Value;

use crate::{
	mcp::{
		McpContext, invalid_tool_arguments, non_empty_string, safe_runtime_identifier, tool_refusal,
	},
	state::StateStore,
};

pub(super) fn planning_mode(
	mode: Option<&str>,
	default_mode: &'static str,
	tool: &str,
) -> Result<&'static str, Value> {
	let mode = mode.map(str::trim).filter(|mode| !mode.is_empty()).unwrap_or(default_mode);

	match mode {
		"dry_run" => Ok("dry_run"),
		"apply" => Ok("apply"),
		_ => Err(invalid_tool_arguments(tool, "`mode` must be dry_run or apply.")),
	}
}

pub(super) fn planning_project_id(
	context: &McpContext,
	explicit_project_id: Option<&str>,
	tool: &str,
) -> Result<String, Value> {
	let project_id = explicit_project_id
		.and_then(|value| non_empty_string(Some(value)))
		.or_else(|| context.project_id())
		.ok_or_else(|| {
			tool_refusal(
				"missing_project_context",
				"Planning tools require a project-scoped MCP context or explicit projectId.",
			)
		})?;

	if safe_runtime_identifier(project_id) {
		Ok(project_id.to_owned())
	} else {
		Err(invalid_tool_arguments(tool, "`projectId` must be a safe Decodex runtime identifier."))
	}
}

pub(super) fn planning_state_store<'a>(
	context: &'a McpContext,
	_tool: &str,
) -> Result<&'a StateStore, Value> {
	context.state_store.as_ref().ok_or_else(|| {
		tool_refusal(
			"missing_runtime_store",
			"Planning apply/readback requires the Decodex runtime store.",
		)
	})
}
