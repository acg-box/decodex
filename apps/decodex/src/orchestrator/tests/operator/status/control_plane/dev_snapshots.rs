use crate::orchestrator::{
	ProjectDaemonRuntime,
	tests::operator::status::{
		self, Instant, ProjectRegistration, StateStore, TestEnvVarGuard, fs, orchestrator, state,
	},
};

#[test]
fn control_plane_linear_scan_cadence_uses_fixed_window_and_manual_override() {
	let now = Instant::now();
	let mut runtime = ProjectDaemonRuntime::default();

	assert!(orchestrator::linear_scan_due("pubfi", &runtime, &[], now));

	orchestrator::remember_next_linear_scan(&mut runtime, now);

	assert!(!orchestrator::linear_scan_due("pubfi", &runtime, &[], now));
	assert!(orchestrator::linear_scan_due(
		"pubfi",
		&runtime,
		&[orchestrator::OperatorLinearScanRequest { project_id: None }],
		now
	));
	assert!(orchestrator::linear_scan_due(
		"pubfi",
		&runtime,
		&[orchestrator::OperatorLinearScanRequest { project_id: Some(String::from("pubfi")) }],
		now
	));
	assert!(!orchestrator::linear_scan_due(
		"pubfi",
		&runtime,
		&[orchestrator::OperatorLinearScanRequest { project_id: Some(String::from("rsnap")) }],
		now
	));
}

#[test]
fn control_plane_dev_snapshot_does_not_tick_enabled_projects() {
	let (temp_dir, config, _workflow) = status::temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&status::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);

	state_store.upsert_project(&registration).expect("project should register");

	let snapshot =
		orchestrator::run_control_plane_dev_tick(&state_store).expect("dev snapshot should build");
	let project = snapshot.projects.first().expect("enabled project should be listed");

	assert_eq!(snapshot.projects.len(), 1);
	assert_eq!(project.project_id, "pubfi");
	assert!(project.enabled);
	assert_eq!(project.connector_state, "dev");
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.queued_candidate_count, 0);
	assert_eq!(project.warning_count, 1);
	assert!(snapshot.current_lanes.is_empty());
	assert!(snapshot.queued_candidates.is_empty());
	assert!(snapshot.warnings.contains(&String::from("automation_disabled")));
	assert!(!snapshot.warnings.contains(&String::from("no_enabled_projects")));
}

#[test]
fn control_plane_dev_snapshot_marks_unloadable_project_config() {
	let (temp_dir, config, _workflow) = status::temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let missing_config_path = temp_dir.path().join("missing/project.toml");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&missing_config_path,
		&config,
		true,
		"test-fingerprint",
	);

	state_store.upsert_project(&registration).expect("project should register");

	let snapshot = orchestrator::run_control_plane_dev_tick(&state_store)
		.expect("dev snapshot should still build");
	let project = snapshot.projects.first().expect("enabled project should be listed");

	assert_eq!(snapshot.projects.len(), 1);
	assert_eq!(project.project_id, "pubfi");
	assert!(project.enabled);
	assert_eq!(project.connector_state, "config_error");
	assert_eq!(project.warning_count, 2);
	assert!(snapshot.current_lanes.is_empty());
	assert!(snapshot.warnings.contains(&String::from("automation_disabled")));
	assert!(snapshot.warnings.contains(&String::from("operator_snapshot_build_failed")));
}

#[test]
fn control_plane_dev_snapshot_includes_local_current_lanes() {
	let (temp_dir, config, _workflow) = status::temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&status::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = status::sample_issue("Todo", &[]);

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("run-active", &issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "run-active", "In Progress")
		.expect("run lease should record");

	let snapshot =
		orchestrator::run_control_plane_dev_tick(&state_store).expect("dev snapshot should build");
	let project = snapshot.projects.first().expect("enabled project should be listed");

	assert_eq!(snapshot.projects.len(), 1);
	assert_eq!(project.project_id, "pubfi");
	assert_eq!(project.connector_state, "dev");
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 1);
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].run_id, "run-active");
	assert_eq!(snapshot.current_lanes[0].project_id, "pubfi");
	assert_eq!(snapshot.current_lanes[0].phase, "executing");
	assert!(snapshot.queued_candidates.is_empty());
	assert!(snapshot.warnings.contains(&String::from("automation_disabled")));
}

#[test]
fn control_plane_dev_snapshot_separates_visible_current_lanes_from_running_lanes() {
	let (temp_dir, config, _workflow) = status::temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&status::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = status::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("run-active", &issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "run-active", "In Progress")
		.expect("run lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-active", 1, u32::MAX)
		.expect("stopped process marker should write");

	let snapshot =
		orchestrator::run_control_plane_dev_tick(&state_store).expect("dev snapshot should build");
	let project = snapshot.projects.first().expect("enabled project should be listed");
	let run = snapshot.current_lanes.first().expect("stopped current lane should stay visible");

	assert_eq!(project.current_lane_count, snapshot.current_lanes.len());
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
	assert_eq!(run.run_id, "run-active");
	assert_eq!(run.status, "running");
	assert_eq!(run.execution_liveness, "process_stopped");
	assert!(snapshot.warnings.contains(&String::from("automation_disabled")));
}
