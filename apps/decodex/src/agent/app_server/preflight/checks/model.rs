use crate::agent::app_server::preflight::{
	AppServerCapabilityPreflightReport, BTreeMap, ModelProviderCapabilitiesReadResponse,
	ModelSummary, PREFLIGHT_CHECK_MODEL, PREFLIGHT_CHECK_MODEL_PROVIDER, RuntimeConfigSummary,
	checks::support,
};

pub(crate) fn record_model_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	config: &RuntimeConfigSummary,
	models: &[ModelSummary],
) {
	let configured_model = config.model.as_deref().filter(|model| !model.trim().is_empty());
	let default_model = models.iter().find(|model| model.is_default);
	let matching_config_model = configured_model.and_then(|configured| {
		models.iter().find(|model| support::model_matches_config(model, configured))
	});
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
