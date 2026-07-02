use std::{fs, path::Path};

use serde_json::{self, Value};

use crate::prelude::{Result, eyre};

pub(in crate::agent::app_server::schema_probe) fn validate_generated_dynamic_tool_schema(
	out_dir: &Path,
) -> Result<()> {
	let mut found_thread_start_schema = false;
	let mut found_supported_dynamic_tool_schema = false;

	collect_dynamic_tool_schema_state(
		out_dir,
		&mut found_thread_start_schema,
		&mut found_supported_dynamic_tool_schema,
	)?;

	if !found_thread_start_schema {
		eyre::bail!(
			"Generated app-server schema was missing ThreadStartParams dynamicTools schema."
		);
	}
	if !found_supported_dynamic_tool_schema {
		eyre::bail!(
			"Generated app-server schema does not expose the Decodex-supported 0.141 dynamicTools tagged union."
		);
	}

	Ok(())
}

fn collect_dynamic_tool_schema_state(
	path: &Path,
	found_thread_start_schema: &mut bool,
	found_supported_dynamic_tool_schema: &mut bool,
) -> Result<()> {
	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let path = entry.path();

		if path.is_dir() {
			collect_dynamic_tool_schema_state(
				&path,
				found_thread_start_schema,
				found_supported_dynamic_tool_schema,
			)?;
		} else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
			let schema = fs::read_to_string(&path)?;
			let value: Value = serde_json::from_str(&schema)?;

			if value.get("title").and_then(Value::as_str) == Some("ThreadStartParams")
				&& value.pointer("/properties/dynamicTools").is_some()
			{
				*found_thread_start_schema = true;
				*found_supported_dynamic_tool_schema |=
					thread_start_schema_has_app_server_dynamic_tool_union(&value);
			}
		}
	}

	Ok(())
}

fn thread_start_schema_has_app_server_dynamic_tool_union(schema: &Value) -> bool {
	let Some(definitions) = schema.get("definitions").and_then(Value::as_object) else {
		return false;
	};
	let Some(dynamic_tool_spec) = definitions.get("DynamicToolSpec") else {
		return false;
	};
	let Some(namespace_tool) = definitions.get("DynamicToolNamespaceTool") else {
		return false;
	};
	let Some(function_branch) =
		one_of_branch_with_title(dynamic_tool_spec, "FunctionDynamicToolSpec")
	else {
		return false;
	};
	let Some(namespace_branch) =
		one_of_branch_with_title(dynamic_tool_spec, "NamespaceDynamicToolSpec")
	else {
		return false;
	};
	let Some(namespace_tool_branch) =
		one_of_branch_with_title(namespace_tool, "FunctionDynamicToolNamespaceTool")
	else {
		return false;
	};

	schema_required_contains_all(function_branch, &["description", "inputSchema", "name", "type"])
		&& schema_type_enum_contains(function_branch, "function")
		&& schema_required_contains_all(namespace_branch, &["description", "name", "tools", "type"])
		&& schema_type_enum_contains(namespace_branch, "namespace")
		&& schema_required_contains_all(
			namespace_tool_branch,
			&["description", "inputSchema", "name", "type"],
		) && schema_type_enum_contains(namespace_tool_branch, "function")
}

fn one_of_branch_with_title<'a>(schema: &'a Value, title: &str) -> Option<&'a Value> {
	schema
		.get("oneOf")?
		.as_array()?
		.iter()
		.find(|branch| branch.get("title").and_then(Value::as_str) == Some(title))
}

fn schema_required_contains_all(schema: &Value, names: &[&str]) -> bool {
	let Some(required) = schema.get("required").and_then(Value::as_array) else {
		return false;
	};

	names.iter().all(|name| required.iter().any(|value| value.as_str() == Some(*name)))
}

fn schema_type_enum_contains(schema: &Value, type_value: &str) -> bool {
	let Some(enum_values) = schema.pointer("/properties/type/enum").and_then(Value::as_array)
	else {
		return false;
	};

	enum_values.iter().any(|value| value.as_str() == Some(type_value))
}
