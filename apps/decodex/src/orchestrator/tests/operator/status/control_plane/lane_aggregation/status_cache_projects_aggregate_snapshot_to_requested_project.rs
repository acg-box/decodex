use crate::orchestrator::{
	StatusSnapshotHttpResponse,
	tests::operator::status::{
		self, ControlPlaneProjectTick, ProjectRegistration, ReviewLevel, StateStore,
		TestEnvVarGuard, orchestrator,
	},
};

#[test]
fn status_cache_projects_aggregate_snapshot_to_requested_project() {
	let (active_temp_dir, active_config, _active_workflow) = status::temp_project_layout();
	let (_idle_temp_dir, idle_base_config, _idle_workflow) = status::temp_project_layout();
	let _home_guard = TestEnvVarGuard::set(
		"HOME",
		active_temp_dir.path().to_str().expect("home should be utf-8"),
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-active",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-04-30T03:01:00Z",
	);
	let worktree_path = active_config.worktree_root().join("PUB-101");

	status::write_service_config(
		idle_base_config.repo_root(),
		&status::sample_service_config_toml("rsnap", "HOME", "HOME", None, ReviewLevel::Strict),
	);

	let idle_config = status::load_service_config(idle_base_config.repo_root());
	let active_registration = ProjectRegistration::from_config(
		active_config.service_id(),
		&status::service_config_path(active_config.repo_root()),
		&active_config,
		true,
		"active-fingerprint",
	);
	let idle_registration = ProjectRegistration::from_config(
		idle_config.service_id(),
		&status::service_config_path(idle_config.repo_root()),
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
				service_id => status::panic!("unexpected project {service_id}"),
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
