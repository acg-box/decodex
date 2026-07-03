use crate::{
	agent::app_server::preflight::{
		AppServerCapabilityPreflightReport, AppServerOutputTimeout, BTreeMap,
		MCP_PREFLIGHT_REQUEST_TIMEOUT, McpServerStatusSummary,
		ModelProviderCapabilitiesReadResponse, ModelSummary, PREFLIGHT_CHECK_CONFIG,
		PREFLIGHT_CHECK_MCP, PREFLIGHT_CHECK_MODEL, PREFLIGHT_CHECK_MODEL_PROVIDER,
		PREFLIGHT_CHECK_PLUGINS, PREFLIGHT_CHECK_SKILLS, PREFLIGHT_EVENT_TYPE, PluginListResponse,
		Report, RunRecorder, RuntimeConfigSummary, SkillsListResponse, Value, serde_json,
	},
	prelude::Result,
};

pub(crate) fn record_config_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	config: &RuntimeConfigSummary,
) {
	let mut details = BTreeMap::new();

	insert_optional_detail(&mut details, "model", config.model.as_deref());
	insert_optional_detail(&mut details, "model_provider", config.model_provider.as_deref());

	if let Some(approval_policy) = config.approval_policy.as_ref().and_then(config_value_name) {
		details.insert(String::from("approval_policy"), approval_policy);
	}
	if let Some(sandbox_mode) = config.sandbox_mode.as_ref().and_then(config_value_name) {
		details.insert(String::from("sandbox_mode"), sandbox_mode);
	}

	report.push_ok(
		PREFLIGHT_CHECK_CONFIG,
		"config/read returned effective runtime configuration.",
		details,
	);
}

pub(crate) fn record_model_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	config: &RuntimeConfigSummary,
	models: &[ModelSummary],
) {
	let configured_model = config.model.as_deref().filter(|model| !model.trim().is_empty());
	let default_model = models.iter().find(|model| model.is_default);
	let matching_config_model = configured_model
		.and_then(|configured| models.iter().find(|model| model_matches_config(model, configured)));
	let mut details = BTreeMap::new();

	details.insert(String::from("model_count"), models.len().to_string());

	if let Some(configured_model) = configured_model {
		details.insert(String::from("configured_model"), configured_model.to_owned());
	}
	if let Some(model) = default_model {
		details.insert(String::from("default_model"), model.model.clone());
	}
	if let Some(model) = matching_config_model {
		details.insert(String::from("matched_model_id"), model.id.clone());
	}

	if models.is_empty() {
		report.push_blocked(
			PREFLIGHT_CHECK_MODEL,
			"model/list returned no available models.",
			details,
		);
	} else if configured_model.is_some() && matching_config_model.is_none() {
		report.push_blocked(
			PREFLIGHT_CHECK_MODEL,
			"configured model was not present in model/list.",
			details,
		);
	} else if configured_model.is_none() && default_model.is_none() {
		report.push_blocked(
			PREFLIGHT_CHECK_MODEL,
			"no configured model or default model was present.",
			details,
		);
	} else {
		report.push_ok(
			PREFLIGHT_CHECK_MODEL,
			"model/list returned an executable model selection.",
			details,
		);
	}
}

pub(crate) fn record_model_provider_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	capabilities: &ModelProviderCapabilitiesReadResponse,
) {
	let mut details = BTreeMap::new();

	details.insert(String::from("web_search"), capabilities.web_search.to_string());
	details.insert(String::from("image_generation"), capabilities.image_generation.to_string());
	details.insert(String::from("namespace_tools"), capabilities.namespace_tools.to_string());
	report.push_ok(
		PREFLIGHT_CHECK_MODEL_PROVIDER,
		"modelProvider/capabilities/read returned provider capabilities.",
		details,
	);
}

pub(crate) fn record_skills_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	cwd: &str,
	skills: &SkillsListResponse,
) {
	let cwd_entry = skills.data.iter().find(|entry| entry.cwd == cwd);
	let all_skill_count: usize = skills.data.iter().map(|entry| entry.skills.len()).sum();
	let enabled_skill_count: usize = skills
		.data
		.iter()
		.flat_map(|entry| entry.skills.iter())
		.filter(|skill| skill.enabled)
		.count();
	let errors = skills.data.iter().flat_map(|entry| entry.errors.iter()).collect::<Vec<_>>();
	let mut details = BTreeMap::new();

	details.insert(String::from("cwd"), cwd.to_owned());
	details.insert(String::from("entry_count"), skills.data.len().to_string());
	details.insert(String::from("skill_count"), all_skill_count.to_string());
	details.insert(String::from("enabled_skill_count"), enabled_skill_count.to_string());
	details.insert(String::from("error_count"), errors.len().to_string());

	if let Some(first_error) = errors.first() {
		details.insert(String::from("first_error_path"), first_error.path.clone());
		details.insert(String::from("first_error"), first_error.message.clone());
	}

	if cwd_entry.is_none() {
		report.push_blocked(
			PREFLIGHT_CHECK_SKILLS,
			"skills/list did not return an entry for the run cwd.",
			details,
		);
	} else if enabled_skill_count == 0 {
		report.push_blocked(
			PREFLIGHT_CHECK_SKILLS,
			"skills/list returned no enabled skills.",
			details,
		);
	} else if errors.is_empty() {
		report.push_ok(PREFLIGHT_CHECK_SKILLS, "skills/list returned enabled skills.", details);
	} else {
		report.push_ok(
			PREFLIGHT_CHECK_SKILLS,
			"skills/list returned enabled skills with scan diagnostics.",
			details,
		);
	}
}

