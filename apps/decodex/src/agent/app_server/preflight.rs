//! App-server capability preflight and command/exec health checks.

mod checks;
mod command_exec;
mod report;
mod requests;

pub(crate) use self::{
	command_exec::CommandExecHealthCheck,
	report::{AppServerCapabilityPreflightFailure, AppServerCapabilityPreflightReport},
};
#[cfg(test)]
pub(crate) use self::{
	command_exec::{
		build_command_exec_health_check_params, validate_command_exec_health_check_result,
	},
	report::AppServerCapabilityPreflightStatus,
};
#[cfg(test)]
pub(crate) use checks::{
	mcp_preflight_can_degrade, record_config_preflight, record_mcp_preflight,
	record_mcp_preflight_degraded, record_model_preflight, record_model_provider_preflight,
	record_plugin_preflight, record_skills_preflight,
};
pub(crate) use command_exec::run_command_exec_health_check;
#[cfg(test)]
pub(crate) use requests::{
	plugin_list_params_for_preflight, preflight_request, preflight_request_with_timeout_retry,
};

use color_eyre::eyre::Report;

use crate::{
	agent::app_server::{
		AppServerClient, AppServerOutputTimeout, AppServerRunRequest, BTreeMap, CommandExecParams,
		CommandExecResponse, ConfigReadParams, Display, Duration, Error, Formatter,
		ListMcpServerStatusParams, ListMcpServerStatusResponse, McpServerStatusSummary,
		ModelListParams, ModelListResponse, ModelProviderCapabilitiesReadResponse, ModelSummary,
		PluginListParams, PluginListResponse, REQUEST_TIMEOUT, RunRecorder, RuntimeConfigSummary,
		Serialize, SkillsListParams, SkillsListResponse, Value,
		constants::{
			PREFLIGHT_CHECK_CONFIG, PREFLIGHT_CHECK_MCP, PREFLIGHT_CHECK_MODEL,
			PREFLIGHT_CHECK_MODEL_PROVIDER, PREFLIGHT_CHECK_PLUGINS, PREFLIGHT_CHECK_SKILLS,
			PREFLIGHT_EVENT_TYPE, PREFLIGHT_MCP_DETAIL, PREFLIGHT_MCP_PAGE_LIMIT,
			PREFLIGHT_MODEL_PAGE_LIMIT, PREFLIGHT_PLUGIN_MARKETPLACE_KIND,
			PROBE_COMMAND_EXEC_EXPECTED_OUTPUT, PROBE_COMMAND_EXEC_OUTPUT_BYTES_CAP,
			PROBE_COMMAND_EXEC_TIMEOUT_MS,
		},
		eyre, fmt, serde_json, turn_loop,
	},
	prelude::Result,
};

const MCP_PREFLIGHT_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PLUGIN_PREFLIGHT_MAX_ATTEMPTS: u32 = 2;

pub(super) fn run_app_server_capability_preflight(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	cwd: &str,
) -> Result<AppServerCapabilityPreflightReport> {
	let mut report = AppServerCapabilityPreflightReport::new();
	let config = requests::preflight_request(recorder, &report, "config/read", || {
		client.read_config(&ConfigReadParams { cwd: Some(cwd.to_owned()), include_layers: false })
	})?;

	checks::record_config_preflight(&mut report, &config.config);

	let models = requests::list_all_models_for_preflight(client, recorder, &report)?;

	checks::record_model_preflight(&mut report, &config.config, &models);

	let provider_capabilities =
		requests::preflight_request(recorder, &report, "modelProvider/capabilities/read", || {
			client.read_model_provider_capabilities()
		})?;

	checks::record_model_provider_preflight(&mut report, &provider_capabilities);

	let skills = requests::preflight_request(recorder, &report, "skills/list", || {
		client.list_skills(&SkillsListParams {
			cwds: vec![cwd.to_owned()],
			force_reload: false,
			per_cwd_extra_user_roots: None,
		})
	})?;

	checks::record_skills_preflight(&mut report, cwd, &skills);

	let plugins = requests::preflight_request_with_timeout_retry(
		recorder,
		&report,
		"plugin/list",
		REQUEST_TIMEOUT,
		PLUGIN_PREFLIGHT_MAX_ATTEMPTS,
		|| client.list_plugins(&requests::plugin_list_params_for_preflight(cwd)),
	)?;

	checks::record_plugin_preflight(&mut report, &plugins);

	match requests::list_all_mcp_servers_for_preflight(client) {
		Ok(mcp_servers) => checks::record_mcp_preflight(&mut report, &mcp_servers),
		Err(error) if checks::mcp_preflight_can_degrade(&error) => {
			checks::record_mcp_preflight_degraded(&mut report, &error);
		},
		Err(error) => {
			return requests::preflight_method_failure(
				recorder,
				&report,
				"mcpServerStatus/list",
				MCP_PREFLIGHT_REQUEST_TIMEOUT,
				1,
				error,
			);
		},
	}

	checks::record_app_server_preflight_report(recorder, &report)?;

	if report.has_blockers() {
		return Err(Report::new(AppServerCapabilityPreflightFailure::blocked(report)));
	}

	Ok(report)
}
