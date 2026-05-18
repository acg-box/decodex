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
	let snapshot = orchestrator::run_control_plane_tick(&state_store, &mut project_runtimes)
		.expect("control-plane snapshot should build");
	let project = snapshot.projects.first().expect("disabled project should be listed");

	assert_eq!(snapshot.project_id, "all");
	assert_eq!(snapshot.projects.len(), 1);
	assert_eq!(project.project_id, "pubfi");
	assert!(!project.enabled);
	assert_eq!(snapshot.account_control.mode, "balanced");
	assert_eq!(snapshot.account_control.account_selector, None);
	assert_eq!(project.connector_state, "disabled");
	assert_eq!(project.active_run_count, 0);
	assert_eq!(project.retained_worktree_count, 0);
	assert!(snapshot.warnings.contains(&String::from("no_enabled_projects")));
	assert!(project_runtimes.is_empty(), "disabled projects should not be ticked");
}

#[test]
fn control_plane_api_only_snapshot_does_not_tick_enabled_projects() {
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

	let snapshot = orchestrator::run_control_plane_api_only_tick(&state_store)
		.expect("api-only snapshot should build");
	let project = snapshot.projects.first().expect("enabled project should be listed");

	assert_eq!(snapshot.projects.len(), 1);
	assert_eq!(project.project_id, "pubfi");
	assert!(project.enabled);
	assert_eq!(project.connector_state, "api_only");
	assert_eq!(project.active_run_count, 0);
	assert_eq!(project.queued_candidate_count, 0);
	assert_eq!(project.warning_count, 1);
	assert!(snapshot.active_runs.is_empty());
	assert!(snapshot.queued_candidates.is_empty());
	assert!(snapshot.warnings.contains(&String::from("automation_disabled")));
	assert!(!snapshot.warnings.contains(&String::from("no_enabled_projects")));
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
		&sample_service_config_toml("rsnap", "HOME", "HOME", None, InternalReviewMode::Loop, true),
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
		.expect("active run should record");
	state_store
		.upsert_lease(active_config.service_id(), &issue.id, "run-active", "In Progress")
		.expect("active lease should record");

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

			ControlPlaneProjectTick {
				snapshot: Some(project_snapshot),
				project_status,
			}
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
		project_by_id.get("pubfi").expect("active project summary should exist").active_run_count,
		1,
	);
	assert_eq!(
		project_by_id.get("rsnap").expect("idle project summary should exist").active_run_count,
		0,
	);
	assert_eq!(snapshot.account_control.mode, "balanced");
	assert_eq!(snapshot.active_runs.len(), 1);
	assert_eq!(snapshot.active_runs[0].run_id, "run-active");
	assert_eq!(snapshot.active_runs[0].project_id, "pubfi");
	assert_eq!(snapshot.active_runs[0].phase, "executing");
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

			ControlPlaneProjectTick {
				snapshot: Some(project_snapshot),
				project_status,
			}
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
		&sample_service_config_toml(
			service_id,
			"HOME",
			"HOME",
			None,
			InternalReviewMode::Loop,
			true,
		),
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
