use tempfile::TempDir;

use crate::state::{AutonomyRuntimePolicyRecord, ProjectRegistration, StateStore};

#[test]
fn state_store_open_refreshes_pubfi_project_registry_across_instances() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.db");
	let initial_config_path = temp_dir.path().join("stale/project.toml");
	let initial_repo_root = temp_dir.path().join("stale/repo");
	let initial_worktree_root = temp_dir.path().join("stale/repo/.worktrees");
	let initial_workflow_path = temp_dir.path().join("stale/repo/WORKFLOW.md");
	let refreshed_config_path = temp_dir.path().join("current/project.toml");
	let refreshed_repo_root = temp_dir.path().join("current/repo");
	let refreshed_worktree_root = temp_dir.path().join("current/repo/.worktrees");
	let refreshed_workflow_path = temp_dir.path().join("current/repo/WORKFLOW.md");
	let store = StateStore::open(&state_path).expect("state store should open");
	let registration = ProjectRegistration {
		service_id: String::from("pubfi"),
		config_path: initial_config_path,
		repo_root: initial_repo_root,
		worktree_root: initial_worktree_root,
		workflow_path: initial_workflow_path,
		tracker_api_key_env_var: String::from("LINEAR_API_KEY_HACKINK"),
		github_token_env_var: String::from("GITHUB_PAT_Y"),
		enabled: true,
		config_fingerprint: String::from("abc123"),
		updated_at: String::from("2026-04-29T00:00:00Z"),
		updated_at_unix: 1_777_392_000,
	};
	let refreshed_registration = ProjectRegistration {
		service_id: String::from("pubfi"),
		config_path: refreshed_config_path.clone(),
		repo_root: refreshed_repo_root.clone(),
		worktree_root: refreshed_worktree_root.clone(),
		workflow_path: refreshed_workflow_path.clone(),
		tracker_api_key_env_var: String::from("LINEAR_API_KEY_HACKINK"),
		github_token_env_var: String::from("GITHUB_PAT_Y"),
		enabled: true,
		config_fingerprint: String::from("def456"),
		updated_at: String::from("2026-04-30T00:00:00Z"),
		updated_at_unix: 1_777_478_400,
	};

	store.upsert_project(&registration).expect("project should persist");
	store.set_project_enabled("pubfi", false).expect("project should disable");
	store.upsert_project(&refreshed_registration).expect("project should refresh");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let projects = reopened.list_projects().expect("project registry should load");

	assert_eq!(projects.len(), 1, "pubfi refresh should keep one scoped registry row");

	let project = &projects[0];

	assert_eq!(
		project.service_id(),
		"pubfi",
		"pubfi refresh should stay scoped to the same service id"
	);
	assert!(!project.enabled(), "pubfi refresh should preserve the existing disabled state");
	assert_eq!(
		project.config_fingerprint(),
		"def456",
		"pubfi refresh should replace the stale config fingerprint"
	);
	assert_eq!(
		project.config_path(),
		refreshed_config_path.as_path(),
		"pubfi refresh should replace the stale config path"
	);
	assert_eq!(
		project.repo_root(),
		refreshed_repo_root.as_path(),
		"pubfi refresh should replace the stale repo root"
	);
	assert_eq!(
		project.worktree_root(),
		refreshed_worktree_root.as_path(),
		"pubfi refresh should replace the stale worktree root"
	);
	assert_eq!(
		project.workflow_path(),
		refreshed_workflow_path.as_path(),
		"pubfi refresh should replace the stale workflow path"
	);
}

