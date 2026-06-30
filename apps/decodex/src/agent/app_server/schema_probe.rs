//! App-server generated schema compatibility probe.

use std::{
	collections::BTreeMap,
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use serde_json::{self, Value};

use crate::{
	agent::json_rpc::{self, AppServerProcessEnv},
	prelude::eyre,
};

const APP_SERVER_SCHEMA_GENERATE_COMMAND: &str =
	"codex app-server generate-json-schema --experimental";
const APP_SERVER_SCHEMA_PROBE_OUT_DIR: &str = "target/decodex-app-server-schema-check";
pub(super) const APP_SERVER_SCHEMA_REQUIRED_MARKERS: &[&str] = &[
	"initialize",
	"config/read",
	"model/list",
	"modelProvider/capabilities/read",
	"skills/list",
	"plugin/list",
	"mcpServerStatus/list",
	"thread/start",
	"thread/resume",
	"thread/goal/set",
	"thread/goal/get",
	"thread/goal/clear",
	"thread/goal/updated",
	"turn/start",
	"thread/archive",
	"command/exec",
	"item/tool/call",
	"thread/status/changed",
	"turn/completed",
	"dynamicTools",
	"function",
	"namespace",
	"tools",
	"type",
	"deferLoading",
	"inputText",
	"marketplaceKinds",
];
pub(super) const APP_SERVER_REQUIRED_CLIENT_REQUESTS: &[(&str, &str)] = &[
	("initialize", "InitializeParams"),
	("account/login/start", "LoginAccountParams"),
	("thread/start", "ThreadStartParams"),
	("thread/resume", "ThreadResumeParams"),
	("thread/archive", "ThreadArchiveParams"),
	("thread/goal/set", "ThreadGoalSetParams"),
	("thread/goal/get", "ThreadGoalGetParams"),
	("thread/goal/clear", "ThreadGoalClearParams"),
	("turn/start", "TurnStartParams"),
	("turn/interrupt", "TurnInterruptParams"),
	("turn/steer", "TurnSteerParams"),
	("command/exec", "CommandExecParams"),
	("config/read", "ConfigReadParams"),
	("model/list", "ModelListParams"),
	("modelProvider/capabilities/read", "ModelProviderCapabilitiesReadParams"),
	("skills/list", "SkillsListParams"),
	("plugin/list", "PluginListParams"),
	("mcpServerStatus/list", "ListMcpServerStatusParams"),
];
pub(super) const APP_SERVER_REQUIRED_SERVER_REQUESTS: &[(&str, &str)] = &[
	("item/commandExecution/requestApproval", "CommandExecutionRequestApprovalParams"),
	("item/fileChange/requestApproval", "FileChangeRequestApprovalParams"),
	("item/tool/requestUserInput", "ToolRequestUserInputParams"),
	("mcpServer/elicitation/request", "McpServerElicitationRequestParams"),
	("item/permissions/requestApproval", "PermissionsRequestApprovalParams"),
	("item/tool/call", "DynamicToolCallParams"),
	("account/chatgptAuthTokens/refresh", "ChatgptAuthTokensRefreshParams"),
];
pub(super) const APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS: &[(&str, &str)] = &[("initialized", "")];
pub(super) const APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS: &[(&str, &str)] = &[
	("error", "ErrorNotification"),
	("thread/started", "ThreadStartedNotification"),
	("thread/status/changed", "ThreadStatusChangedNotification"),
	("thread/archived", "ThreadArchivedNotification"),
	("thread/goal/updated", "ThreadGoalUpdatedNotification"),
	("thread/goal/cleared", "ThreadGoalClearedNotification"),
	("thread/tokenUsage/updated", "ThreadTokenUsageUpdatedNotification"),
	("turn/started", "TurnStartedNotification"),
	("turn/completed", "TurnCompletedNotification"),
	("item/started", "ItemStartedNotification"),
	("item/completed", "ItemCompletedNotification"),
	("item/agentMessage/delta", "AgentMessageDeltaNotification"),
	("account/updated", "AccountUpdatedNotification"),
	("account/rateLimits/updated", "AccountRateLimitsUpdatedNotification"),
	("model/rerouted", "ModelReroutedNotification"),
	("model/verification", "ModelVerificationNotification"),
];
const APP_SERVER_SCHEMA_PROSE_KEYS: &[&str] =
	&["$comment", "comment", "description", "examples", "markdownDescription", "title"];
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AppServerSchemaProbeEvidence {
	cache_path: String,
	marker_count: usize,
	required_markers: Vec<&'static str>,
}
impl AppServerSchemaProbeEvidence {
	fn checked(cache_path: String, required_markers: &[&'static str]) -> Self {
		Self {
			cache_path,
			marker_count: required_markers.len(),
			required_markers: required_markers.to_vec(),
		}
	}
}

pub(super) fn probe_app_server_schema(
	process_env: &AppServerProcessEnv,
) -> crate::prelude::Result<AppServerSchemaProbeEvidence> {
	let out_dir = PathBuf::from(APP_SERVER_SCHEMA_PROBE_OUT_DIR);

	if out_dir.exists() {
		fs::remove_dir_all(&out_dir)?;
	}

	if let Some(parent) = out_dir.parent() {
		fs::create_dir_all(parent)?;
	}

	let mut command = Command::new(json_rpc::app_server_command_program());

	command.args(["app-server", "generate-json-schema", "--experimental", "--out"]);
	command.arg(&out_dir);
	process_env.apply_to(&mut command)?;

	let output = command.output()?;

	if !output.status.success() {
		eyre::bail!(
			"`{APP_SERVER_SCHEMA_GENERATE_COMMAND}` failed with status {}: stdout={} stderr={}",
			output.status,
			command_output_excerpt(&output.stdout),
			command_output_excerpt(&output.stderr)
		);
	}

	validate_generated_app_server_schema(&out_dir)?;

	Ok(AppServerSchemaProbeEvidence::checked(
		APP_SERVER_SCHEMA_PROBE_OUT_DIR.to_owned(),
		APP_SERVER_SCHEMA_REQUIRED_MARKERS,
	))
}

pub(super) fn validate_generated_app_server_schema(out_dir: &Path) -> crate::prelude::Result<()> {
	let mut marker_presence = APP_SERVER_SCHEMA_REQUIRED_MARKERS
		.iter()
		.map(|marker| (*marker, false))
		.collect::<BTreeMap<_, _>>();
	let schema_file_count = collect_schema_markers(out_dir, &mut marker_presence)?;

	if schema_file_count == 0 {
		eyre::bail!(
			"Generated app-server schema directory `{}` contained no JSON files.",
			out_dir.display()
		);
	}

	let missing_markers = marker_presence
		.iter()
		.filter_map(|(marker, present)| (!*present).then_some(*marker))
		.collect::<Vec<_>>();

	if !missing_markers.is_empty() {
		eyre::bail!(
			"Generated app-server schema was missing required Decodex markers: {}",
			missing_markers.join(", ")
		);
	}

	validate_generated_dynamic_tool_schema(out_dir)?;
	validate_generated_app_server_method_unions(out_dir)?;

	Ok(())
}

fn validate_generated_app_server_method_unions(out_dir: &Path) -> crate::prelude::Result<()> {
	validate_generated_method_union(out_dir, "ClientRequest", APP_SERVER_REQUIRED_CLIENT_REQUESTS)?;
	validate_generated_method_union(out_dir, "ServerRequest", APP_SERVER_REQUIRED_SERVER_REQUESTS)?;
	validate_generated_method_union(
		out_dir,
		"ClientNotification",
		APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS,
	)?;
	validate_generated_method_union(
		out_dir,
		"ServerNotification",
		APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS,
	)?;

	Ok(())
}

fn validate_generated_method_union(
	out_dir: &Path,
	title: &'static str,
	required_methods: &[(&'static str, &'static str)],
) -> crate::prelude::Result<()> {
	let Some(schema) = find_schema_by_title(out_dir, title)? else {
		eyre::bail!("Generated app-server schema was missing `{title}` method union.");
	};
	let method_refs = method_schema_refs(&schema);
	let missing_or_mismatched = required_methods
		.iter()
		.filter_map(|(method, expected_ref)| match method_refs.get(*method) {
			Some(actual_ref) if actual_ref.as_deref() == expected_ref_to_option(expected_ref) => {
				None
			},
			Some(actual_ref) => Some(format!(
				"{method} expected {} got {}",
				expected_ref_display(expected_ref),
				actual_ref.as_deref().unwrap_or("<no params>")
			)),
			None => Some(format!("{method} missing")),
		})
		.collect::<Vec<_>>();

	if !missing_or_mismatched.is_empty() {
		eyre::bail!(
			"Generated app-server `{title}` schema was missing or changed Decodex-owned methods: {}",
			missing_or_mismatched.join(", ")
		);
	}

	Ok(())
}

fn find_schema_by_title(out_dir: &Path, title: &str) -> crate::prelude::Result<Option<Value>> {
	let direct_path = out_dir.join(format!("{title}.json"));

	if direct_path.is_file() {
		let schema = fs::read_to_string(&direct_path)?;

		return Ok(Some(serde_json::from_str(&schema)?));
	}

	let mut schema = None;

	collect_schema_by_title(out_dir, title, &mut schema)?;

	Ok(schema)
}

fn collect_schema_by_title(
	path: &Path,
	title: &str,
	matching_schema: &mut Option<Value>,
) -> crate::prelude::Result<()> {
	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let path = entry.path();

		if path.is_dir() {
			collect_schema_by_title(&path, title, matching_schema)?;
		} else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
			let schema = fs::read_to_string(&path)?;
			let value: Value = serde_json::from_str(&schema)?;

			if value.get("title").and_then(Value::as_str) == Some(title) {
				*matching_schema = Some(value);
			}
		}
	}

	Ok(())
}

