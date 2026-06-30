//! App-server capability preflight and command/exec health checks.

use super::{
	AppServerClient, AppServerOutputTimeout, AppServerRunRequest, BTreeMap, CommandExecParams,
	CommandExecResponse, ConfigReadParams, Display, Duration, Error, Formatter,
	ListMcpServerStatusParams, ListMcpServerStatusResponse, McpServerStatusSummary,
	ModelListParams, ModelListResponse, ModelProviderCapabilitiesReadResponse, ModelSummary,
	PREFLIGHT_CHECK_CONFIG, PREFLIGHT_CHECK_MCP, PREFLIGHT_CHECK_MODEL,
	PREFLIGHT_CHECK_MODEL_PROVIDER, PREFLIGHT_CHECK_PLUGINS, PREFLIGHT_CHECK_SKILLS,
	PREFLIGHT_EVENT_TYPE, PREFLIGHT_MCP_DETAIL, PREFLIGHT_MCP_PAGE_LIMIT,
	PREFLIGHT_MODEL_PAGE_LIMIT, PREFLIGHT_PLUGIN_MARKETPLACE_KIND,
	PROBE_COMMAND_EXEC_EXPECTED_OUTPUT, PROBE_COMMAND_EXEC_OUTPUT_BYTES_CAP,
	PROBE_COMMAND_EXEC_TIMEOUT_MS, PluginListParams, PluginListResponse, REQUEST_TIMEOUT,
	RunRecorder, RuntimeConfigSummary, Serialize, SkillsListParams, SkillsListResponse, Value,
	eyre, flush_pending_messages, fmt, serde_json,
};
use color_eyre::eyre::Report;

