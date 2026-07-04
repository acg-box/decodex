use crate::agent::app_server::preflight::{
	AppServerCapabilityPreflightReport, BTreeMap, PREFLIGHT_CHECK_PLUGINS, PREFLIGHT_CHECK_SKILLS,
	PluginListResponse, SkillsListResponse,
};

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