fn method_schema_refs(schema: &Value) -> BTreeMap<String, Option<String>> {
	let Some(branches) = schema.get("oneOf").and_then(Value::as_array) else {
		return BTreeMap::new();
	};

	branches
		.iter()
		.filter_map(|branch| {
			let properties = branch.get("properties")?.as_object()?;
			let method =
				properties.get("method")?.get("enum")?.as_array()?.first()?.as_str()?.to_owned();
			let params_ref = properties
				.get("params")
				.and_then(|params| params.get("$ref"))
				.and_then(Value::as_str)
				.map(schema_ref_title);

			Some((method, params_ref))
		})
		.collect()
}

fn schema_ref_title(schema_ref: &str) -> String {
	schema_ref.rsplit('/').next().unwrap_or(schema_ref).to_owned()
}

fn expected_ref_to_option(expected_ref: &str) -> Option<&str> {
	(!expected_ref.is_empty()).then_some(expected_ref)
}

fn expected_ref_display(expected_ref: &str) -> &str {
	expected_ref_to_option(expected_ref).unwrap_or("<no params>")
}

fn validate_generated_dynamic_tool_schema(out_dir: &Path) -> crate::prelude::Result<()> {
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
) -> crate::prelude::Result<()> {
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

fn collect_schema_markers(
	path: &Path,
	marker_presence: &mut BTreeMap<&'static str, bool>,
) -> crate::prelude::Result<usize> {
	let mut json_file_count = 0;

	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let path = entry.path();

		if path.is_dir() {
			json_file_count += collect_schema_markers(&path, marker_presence)?;
		} else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
			let schema = fs::read_to_string(&path)?;
			let value: Value = serde_json::from_str(&schema)?;

			json_file_count += 1;

			record_schema_markers_from_value(&value, marker_presence);
		}
	}

	Ok(json_file_count)
}

