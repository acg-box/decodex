use std::fs;

use crate::orchestrator::{
	self,
	tests::{self},
};

#[test]
fn daemon_workflow_reload_keeps_last_known_good_on_same_path_failure() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let mut workflow_cache = None;
	let initial = orchestrator::load_daemon_tick_workflow(&config, &mut workflow_cache)
		.expect("initial workflow load should succeed");

	assert_eq!(initial, workflow);

	fs::write(config.workflow_path(), "not valid workflow markdown")
		.expect("invalid workflow should be written");

	let fallback = orchestrator::load_daemon_tick_workflow(&config, &mut workflow_cache)
		.expect("invalid reload should keep the cached workflow");

	assert_eq!(fallback, workflow);
}
