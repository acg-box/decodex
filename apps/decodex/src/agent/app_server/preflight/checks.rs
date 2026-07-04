pub(crate) mod support;

mod config;
mod inventory;
mod mcp;
mod model;
mod record;

pub(crate) use self::{
	config::record_config_preflight,
	inventory::{record_plugin_preflight, record_skills_preflight},
	mcp::{
		mcp_preflight_can_degrade, preflight_error_timed_out, record_mcp_preflight,
		record_mcp_preflight_degraded,
	},
	model::{record_model_preflight, record_model_provider_preflight},
	record::record_app_server_preflight_report,
};
