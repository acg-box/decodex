use crate::agent::app_server::preflight::{
	BTreeMap, PREFLIGHT_CHECK_CONFIG, PREFLIGHT_CHECK_MCP, PREFLIGHT_CHECK_MODEL,
	PREFLIGHT_CHECK_MODEL_PROVIDER, PREFLIGHT_CHECK_PLUGINS, PREFLIGHT_CHECK_SKILLS, Serialize,
	report::status::AppServerCapabilityPreflightStatus,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AppServerCapabilityPreflightCheck {
	pub(crate) name: &'static str,
	pub(crate) status: AppServerCapabilityPreflightStatus,
	pub(crate) summary: String,
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	pub(crate) details: BTreeMap<String, String>,
}

pub(crate) fn check_name_for_method(method: &str) -> &'static str {
	match method {
		"config/read" => PREFLIGHT_CHECK_CONFIG,
		"model/list" => PREFLIGHT_CHECK_MODEL,
		"modelProvider/capabilities/read" => PREFLIGHT_CHECK_MODEL_PROVIDER,
		"skills/list" => PREFLIGHT_CHECK_SKILLS,
		"plugin/list" => PREFLIGHT_CHECK_PLUGINS,
		"mcpServerStatus/list" => PREFLIGHT_CHECK_MCP,
		_ => "introspection",
	}
}

pub(in crate::agent::app_server::preflight::report) fn blocker_summary(
	check: &AppServerCapabilityPreflightCheck,
) -> String {
	let first_error_path = check.details.get("first_error_path");
	let first_error = check.details.get("first_error");
	let mut summary = format!("{}: {}", check.name, check.summary);

	if first_error_path.is_some() || first_error.is_some() {
		let path = first_error_path.map_or("unknown", String::as_str);
		let error = first_error.map_or("unknown", String::as_str);

		summary.push_str(" first_error_path=");
		summary.push_str(path);
		summary.push_str("; first_error=");
		summary.push_str(error);
	}

	summary
}