pub(crate) fn record_plugin_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	plugins: &PluginListResponse,
) {
	let plugin_count: usize =
		plugins.marketplaces.iter().map(|marketplace| marketplace.plugins.len()).sum();
	let installed_count = plugins
		.marketplaces
		.iter()
		.flat_map(|marketplace| marketplace.plugins.iter())
		.filter(|plugin| plugin.installed)
		.count();
	let enabled_count = plugins
		.marketplaces
		.iter()
		.flat_map(|marketplace| marketplace.plugins.iter())
		.filter(|plugin| plugin.enabled)
		.count();
	let mut details = BTreeMap::new();

	details.insert(String::from("marketplace_count"), plugins.marketplaces.len().to_string());
	details.insert(String::from("plugin_count"), plugin_count.to_string());
	details.insert(String::from("installed_plugin_count"), installed_count.to_string());
	details.insert(String::from("enabled_plugin_count"), enabled_count.to_string());

	if let Some(first_error) = plugins.marketplace_load_errors.first() {
		details.insert(String::from("first_error_path"), first_error.marketplace_path.clone());
		details.insert(String::from("first_error"), first_error.message.clone());
	}

	if !plugins.marketplace_load_errors.is_empty() {
		report.push_blocked(
			PREFLIGHT_CHECK_PLUGINS,
			"plugin/list returned marketplace load errors.",
			details,
		);
	} else if plugins.marketplaces.is_empty() {
		report.push_blocked(
			PREFLIGHT_CHECK_PLUGINS,
			"plugin/list returned no marketplaces.",
			details,
		);
	} else {
		report.push_ok(PREFLIGHT_CHECK_PLUGINS, "plugin/list returned plugin inventory.", details);
	}
}

pub(crate) fn record_mcp_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	servers: &[McpServerStatusSummary],
) {
	let not_logged_in = servers
		.iter()
		.filter(|server| server.auth_status == "notLoggedIn")
		.map(|server| server.name.clone())
		.collect::<Vec<_>>();
	let tool_count: usize = servers.iter().map(|server| server.tools.len()).sum();
	let mut details = BTreeMap::new();

	details.insert(String::from("server_count"), servers.len().to_string());
	details.insert(String::from("tool_count"), tool_count.to_string());

	if !not_logged_in.is_empty() {
		details.insert(String::from("not_logged_in_servers"), not_logged_in.join(", "));
	}
	if !not_logged_in.is_empty() {
		report.push_blocked(
			PREFLIGHT_CHECK_MCP,
			"mcpServerStatus/list returned MCP servers that are not logged in.",
			details,
		);
	} else {
		report.push_ok(
			PREFLIGHT_CHECK_MCP,
			"mcpServerStatus/list returned MCP server state.",
			details,
		);
	}
}

pub(crate) fn mcp_preflight_can_degrade(error: &Report) -> bool {
	preflight_error_timed_out(error)
}

pub(crate) fn record_mcp_preflight_degraded(
	report: &mut AppServerCapabilityPreflightReport,
	error: &Report,
) {
	let mut details = BTreeMap::new();

	details.insert(String::from("method"), String::from("mcpServerStatus/list"));
	details.insert(String::from("degraded_reason"), String::from("timeout"));
	details.insert(String::from("error"), error.to_string());
	details.insert(
		String::from("timeout_seconds"),
		MCP_PREFLIGHT_REQUEST_TIMEOUT.as_secs().to_string(),
	);
	report.push_ok(
		PREFLIGHT_CHECK_MCP,
		"mcpServerStatus/list timed out during optional MCP inventory; continuing after core app-server capability checks passed.",
		details,
	);
}

pub(crate) fn preflight_error_timed_out(error: &Report) -> bool {
	error.downcast_ref::<AppServerOutputTimeout>().is_some()
}

pub(crate) fn record_app_server_preflight_report(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
) -> Result<()> {
	recorder.record(PREFLIGHT_EVENT_TYPE, &serde_json::to_string(report)?)
}

fn model_matches_config(model: &ModelSummary, configured_model: &str) -> bool {
	model.model == configured_model || model.id == configured_model
}

fn insert_optional_detail(details: &mut BTreeMap<String, String>, name: &str, value: Option<&str>) {
	if let Some(value) = value.filter(|value| !value.is_empty()) {
		details.insert(name.to_owned(), value.to_owned());
	}
}

fn config_value_name(value: &Value) -> Option<String> {
	match value {
		Value::String(value) if !value.is_empty() => Some(value.clone()),
		Value::Object(object) => object
			.get("type")
			.and_then(Value::as_str)
			.map(str::to_owned)
			.or_else(|| (object.len() == 1).then(|| object.keys().next().cloned()).flatten()),
		_ => None,
	}
}
