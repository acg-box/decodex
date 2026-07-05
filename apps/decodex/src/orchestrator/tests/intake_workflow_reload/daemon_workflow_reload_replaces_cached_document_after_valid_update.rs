use std::fs;

use crate::orchestrator::{
	self,
	tests::{self},
};

#[test]
fn daemon_workflow_reload_replaces_cached_document_after_valid_update() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let mut workflow_cache = None;

	orchestrator::load_daemon_tick_workflow(&config, &mut workflow_cache)
		.expect("initial workflow load should succeed");

	let updated_workflow =
		tests::sample_workflow_markdown("pubfi", &[], "Updated workflow policy.\n", 1)
			.replace("max_attempts = 3", "max_attempts = 5");

	fs::write(config.workflow_path(), updated_workflow)
		.expect("updated workflow should be written");

	let reloaded = orchestrator::load_daemon_tick_workflow(&config, &mut workflow_cache)
		.expect("valid reload should replace the cached workflow");

	assert_ne!(reloaded, workflow);
	assert_eq!(reloaded.frontmatter().execution().max_attempts(), 5);
	assert_eq!(reloaded.body(), "Updated workflow policy.");
}
