use serde_json::{self, Value};

use crate::mcp::{McpCapabilityProfile, McpTool};

pub(in crate::mcp::tools) fn mcp_tool_entry(
	profile: McpCapabilityProfile,
	name: &str,
	title: &str,
	description: &str,
	input_schema: Value,
	output_schema: Value,
	read_only: bool,
) -> McpTool {
	McpTool {
		required_profile: profile,
		value: mcp_tool_value(
			name,
			title,
			description,
			profile,
			input_schema,
			output_schema,
			read_only,
		),
	}
}

fn mcp_tool_value(
	name: &str,
	title: &str,
	description: &str,
	profile: McpCapabilityProfile,
	input_schema: Value,
	output_schema: Value,
	read_only: bool,
) -> Value {
	serde_json::json!({
		"name": name,
		"title": title,
		"description": description,
		"inputSchema": input_schema,
		"outputSchema": output_schema,
		"annotations": {
			"readOnlyHint": read_only,
			"destructiveHint": false,
			"idempotentHint": read_only,
			"openWorldHint": false
		},
		"_meta": {
			"decodex/capabilityProfile": profile.as_str()
		}
	})
}