const MCP_PREFLIGHT_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PLUGIN_PREFLIGHT_MAX_ATTEMPTS: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AppServerCapabilityPreflightStatus {
	Ok,
	Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppServerCapabilityPreflightFailureKind {
	MethodFailed { method: &'static str, error: String, timed_out: bool },
	BlockedState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AppServerCapabilityPreflightReport {
	checks: Vec<AppServerCapabilityPreflightCheck>,
}
impl AppServerCapabilityPreflightReport {
	pub(crate) fn new() -> Self {
		Self { checks: Vec::new() }
	}

	#[cfg(test)]
	pub(super) fn checks(&self) -> &[AppServerCapabilityPreflightCheck] {
		&self.checks
	}

	pub(crate) fn check_count(&self) -> usize {
		self.checks.len()
	}

	pub(super) fn push_ok(
		&mut self,
		name: &'static str,
		summary: impl Into<String>,
		details: BTreeMap<String, String>,
	) {
		self.checks.push(AppServerCapabilityPreflightCheck {
			name,
			status: AppServerCapabilityPreflightStatus::Ok,
			summary: summary.into(),
			details,
		});
	}

	fn push_blocked(
		&mut self,
		name: &'static str,
		summary: impl Into<String>,
		details: BTreeMap<String, String>,
	) {
		self.checks.push(AppServerCapabilityPreflightCheck {
			name,
			status: AppServerCapabilityPreflightStatus::Blocked,
			summary: summary.into(),
			details,
		});
	}

	pub(super) fn has_blockers(&self) -> bool {
		self.checks.iter().any(|check| check.status == AppServerCapabilityPreflightStatus::Blocked)
	}

	pub(super) fn blocker_summary(&self) -> String {
		let blockers = self
			.checks
			.iter()
			.filter(|check| check.status == AppServerCapabilityPreflightStatus::Blocked)
			.map(preflight_check_blocker_summary)
			.collect::<Vec<_>>();

		if blockers.is_empty() { String::from("no blockers recorded") } else { blockers.join("; ") }
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppServerCapabilityPreflightFailure {
	kind: AppServerCapabilityPreflightFailureKind,
	report: AppServerCapabilityPreflightReport,
}
impl AppServerCapabilityPreflightFailure {
	fn blocked(report: AppServerCapabilityPreflightReport) -> Self {
		Self { kind: AppServerCapabilityPreflightFailureKind::BlockedState, report }
	}

	pub(super) fn method_failed(
		method: &'static str,
		error: String,
		report: AppServerCapabilityPreflightReport,
	) -> Self {
		Self {
			kind: AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				error,
				timed_out: false,
			},
			report,
		}
	}

	fn method_timed_out(
		method: &'static str,
		error: String,
		report: AppServerCapabilityPreflightReport,
	) -> Self {
		Self {
			kind: AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				error,
				timed_out: true,
			},
			report,
		}
	}

	#[cfg(test)]
	pub(crate) fn blocked_for_test(check: &'static str, summary: &str) -> Self {
		let mut report = AppServerCapabilityPreflightReport::new();

		report.push_blocked(check, summary, BTreeMap::new());

		Self::blocked(report)
	}

	#[cfg(test)]
	pub(crate) fn blocked_for_test_with_details(
		check: &'static str,
		summary: &str,
		details: BTreeMap<String, String>,
	) -> Self {
		let mut report = AppServerCapabilityPreflightReport::new();

		report.push_blocked(check, summary, details);

		Self::blocked(report)
	}

	#[cfg(test)]
	pub(crate) fn method_timed_out_for_test(method: &'static str, error: String) -> Self {
		let mut report = AppServerCapabilityPreflightReport::new();

		report.push_blocked(
			check_name_for_method(method),
			format!("`{method}` timed out."),
			BTreeMap::new(),
		);

		Self::method_timed_out(method, error, report)
	}

	pub(crate) fn error_class(&self) -> &'static str {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method: "plugin/list",
				timed_out: true,
				..
			} => "app_server_plugin_list_timeout",
			AppServerCapabilityPreflightFailureKind::MethodFailed { timed_out: true, .. } =>
				"app_server_preflight_timeout",
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. } =>
				"app_server_introspection_method_failed",
			AppServerCapabilityPreflightFailureKind::BlockedState =>
				"app_server_runtime_preflight_failed",
		}
	}

	pub(crate) fn is_retryable_timeout(&self) -> bool {
		matches!(
			self.kind,
			AppServerCapabilityPreflightFailureKind::MethodFailed { timed_out: true, .. }
		)
	}

	pub(crate) fn retry_next_action(&self) -> String {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method: "plugin/list",
				timed_out: true,
				..
			} => String::from(
				"decodex will retry app-server preflight automatically; inspect local app_server_preflight_failed evidence for the `plugin/list` timeout and restart `decodex serve` if the retry budget exhausts",
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				timed_out: true,
				..
			} => format!(
				"decodex will retry app-server preflight automatically; inspect local app_server_preflight_failed evidence for the `{method}` timeout and restart `decodex serve` if the retry budget exhausts"
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. }
			| AppServerCapabilityPreflightFailureKind::BlockedState =>
				String::from("app-server preflight requires operator recovery"),
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method: "plugin/list",
				timed_out: true,
				..
			} => format!(
				"inspect local app_server_preflight_failed evidence for the `plugin/list` timeout, restart `decodex serve` if the app-server is stale, run `decodex probe` to confirm plugin inventory recovers, {recovery_gate}"
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				timed_out: true,
				..
			} => format!(
				"inspect local app_server_preflight_failed evidence for the `{method}` timeout, restart `decodex serve` if the app-server is stale, run `decodex probe` to confirm app-server preflight recovers, {recovery_gate}"
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { .. } => format!(
				"inspect the Codex app-server preflight status, repair the local Codex runtime configuration, restart `decodex serve`, {recovery_gate}"
			),
			AppServerCapabilityPreflightFailureKind::BlockedState => {
				let blocker_summary = self.blocker_summary();

				format!(
					"inspect local app_server_preflight_failed evidence for `{blocker_summary}`, repair the local Codex runtime configuration, restart `decodex serve`, {recovery_gate}"
				)
			},
		}
	}

	fn blocker_summary(&self) -> String {
		match &self.kind {
			AppServerCapabilityPreflightFailureKind::MethodFailed {
				method,
				error,
				timed_out: true,
			} => format!(
				"{}: `{method}` timed out during preflight: {error}",
				check_name_for_method(method)
			),
			AppServerCapabilityPreflightFailureKind::MethodFailed { method, error, .. } => {
				format!("{}: `{method}` returned {error}", check_name_for_method(method))
			},
			AppServerCapabilityPreflightFailureKind::BlockedState => self.report.blocker_summary(),
		}
	}

	#[cfg(test)]
	pub(super) fn report(&self) -> &AppServerCapabilityPreflightReport {
		&self.report
	}
}

impl Display for AppServerCapabilityPreflightFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		write!(formatter, "app_server_preflight_failed: {}", self.blocker_summary())
	}
}

