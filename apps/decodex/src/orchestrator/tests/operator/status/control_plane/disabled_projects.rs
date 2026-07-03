use crate::orchestrator::tests::operator::status::{
	self, HashMap, ProjectRegistration, StateStore, TestEnvVarGuard, env, orchestrator,
};

#[test]
fn control_plane_snapshot_lists_disabled_registered_projects() {
	let (temp_dir, config, _workflow) = status::temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&status::service_config_path(config.repo_root()),
		&config,
		false,
		"test-fingerprint",
	);

	state_store.upsert_project(&registration).expect("project should register");

	let mut project_runtimes = HashMap::new();
	let snapshot = orchestrator::run_control_plane_tick(&state_store, &mut project_runtimes, &[])
		.expect("control-plane snapshot should build");
	let project = snapshot.projects.first().expect("disabled project should be listed");

	assert_eq!(snapshot.project_id, "all");
	assert_eq!(snapshot.projects.len(), 1);
	assert_eq!(project.project_id, "pubfi");
	assert!(!project.enabled);
	assert_eq!(snapshot.account_control.mode, "balanced");
	assert_eq!(snapshot.account_control.account_selector, None);
	assert_eq!(project.connector_state, "disabled");
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.retained_worktree_count, 0);
	assert!(snapshot.warnings.contains(&String::from("no_enabled_projects")));
	assert!(project_runtimes.is_empty(), "disabled projects should not be ticked");
}

#[test]
fn control_plane_snapshot_includes_disabled_project_current_lanes_without_ticking() {
	let (temp_dir, config, _workflow) = status::temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer_store = StateStore::open(&state_path).expect("observer store should open");
	let writer_store = StateStore::open(&state_path).expect("writer store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&status::service_config_path(config.repo_root()),
		&config,
		false,
		"test-fingerprint",
	);
	let issue = status::sample_issue("In Progress", &[]);

	observer_store.upsert_project(&registration).expect("project should register");
	writer_store
		.record_run_attempt("run-disabled-active", &issue.id, 1, "running")
		.expect("current lane should record");
	writer_store
		.upsert_lease(config.service_id(), &issue.id, "run-disabled-active", "In Progress")
		.expect("run lease should record");

	let mut project_runtimes = HashMap::new();
	let snapshot =
		orchestrator::run_control_plane_tick(&observer_store, &mut project_runtimes, &[])
			.expect("control-plane snapshot should build");
	let project = snapshot.projects.first().expect("disabled project should be listed");

	assert_eq!(snapshot.project_id, "pubfi");
	assert_eq!(snapshot.projects.len(), 1);
	assert_eq!(project.project_id, "pubfi");
	assert!(!project.enabled);
	assert_eq!(project.connector_state, "disabled");
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].run_id, "run-disabled-active");
	assert_eq!(snapshot.current_lanes[0].project_id, "pubfi");
	assert_eq!(snapshot.current_lanes[0].phase, "executing");
	assert!(snapshot.warnings.contains(&String::from("no_enabled_projects")));
	assert!(project_runtimes.is_empty(), "disabled projects should not be ticked");
}

#[test]
fn control_plane_context_failure_includes_project_warning_detail() {
	let (_temp_dir, base_config, _workflow) = status::temp_project_layout();
	let missing_env_var = "DECODEX_TEST_MISSING_CONTROL_PLANE_LINEAR_API_KEY";
	let _env_lock = TestEnvVarGuard::lock();

	unsafe {
		env::remove_var(missing_env_var);
	}

	status::write_service_config(
		base_config.repo_root(),
		&status::sample_service_config_toml(
			base_config.service_id(),
			missing_env_var,
			base_config.github().token_env_var(),
			None,
			base_config.codex().review_level(),
		),
	);

	let config = status::load_service_config(base_config.repo_root());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&status::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);

	state_store.upsert_project(&registration).expect("project should register");

	let mut project_runtimes = HashMap::new();
	let snapshot = orchestrator::run_control_plane_tick(&state_store, &mut project_runtimes, &[])
		.expect("control-plane snapshot should build");
	let project = snapshot.projects.first().expect("enabled project should be listed");
	let detail = snapshot
		.warning_details
		.iter()
		.find(|detail| detail.warning == "control_plane_tick_context_failed")
		.expect("context warning detail should be surfaced");

	assert!(snapshot.warnings.contains(&String::from("control_plane_tick_context_failed")));
	assert_eq!(project.project_id, "pubfi");
	assert_eq!(project.connector_state, "degraded");
	assert_eq!(project.warning_count, 1);
	assert_eq!(detail.project_id.as_deref(), Some("pubfi"));
	assert_eq!(detail.repo_root.as_deref(), Some(config.repo_root().to_str().expect("utf-8 path")));
	assert!(detail.reason.contains(missing_env_var), "detail reason: {}", detail.reason);
	assert!(
		detail.reason.contains(&registration.config_path().display().to_string()),
		"detail reason should include config path: {}",
		detail.reason,
	);
	assert!(
		detail.next_action.as_deref().is_some_and(|action| {
			action.contains("launchctl setenv") && action.contains(missing_env_var)
		}),
		"detail next action should explain macOS GUI env setup: {:?}",
		detail.next_action,
	);
}
