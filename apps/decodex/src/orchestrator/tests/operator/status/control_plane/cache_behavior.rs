use crate::orchestrator::{
	StatusSnapshotHttpResponse,
	tests::operator::status::{
		self, ControlPlaneProjectTick, FakeTracker, HashMap, ProjectRegistration, ReviewLevel,
		ServiceConfig, StateStore, WorkflowDocument, orchestrator,
	},
};

#[test]
fn status_cache_rejects_missing_stale_or_too_small_snapshot() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
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
	let (_decodex_temp_dir, decodex_base_config, _decodex_workflow) = status::temp_project_layout();
	let (_rsnap_temp_dir, rsnap_base_config, _rsnap_workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let decodex_registration = service_scoped_project_registration(&decodex_base_config, "decodex");
	let rsnap_registration = service_scoped_project_registration(&rsnap_base_config, "rsnap");
	let queued_issue = status::sample_issue_with_project_slug_and_sort_fields(
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
	status::write_service_config(
		base_config.repo_root(),
		&status::sample_service_config_toml(service_id, "HOME", "HOME", None, ReviewLevel::Strict),
	);

	let config = status::load_service_config(base_config.repo_root());

	ProjectRegistration::from_config(
		config.service_id(),
		&status::service_config_path(config.repo_root()),
		&config,
		true,
		&format!("{service_id}-fingerprint"),
	)
}