impl Error for AppServerCapabilityPreflightFailure {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandExecHealthCheck {
	pub(crate) command: Vec<String>,
	pub(crate) expected_stdout: String,
	pub(crate) timeout_ms: u64,
	pub(crate) output_bytes_cap: u64,
}
impl CommandExecHealthCheck {
	pub(super) fn probe() -> Self {
		Self {
			command: vec![
				String::from("/bin/sh"),
				String::from("-c"),
				format!("printf {PROBE_COMMAND_EXEC_EXPECTED_OUTPUT}"),
			],
			expected_stdout: String::from(PROBE_COMMAND_EXEC_EXPECTED_OUTPUT),
			timeout_ms: PROBE_COMMAND_EXEC_TIMEOUT_MS,
			output_bytes_cap: PROBE_COMMAND_EXEC_OUTPUT_BYTES_CAP,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct AppServerCapabilityPreflightCheck {
	pub(super) name: &'static str,
	pub(super) status: AppServerCapabilityPreflightStatus,
	pub(super) summary: String,
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	pub(super) details: BTreeMap<String, String>,
}

fn preflight_check_blocker_summary(check: &AppServerCapabilityPreflightCheck) -> String {
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

pub(super) fn run_app_server_capability_preflight(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	cwd: &str,
) -> crate::prelude::Result<AppServerCapabilityPreflightReport> {
	let mut report = AppServerCapabilityPreflightReport::new();
	let config = preflight_request(recorder, &report, "config/read", || {
		client.read_config(&ConfigReadParams { cwd: Some(cwd.to_owned()), include_layers: false })
	})?;

	record_config_preflight(&mut report, &config.config);

	let models = list_all_models_for_preflight(client, recorder, &report)?;

	record_model_preflight(&mut report, &config.config, &models);

	let provider_capabilities =
		preflight_request(recorder, &report, "modelProvider/capabilities/read", || {
			client.read_model_provider_capabilities()
		})?;

	record_model_provider_preflight(&mut report, &provider_capabilities);

	let skills = preflight_request(recorder, &report, "skills/list", || {
		client.list_skills(&SkillsListParams {
			cwds: vec![cwd.to_owned()],
			force_reload: false,
			per_cwd_extra_user_roots: None,
		})
	})?;

	record_skills_preflight(&mut report, cwd, &skills);

	let plugins = preflight_request_with_timeout_retry(
		recorder,
		&report,
		"plugin/list",
		REQUEST_TIMEOUT,
		PLUGIN_PREFLIGHT_MAX_ATTEMPTS,
		|| client.list_plugins(&plugin_list_params_for_preflight(cwd)),
	)?;

	record_plugin_preflight(&mut report, &plugins);

	match list_all_mcp_servers_for_preflight(client) {
		Ok(mcp_servers) => record_mcp_preflight(&mut report, &mcp_servers),
		Err(error) if mcp_preflight_can_degrade(&error) => {
			record_mcp_preflight_degraded(&mut report, &error);
		},
		Err(error) => {
			return preflight_method_failure(
				recorder,
				&report,
				"mcpServerStatus/list",
				MCP_PREFLIGHT_REQUEST_TIMEOUT,
				1,
				error,
			);
		},
	}

	record_app_server_preflight_report(recorder, &report)?;

	if report.has_blockers() {
		return Err(Report::new(AppServerCapabilityPreflightFailure::blocked(report)));
	}

	Ok(report)
}

pub(super) fn plugin_list_params_for_preflight(cwd: &str) -> PluginListParams {
	PluginListParams {
		cwds: Some(vec![cwd.to_owned()]),
		marketplace_kinds: Some(vec![PREFLIGHT_PLUGIN_MARKETPLACE_KIND.to_owned()]),
	}
}

fn preflight_method_failure<T>(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
	method: &'static str,
	request_timeout: Duration,
	attempt_count: u32,
	error: Report,
) -> crate::prelude::Result<T> {
	let error_message = error.to_string();
	let timed_out = preflight_error_timed_out(&error);
	let retry_count = attempt_count.saturating_sub(1);
	let mut failed_report = report.clone();
	let mut details = BTreeMap::new();

	details.insert(String::from("method"), method.to_owned());
	details.insert(String::from("error"), error_message.clone());
	details.insert(String::from("attempt_count"), attempt_count.to_string());

	if retry_count > 0 {
		details.insert(String::from("retry_count"), retry_count.to_string());
	}
	if timed_out {
		details.insert(String::from("failure_reason"), String::from("timeout"));
		details.insert(String::from("timeout_seconds"), request_timeout.as_secs().to_string());
	}

	failed_report.push_blocked(
		check_name_for_method(method),
		if timed_out {
			format!("`{method}` timed out before thread/start after {attempt_count} attempts.")
		} else {
			format!("`{method}` failed before thread/start.")
		},
		details,
	);

	record_app_server_preflight_report(recorder, &failed_report)?;

	let failure = if timed_out {
		AppServerCapabilityPreflightFailure::method_timed_out(method, error_message, failed_report)
	} else {
		AppServerCapabilityPreflightFailure::method_failed(method, error_message, failed_report)
	};

	Err(Report::new(failure))
}

pub(super) fn preflight_request<T, F>(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
	method: &'static str,
	request: F,
) -> crate::prelude::Result<T>
where
	F: FnOnce() -> crate::prelude::Result<T>,
{
	match request() {
		Ok(response) => Ok(response),
		Err(error) => preflight_method_failure(recorder, report, method, REQUEST_TIMEOUT, 1, error),
	}
}

pub(super) fn preflight_request_with_timeout_retry<T, F>(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
	method: &'static str,
	request_timeout: Duration,
	max_attempts: u32,
	mut request: F,
) -> crate::prelude::Result<T>
where
	F: FnMut() -> crate::prelude::Result<T>,
{
	let max_attempts = max_attempts.max(1);
	let mut attempt_count = 1;

	loop {
		match request() {
			Ok(response) => return Ok(response),
			Err(error) if preflight_error_timed_out(&error) && attempt_count < max_attempts => {
				tracing::warn!(
					method,
					attempt = attempt_count,
					max_attempts,
					"Retrying app-server preflight method after timeout."
				);

				attempt_count += 1;
			},
			Err(error) => {
				return preflight_method_failure(
					recorder,
					report,
					method,
					request_timeout,
					attempt_count,
					error,
				);
			},
		}
	}
}

fn list_all_models_for_preflight(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
) -> crate::prelude::Result<Vec<ModelSummary>> {
	let mut cursor = None;
	let mut models = Vec::new();

	loop {
		let response: ModelListResponse =
			preflight_request(recorder, report, "model/list", || {
				client.list_models(&ModelListParams {
					cursor: cursor.clone(),
					include_hidden: Some(true),
					limit: Some(PREFLIGHT_MODEL_PAGE_LIMIT),
				})
			})?;

		models.extend(response.data);

		let Some(next_cursor) = response.next_cursor else {
			return Ok(models);
		};

		cursor = Some(next_cursor);
	}
}

fn list_all_mcp_servers_for_preflight(
	client: &mut AppServerClient,
) -> crate::prelude::Result<Vec<McpServerStatusSummary>> {
	let mut cursor = None;
	let mut servers = Vec::new();

	loop {
		let response: ListMcpServerStatusResponse = client.list_mcp_server_status(
			&ListMcpServerStatusParams {
				cursor: cursor.clone(),
				detail: Some(PREFLIGHT_MCP_DETAIL.to_owned()),
				limit: Some(PREFLIGHT_MCP_PAGE_LIMIT),
			},
			MCP_PREFLIGHT_REQUEST_TIMEOUT,
		)?;

		servers.extend(response.data);

		let Some(next_cursor) = response.next_cursor else {
			return Ok(servers);
		};

		cursor = Some(next_cursor);
	}
}

pub(super) fn record_config_preflight(
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

pub(super) fn record_model_preflight(
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

pub(super) fn record_model_provider_preflight(
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

pub(super) fn record_skills_preflight(
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

pub(super) fn record_plugin_preflight(
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

pub(super) fn record_mcp_preflight(
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

pub(super) fn mcp_preflight_can_degrade(error: &Report) -> bool {
	preflight_error_timed_out(error)
}

fn preflight_error_timed_out(error: &Report) -> bool {
	error.downcast_ref::<AppServerOutputTimeout>().is_some()
}

pub(super) fn record_mcp_preflight_degraded(
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

fn record_app_server_preflight_report(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
) -> crate::prelude::Result<()> {
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

fn check_name_for_method(method: &str) -> &'static str {
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

pub(super) fn run_command_exec_health_check(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	health_check: &CommandExecHealthCheck,
) -> crate::prelude::Result<()> {
	let params = build_command_exec_health_check_params(health_check, &request.cwd);
	let response = client.command_exec(&params)?;

	flush_pending_messages(client, recorder, None)?;

	validate_command_exec_health_check_result(health_check, &response)
}

pub(super) fn build_command_exec_health_check_params(
	health_check: &CommandExecHealthCheck,
	cwd: &str,
) -> CommandExecParams {
	CommandExecParams {
		command: health_check.command.clone(),
		cwd: Some(cwd.to_owned()),
		timeout_ms: Some(health_check.timeout_ms),
		output_bytes_cap: Some(health_check.output_bytes_cap),
	}
}

pub(super) fn validate_command_exec_health_check_result(
	health_check: &CommandExecHealthCheck,
	response: &CommandExecResponse,
) -> crate::prelude::Result<()> {
	if response.exit_code != 0 {
		eyre::bail!(
			"`command/exec` health check failed with exit code {}. stdout: {:?}; stderr: {:?}",
			response.exit_code,
			response.stdout,
			response.stderr
		);
	}
	if response.stdout != health_check.expected_stdout {
		eyre::bail!(
			"`command/exec` health check returned stdout {:?}, expected {:?}. stderr: {:?}",
			response.stdout,
			health_check.expected_stdout,
			response.stderr
		);
	}
	if !response.stderr.is_empty() {
		eyre::bail!("`command/exec` health check wrote unexpected stderr: {:?}", response.stderr);
	}

	Ok(())
}
