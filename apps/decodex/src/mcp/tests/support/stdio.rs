use std::{io::Cursor, path::Path};

use serde_json::Value;

use crate::mcp::{McpCapabilityProfile, McpContext, server};

pub(in crate::mcp::tests) fn run_stdio(repo_root: &Path, input: &str) -> Vec<Value> {
	run_stdio_raw(repo_root, input)
		.lines()
		.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
		.collect()
}

pub(in crate::mcp::tests) fn run_stdio_with_context(
	context: McpContext,
	input: &str,
) -> Vec<Value> {
	run_stdio_raw_with_context(context, input)
		.lines()
		.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
		.collect()
}

pub(in crate::mcp::tests) fn run_stdio_with_profile(
	_repo_root: &Path,
	capability_profile: McpCapabilityProfile,
	input: &str,
) -> Vec<Value> {
	let context = McpContext { config_path: None, project_id: None, state_store: None };

	run_stdio_raw_with_profile(context, capability_profile, input)
		.lines()
		.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
		.collect()
}

pub(in crate::mcp::tests) fn project_mcp_context(
	_repo_root: &Path,
	config_path: &Path,
) -> McpContext {
	McpContext {
		config_path: Some(config_path.to_path_buf()),
		project_id: Some(String::from("pubfi")),
		state_store: None,
	}
}

pub(in crate::mcp::tests) fn run_stdio_raw(_repo_root: &Path, input: &str) -> String {
	let context = McpContext { config_path: None, project_id: None, state_store: None };

	run_stdio_raw_with_context(context, input)
}

pub(in crate::mcp::tests) fn run_stdio_raw_with_context(
	context: McpContext,
	input: &str,
) -> String {
	run_stdio_raw_with_profile(context, McpCapabilityProfile::Admin, input)
}

pub(in crate::mcp::tests) fn run_stdio_raw_with_profile(
	context: McpContext,
	capability_profile: McpCapabilityProfile,
	input: &str,
) -> String {
	let mut output = Vec::new();

	server::serve_stdio_with_profile(
		Cursor::new(format!("{input}\n")),
		&mut output,
		context,
		capability_profile,
	)
	.expect("stdio server should run");

	String::from_utf8(output).expect("stdout should be utf-8")
}

pub(in crate::mcp::tests) fn response_at(responses: &[Value], index: usize) -> &Value {
	responses.get(index).expect("response should exist")
}

pub(in crate::mcp::tests) fn response_error(responses: &[Value], index: usize) -> &Value {
	response_at(responses, index).get("error").expect("error response")
}

pub(in crate::mcp::tests) fn resource_response_json(responses: &[Value], index: usize) -> Value {
	let contents = response_at(responses, index)["result"]["contents"]
		.as_array()
		.expect("resource contents array");
	let text = contents[0]["text"].as_str().expect("resource text should exist");

	serde_json::from_str(text).expect("resource text should be JSON")
}

pub(in crate::mcp::tests) fn assert_tool_output_schema_variant(
	tool: &Value,
	schema: &str,
	required_field: Option<&str>,
) {
	let variants = tool["outputSchema"]["oneOf"].as_array().expect("oneOf variants");
	let variant = variants
		.iter()
		.find(|variant| {
			variant["properties"]["schema"]["enum"]
				.as_array()
				.expect("schema enum")
				.iter()
				.any(|value| value.as_str() == Some(schema))
		})
		.expect("schema variant should exist");

	if let Some(required_field) = required_field {
		assert!(
			variant["required"]
				.as_array()
				.expect("required array")
				.iter()
				.any(|value| value.as_str() == Some(required_field))
		);
	}
}
