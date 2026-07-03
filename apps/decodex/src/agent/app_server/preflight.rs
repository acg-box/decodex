//! App-server capability preflight and command/exec health checks.

use super::{
	AppServerClient, ConfigReadParams, Duration, REQUEST_TIMEOUT, RunRecorder, SkillsListParams,
};
use color_eyre::eyre::Report;

const MCP_PREFLIGHT_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PLUGIN_PREFLIGHT_MAX_ATTEMPTS: u32 = 2;

mod checks;
mod command_exec;
mod report;
mod requests;

#[cfg(test)]
pub(super) use checks::{
	mcp_preflight_can_degrade, record_config_preflight, record_mcp_preflight,
	record_mcp_preflight_degraded, record_model_preflight, record_model_provider_preflight,
	record_plugin_preflight, record_skills_preflight,
};
pub(crate) use command_exec::CommandExecHealthCheck;
pub(super) use command_exec::run_command_exec_health_check;
#[cfg(test)]
pub(super) use command_exec::{
	build_command_exec_health_check_params, validate_command_exec_health_check_result,
};
#[cfg(test)]
pub(super) use report::AppServerCapabilityPreflightStatus;
pub(crate) use report::{AppServerCapabilityPreflightFailure, AppServerCapabilityPreflightReport};
#[cfg(test)]
pub(super) use requests::{
	plugin_list_params_for_preflight, preflight_request, preflight_request_with_timeout_retry,
};

use checks::record_app_server_preflight_report;
#[cfg(not(test))]
use checks::{
	mcp_preflight_can_degrade, record_config_preflight, record_mcp_preflight,
	record_mcp_preflight_degraded, record_model_preflight, record_model_provider_preflight,
	record_plugin_preflight, record_skills_preflight,
};
use requests::{
	list_all_mcp_servers_for_preflight, list_all_models_for_preflight, preflight_method_failure,
};
#[cfg(not(test))]
use requests::{
	plugin_list_params_for_preflight, preflight_request, preflight_request_with_timeout_retry,
};

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
