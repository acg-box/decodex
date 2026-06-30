use super::*;

use orchestrator::{ProjectDaemonRuntime, StatusSnapshotHttpResponse};

#[test]
fn control_plane_snapshot_lists_disabled_registered_projects() {
	let (temp_dir, config, _workflow) = temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
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
	let (temp_dir, config, _workflow) = temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer_store = StateStore::open(&state_path).expect("observer store should open");
	let writer_store = StateStore::open(&state_path).expect("writer store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		false,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);

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
	let (_temp_dir, base_config, _workflow) = temp_project_layout();
	let missing_env_var = "DECODEX_TEST_MISSING_CONTROL_PLANE_LINEAR_API_KEY";
	let _env_lock = TestEnvVarGuard::lock();

	unsafe {
		env::remove_var(missing_env_var);
	}

	write_service_config(
		base_config.repo_root(),
		&sample_service_config_toml(
			base_config.service_id(),
			missing_env_var,
			base_config.github().token_env_var(),
			None,
			base_config.codex().review_level(),
		),
	);

	let config = load_service_config(base_config.repo_root());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
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
	let (temp_dir, config, _workflow) = temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
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
	let (temp_dir, config, _workflow) = temp_project_layout();
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
	let (temp_dir, config, _workflow) = temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("Todo", &[]);

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
	let (temp_dir, config, _workflow) = temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("Todo", &[]);
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

#[test]
fn control_plane_snapshot_aggregates_top_level_lanes_for_all_registered_projects() {
	let (active_temp_dir, active_config, _active_workflow) = temp_project_layout();
	let (_idle_temp_dir, idle_base_config, _idle_workflow) = temp_project_layout();
	let _home_guard = TestEnvVarGuard::set(
		"HOME",
		active_temp_dir.path().to_str().expect("home should be utf-8"),
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-active",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-04-30T03:01:00Z",
	);

	write_service_config(
		idle_base_config.repo_root(),
		&sample_service_config_toml("rsnap", "HOME", "HOME", None, ReviewLevel::Strict),
	);

	let idle_config = load_service_config(idle_base_config.repo_root());
	let active_registration = ProjectRegistration::from_config(
		active_config.service_id(),
		&service_config_path(active_config.repo_root()),
		&active_config,
		true,
		"active-fingerprint",
	);
	let idle_registration = ProjectRegistration::from_config(
		idle_config.service_id(),
		&service_config_path(idle_config.repo_root()),
		&idle_config,
		true,
		"idle-fingerprint",
	);

	state_store
		.record_run_attempt("run-active", &issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease(active_config.service_id(), &issue.id, "run-active", "In Progress")
		.expect("run lease should record");

	let active_snapshot =
		orchestrator::build_operator_status_snapshot(&active_config, &state_store, 10)
			.expect("active project snapshot should build");
	let idle_snapshot =
		orchestrator::build_operator_status_snapshot(&idle_config, &state_store, 10)
			.expect("idle project snapshot should build");
	let mut active_snapshot = Some(active_snapshot);
	let mut idle_snapshot = Some(idle_snapshot);
	let snapshot = orchestrator::collect_control_plane_snapshot(
		vec![active_registration, idle_registration],
		|project, _project_warnings| {
			let project_snapshot = match project.service_id() {
				"pubfi" => active_snapshot.take().expect("active snapshot should be used once"),
				"rsnap" => idle_snapshot.take().expect("idle snapshot should be used once"),
				service_id => panic!("unexpected project {service_id}"),
			};
			let project_status = project_snapshot
				.projects
				.first()
				.cloned()
				.map(|status| orchestrator::complete_project_status(project, status));

			ControlPlaneProjectTick { snapshot: Some(project_snapshot), project_status }
		},
	);
	let project_by_id = snapshot
		.projects
		.iter()
		.map(|project| (project.project_id.as_str(), project))
		.collect::<HashMap<_, _>>();

	assert_eq!(snapshot.project_id, "all");
	assert_eq!(snapshot.projects.len(), 2);
	assert_eq!(
		project_by_id.get("pubfi").expect("active project summary should exist").current_lane_count,
		1,
	);
	assert_eq!(
		project_by_id.get("rsnap").expect("idle project summary should exist").current_lane_count,
		0,
	);
	assert_eq!(snapshot.account_control.mode, "balanced");
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].run_id, "run-active");
	assert_eq!(snapshot.current_lanes[0].project_id, "pubfi");
	assert_eq!(snapshot.current_lanes[0].phase, "executing");
}

#[test]
fn status_cache_projects_aggregate_snapshot_to_requested_project() {
	let (active_temp_dir, active_config, _active_workflow) = temp_project_layout();
	let (_idle_temp_dir, idle_base_config, _idle_workflow) = temp_project_layout();
	let _home_guard = TestEnvVarGuard::set(
		"HOME",
		active_temp_dir.path().to_str().expect("home should be utf-8"),
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-active",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-04-30T03:01:00Z",
	);
	let worktree_path = active_config.worktree_root().join("PUB-101");

	write_service_config(
		idle_base_config.repo_root(),
		&sample_service_config_toml("rsnap", "HOME", "HOME", None, ReviewLevel::Strict),
	);

	let idle_config = load_service_config(idle_base_config.repo_root());
	let active_registration = ProjectRegistration::from_config(
		active_config.service_id(),
		&service_config_path(active_config.repo_root()),
		&active_config,
		true,
		"active-fingerprint",
	);
	let idle_registration = ProjectRegistration::from_config(
		idle_config.service_id(),
		&service_config_path(idle_config.repo_root()),
		&idle_config,
		true,
		"idle-fingerprint",
	);

	state_store
		.record_run_attempt("run-active", &issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease(active_config.service_id(), &issue.id, "run-active", "In Progress")
		.expect("run lease should record");
	state_store
		.upsert_worktree(
			active_config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("active worktree should record");

	let active_snapshot =
		orchestrator::build_operator_status_snapshot(&active_config, &state_store, 25)
			.expect("active project snapshot should build");
	let idle_snapshot =
		orchestrator::build_operator_status_snapshot(&idle_config, &state_store, 25)
			.expect("idle project snapshot should build");
	let mut active_snapshot = Some(active_snapshot);
	let mut idle_snapshot = Some(idle_snapshot);
	let aggregate = orchestrator::collect_control_plane_snapshot(
		vec![active_registration, idle_registration],
		|project, _project_warnings| {
			let project_snapshot = match project.service_id() {
				"pubfi" => active_snapshot.take().expect("active snapshot should be used once"),
				"rsnap" => idle_snapshot.take().expect("idle snapshot should be used once"),
				service_id => panic!("unexpected project {service_id}"),
			};
			let project_status = project_snapshot
				.projects
				.first()
				.cloned()
				.map(|status| orchestrator::complete_project_status(project, status));

			ControlPlaneProjectTick { snapshot: Some(project_snapshot), project_status }
		},
	);
	let cached = orchestrator::status_snapshot_from_operator_cache_response(
		&active_config,
		10,
		StatusSnapshotHttpResponse {
			body: serde_json::to_vec(&aggregate).expect("snapshot should serialize"),
			published_at_unix_epoch: Some(100),
		},
		105,
	)
	.expect("matching cached snapshot should project");

	assert_eq!(cached.project_id, "pubfi");
	assert_eq!(cached.run_limit, 10);
	assert_eq!(cached.status_source.as_deref(), Some("operator_snapshot_cache"));
	assert_eq!(cached.snapshot_age_seconds, Some(5));
	assert_eq!(cached.projects.len(), 1);
	assert_eq!(cached.projects[0].project_id, "pubfi");
	assert_eq!(cached.current_lanes.len(), 1);
	assert_eq!(cached.current_lanes[0].project_id, "pubfi");
	assert!(cached.worktrees.iter().all(|worktree| worktree.project_id == "pubfi"));
	assert!(cached.recent_runs.iter().all(|run| run.project_id == "pubfi"));
}

#[test]
fn status_cache_rejects_missing_stale_or_too_small_snapshot() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 25)
		.expect("project snapshot should build");
	let body = serde_json::to_vec(&snapshot).expect("snapshot should serialize");
	let missing_timestamp = orchestrator::status_snapshot_from_operator_cache_response(
		&config,
		10,
		StatusSnapshotHttpResponse { body: body.clone(), published_at_unix_epoch: None },
		105,
	)
	.expect_err("missing publish timestamp should fall back");

	assert!(missing_timestamp.reason.contains("omitted X-Decodex-Snapshot-Unix-Epoch"));

	let stale = orchestrator::status_snapshot_from_operator_cache_response(
		&config,
		10,
		StatusSnapshotHttpResponse { body: body.clone(), published_at_unix_epoch: Some(1) },
		1 + 61,
	)
	.expect_err("stale snapshot should fall back");

	assert!(stale.reason.contains("stale"));

	let too_small = orchestrator::status_snapshot_from_operator_cache_response(
		&config,
		100,
		StatusSnapshotHttpResponse { body, published_at_unix_epoch: Some(100) },
		101,
	)
	.expect_err("lower snapshot limit should fall back");

	assert!(too_small.reason.contains("run limit"));
}

#[test]
fn status_cache_is_bypassed_for_live_status() {
	assert!(orchestrator::status_should_attempt_operator_snapshot_cache(false));
	assert!(!orchestrator::status_should_attempt_operator_snapshot_cache(true));
}

#[test]
fn control_plane_snapshot_keeps_queued_project_summaries_service_scoped() {
	let (_decodex_temp_dir, decodex_base_config, _decodex_workflow) = temp_project_layout();
	let (_rsnap_temp_dir, rsnap_base_config, _rsnap_workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let decodex_registration = service_scoped_project_registration(&decodex_base_config, "decodex");
	let rsnap_registration = service_scoped_project_registration(&rsnap_base_config, "rsnap");
	let queued_issue = sample_issue_with_project_slug_and_sort_fields(
		"issue-decodex",
		"XY-403",
		"decodex",
		"Todo",
		&[],
		Some(1),
		"2026-05-01T03:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![queued_issue]);

	state_store.upsert_project(&decodex_registration).expect("decodex project should register");
	state_store.upsert_project(&rsnap_registration).expect("rsnap project should register");

	let registered_projects = state_store.list_projects().expect("registered projects should list");
	let snapshot = orchestrator::collect_control_plane_snapshot(
		registered_projects,
		|project, _project_warnings| {
			let config = ServiceConfig::from_path(project.config_path())
				.expect("project config should load");
			let workflow = WorkflowDocument::from_path(config.workflow_path())
				.expect("project workflow should load");
			let project_snapshot = orchestrator::build_operator_state_snapshot_for_publish(
				&tracker,
				&config,
				&workflow,
				&state_store,
				10,
				&[],
				&[],
			)
			.expect("project snapshot should build");
			let project_status = project_snapshot
				.projects
				.first()
				.cloned()
				.map(|status| orchestrator::complete_project_status(project, status));

			ControlPlaneProjectTick { snapshot: Some(project_snapshot), project_status }
		},
	);
	let project_by_id = snapshot
		.projects
		.iter()
		.map(|project| (project.project_id.as_str(), project))
		.collect::<HashMap<_, _>>();
	let decodex_project =
		project_by_id.get("decodex").expect("decodex project summary should exist");
	let rsnap_project = project_by_id.get("rsnap").expect("rsnap project summary should exist");

	assert_eq!(snapshot.project_id, "all");
	assert_eq!(snapshot.projects.len(), 2);
	assert_eq!(snapshot.queued_candidates.len(), 1);
	assert_eq!(snapshot.queued_candidates[0].issue_identifier, "XY-403");
	assert_eq!(decodex_project.queued_candidate_count, 1);
	assert_eq!(rsnap_project.queued_candidate_count, 0);
	assert_eq!(rsnap_project.waiting_lane_count, 0);
	assert_eq!(rsnap_project.attention_count, 0);

	let label_queries = tracker.label_queries.borrow().clone();

	assert_eq!(
		label_queries,
		vec![String::from("decodex:queued:decodex"), String::from("decodex:queued:rsnap"),],
	);
}

fn service_scoped_project_registration(
	base_config: &ServiceConfig,
	service_id: &str,
) -> ProjectRegistration {
	write_service_config(
		base_config.repo_root(),
		&sample_service_config_toml(service_id, "HOME", "HOME", None, ReviewLevel::Strict),
	);

	let config = load_service_config(base_config.repo_root());

	ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		&format!("{service_id}-fingerprint"),
	)
}
