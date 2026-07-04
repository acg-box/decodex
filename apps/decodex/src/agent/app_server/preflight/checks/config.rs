use crate::agent::app_server::preflight::{
	AppServerCapabilityPreflightReport, BTreeMap, PREFLIGHT_CHECK_CONFIG, RuntimeConfigSummary,
	checks::support,
};

pub(crate) fn record_config_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	config: &RuntimeConfigSummary,
) {
	let mut details = BTreeMap::new();

	support::insert_optional_detail(&mut details, "model", config.model.as_deref());
	support::insert_optional_detail(
		&mut details,
		"model_provider",
		config.model_provider.as_deref(),
	);

	if let Some(approval_policy) =
		config.approval_policy.as_ref().and_then(support::config_value_name)
	{
		details.insert(String::from("approval_policy"), approval_policy);
	}
	if let Some(sandbox_mode) = config.sandbox_mode.as_ref().and_then(support::config_value_name) {
		details.insert(String::from("sandbox_mode"), sandbox_mode);
	}

	report.push_ok(
		PREFLIGHT_CHECK_CONFIG,
		"config/read returned effective runtime configuration.",
		details,
	);
}