#[test]
fn lazy_project_registry_refresh_preserves_runtime_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.db");
	let full_store = StateStore::open(&state_path).expect("state store should open");
	let registration = ProjectRegistration {
		service_id: String::from("pubfi"),
		config_path: temp_dir.path().join("project.toml"),
		repo_root: temp_dir.path().join("repo"),
		worktree_root: temp_dir.path().join("repo/.worktrees"),
		workflow_path: temp_dir.path().join("repo/WORKFLOW.md"),
		tracker_api_key_env_var: String::from("LINEAR_API_KEY_HACKINK"),
		github_token_env_var: String::from("GITHUB_PAT_Y"),
		enabled: true,
		config_fingerprint: String::from("abc123"),
		updated_at: String::from("2026-04-29T00:00:00Z"),
		updated_at_unix: 1_777_392_000,
	};
	let refreshed_registration = ProjectRegistration {
		config_fingerprint: String::from("def456"),
		updated_at: String::from("2026-04-30T00:00:00Z"),
		updated_at_unix: 1_777_478_400,
		..registration.clone()
	};

	full_store.upsert_project(&registration).expect("project should persist");
	full_store.record_run_attempt("run-1", "PUB-101", 1, "running").expect("run should record");
	full_store
		.append_event("run-1", 1, "item/agentMessage/delta", "{}")
		.expect("event should append");
	full_store
		.upsert_worktree(
			"pubfi",
			"PUB-101",
			"x/pub-101",
			temp_dir.path().join("repo/.worktrees/PUB-101").to_string_lossy().as_ref(),
		)
		.expect("worktree should persist");

	let lazy_store = StateStore::open_lazy(&state_path).expect("lazy state store should open");

	lazy_store.upsert_project(&refreshed_registration).expect("project should refresh");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let attempt = reopened
		.latest_run_attempt_for_issue("PUB-101")
		.expect("attempt lookup should succeed")
		.expect("attempt should survive lazy project refresh");
	let mapping = reopened
		.worktree_for_issue("PUB-101")
		.expect("worktree lookup should succeed")
		.expect("worktree should survive lazy project refresh");

	assert_eq!(attempt.run_id(), "run-1");
	assert_eq!(reopened.event_count("run-1").expect("event count should survive"), 1);
	assert_eq!(mapping.project_id(), "pubfi");
	assert_eq!(
		reopened.list_projects().expect("project registry should load")[0].config_fingerprint(),
		"def456"
	);
}

#[test]
fn remove_project_deletes_persistent_registry_row() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.db");
	let store = StateStore::open(&state_path).expect("state store should open");
	let registration = ProjectRegistration {
		service_id: String::from("vibe-mono"),
		config_path: temp_dir.path().join("project.toml"),
		repo_root: temp_dir.path().join("repo"),
		worktree_root: temp_dir.path().join("repo/.worktrees"),
		workflow_path: temp_dir.path().join("repo/WORKFLOW.md"),
		tracker_api_key_env_var: String::from("LINEAR_API_KEY_HACKINK"),
		github_token_env_var: String::from("GITHUB_PAT_Y"),
		enabled: true,
		config_fingerprint: String::from("abc123"),
		updated_at: String::from("2026-05-25T00:00:00Z"),
		updated_at_unix: 1_779_667_200,
	};

	store.upsert_project(&registration).expect("project should persist");
	store
		.accept_autonomy_runtime_policy(
			AutonomyRuntimePolicyRecord::new(
				"vibe-mono",
				"policy",
				"1",
				"objective",
				1,
				"sha256:objective",
				"policy:1",
				"operator",
				"2026-07-10T12:00:00Z",
				"test",
				vec![String::from("No bypass.")],
			)
			.expect("policy should validate"),
		)
		.expect("policy should persist");
	store
		.begin_program_intake_attempt("vibe-mono", "contract-1", "digest-1")
		.expect("intake attempt should persist");

	let removed = store.remove_project("vibe-mono").expect("project should remove");

	assert_eq!(removed.service_id(), "vibe-mono");
	assert!(store.list_projects().expect("projects should list").is_empty());

	let reopened = StateStore::open(&state_path).expect("state store should reopen");

	assert!(
		reopened.list_projects().expect("project registry should load").is_empty(),
		"removed project must not remain in SQLite registry"
	);
	assert!(
		reopened
			.autonomy_runtime_policy("vibe-mono", "policy", "1")
			.expect("policy lookup should work")
			.is_none(),
		"project removal must delete accepted runtime policy authority"
	);
	assert_eq!(
		reopened
			.program_intake_attempt_status("vibe-mono", "contract-1")
			.expect("attempt lookup should work"),
		None,
		"project removal must delete stale Program Intake claims"
	);
}