fn record_schema_markers_from_value(
	value: &Value,
	marker_presence: &mut BTreeMap<&'static str, bool>,
) {
	match value {
		Value::Object(object) => {
			for (key, value) in object {
				if schema_prose_key(key) {
					continue;
				}

				record_schema_marker_from_text(key, marker_presence);
				record_schema_markers_from_value(value, marker_presence);
			}
		},
		Value::Array(values) => {
			for value in values {
				record_schema_markers_from_value(value, marker_presence);
			}
		},
		Value::String(value) => record_schema_marker_from_text(value, marker_presence),
		Value::Null | Value::Bool(_) | Value::Number(_) => {},
	}
}

fn schema_prose_key(key: &str) -> bool {
	APP_SERVER_SCHEMA_PROSE_KEYS.contains(&key)
}

fn record_schema_marker_from_text(value: &str, marker_presence: &mut BTreeMap<&'static str, bool>) {
	for (marker, present) in marker_presence {
		if value.contains(*marker) {
			*present = true;
		}
	}
}

fn command_output_excerpt(output: &[u8]) -> String {
	let text = String::from_utf8_lossy(output);
	let trimmed = text.trim();
	let excerpt = trimmed.chars().take(1_000).collect::<String>();

	if excerpt.is_empty() { String::from("<empty>") } else { excerpt }
}
