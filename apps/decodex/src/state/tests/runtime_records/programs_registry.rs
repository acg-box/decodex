use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

use crate::state::{ProjectRegistration, StateStore, tests};

#[test]
fn execution_programs_persist_reload_and_list_by_contract() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let mut contract = tests::latent_decision_contract_fixture();

	contract.promote(tests::sample_decision_promotion()).expect("contract should promote");

	let program = tests::sample_execution_program(&contract);
	let record = store
		.upsert_execution_program("decodex", program)
		.expect("execution program should persist");

	assert_eq!(record.project_id(), "decodex");
	assert_eq!(record.program_id(), "program-853");
	assert_eq!(record.source_contract_id(), Some("research-x-loop-contract"));
	assert_eq!(record.program().nodes().len(), 1);
	assert!(record.created_at_unix() > 0);
	assert!(record.updated_at_unix() >= record.created_at_unix());

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let reloaded = reopened
		.execution_program("decodex", "program-853")
		.expect("execution program should read")
		.expect("execution program should exist");

	assert_eq!(reloaded.created_at(), record.created_at());
	assert_eq!(reloaded.program().source_contract_id(), Some("research-x-loop-contract"));

	let contract_programs = reopened
		.list_execution_programs_for_contract("decodex", "research-x-loop-contract")
		.expect("contract programs should list");

	assert_eq!(contract_programs.len(), 1);
	assert_eq!(contract_programs[0].program_id(), "program-853");

	let project_programs =
		reopened.list_execution_programs("decodex").expect("project programs should list");

	assert_eq!(project_programs.len(), 1);
	assert_eq!(project_programs[0].program_id(), "program-853");

	let intake_plans =
		reopened.list_program_intake_plans("decodex").expect("program intake plans should list");

	assert_eq!(intake_plans.len(), 1);
	assert_eq!(intake_plans[0].program_id(), "program-853");
	assert_eq!(intake_plans[0].intake_kind(), "goal_intake");
	assert_eq!(intake_plans[0].source_contract_id(), Some("research-x-loop-contract"));

	let issue_mappings = reopened
		.list_program_issue_mappings("decodex", "program-853")
		.expect("program issue mappings should list");

	assert_eq!(issue_mappings.len(), 1);
	assert_eq!(issue_mappings[0].node_id(), "runtime-readiness");
	assert_eq!(issue_mappings[0].issue_identifier(), "XY-853");
	assert_eq!(issue_mappings[0].queue_intent(), "ready_to_queue");
	assert!(!issue_mappings[0].has_active_label());
}

#[test]
fn execution_program_reload_rejects_row_key_payload_mismatch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let mut contract = tests::latent_decision_contract_fixture();

	contract.promote(tests::sample_decision_promotion()).expect("contract should promote");
	store
		.upsert_execution_program("decodex", tests::sample_execution_program(&contract))
		.expect("execution program should persist");

	let connection = Connection::open(&state_path).expect("sqlite should open");
	let mut payload: Value = serde_json::from_str(
		&connection
			.query_row(
				"SELECT payload_json FROM execution_programs WHERE program_id = ?1",
				["program-853"],
				|row| row.get::<_, String>(0),
			)
			.expect("payload should load"),
	)
	.expect("payload should parse");

	payload["program_id"] = serde_json::json!("program-mismatch");

	connection
		.execute(
			"UPDATE execution_programs SET payload_json = ?1 WHERE program_id = ?2",
			[
				serde_json::to_string(&payload).expect("payload should serialize"),
				String::from("program-853"),
			],
		)
		.expect("payload should corrupt");

	assert!(
		StateStore::open(&state_path).is_err(),
		"execution program row key must match the versioned payload program_id"
	);
}

#[test]
fn decision_contract_reload_rejects_row_key_payload_mismatch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			tests::latent_decision_contract_fixture(),
		)
		.expect("latent decision contract should persist");

	let mut payload = serde_json::from_str::<Value>(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("fixture should parse as JSON");

	payload["contract_id"] = serde_json::json!("mismatched-contract-id");

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"UPDATE decision_contracts SET payload_json = ?1 WHERE contract_id = ?2",
			rusqlite::params![
				serde_json::to_string(&payload).expect("payload should serialize"),
				"research-x-loop-contract",
			],
		)
		.expect("decision contract row should corrupt for test");

	assert!(
		StateStore::open(&state_path).is_err(),
		"decision contract row key must match the versioned payload contract_id"
	);
}

#[test]
fn decision_contract_snapshot_load_quarantines_invalid_issue_dependency_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			tests::latent_decision_contract_fixture(),
		)
		.expect("valid decision contract should persist");

	let mut invalid_payload = serde_json::to_value(tests::latent_decision_contract_fixture())
		.expect("fixture should encode as JSON");

	invalid_payload["contract_id"] = serde_json::json!("invalid-dependency-contract");
	invalid_payload["execution_readiness"]["proposed_issues"][0]["dependencies"] =
		serde_json::json!(["XY-952"]);

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			rusqlite::params![
				"decodex",
				"invalid-dependency-contract",
				"XY-BROKEN",
				"draft_latent",
				serde_json::to_string(&invalid_payload)
					.expect("invalid dependency payload should serialize"),
				"2026-07-01T00:00:00Z",
				1_i64,
				"2026-07-01T00:00:00Z",
				1_i64,
			],
		)
		.expect("invalid dependency row should insert");

	let reopened =
		StateStore::open(&state_path).expect("invalid dependency contract should be quarantined");
	let valid_contract = reopened
		.decision_contract("decodex", "research-x-loop-contract")
		.expect("valid contract should remain readable")
		.expect("valid contract should exist");

	assert_eq!(valid_contract.contract_id(), "research-x-loop-contract");

	let project_contracts = reopened
		.list_decision_contracts_for_project("decodex")
		.expect("project contract list should skip invalid rows");

	assert_eq!(project_contracts.len(), 1);
	assert_eq!(project_contracts[0].contract_id(), "research-x-loop-contract");
	assert!(
		reopened
			.list_decision_contracts_for_issue("decodex", "XY-BROKEN")
			.expect("issue contract list should skip invalid rows")
			.is_empty()
	);
	assert!(
		reopened.decision_contract("decodex", "invalid-dependency-contract").is_err(),
		"direct reads of the invalid contract should still fail validation"
	);
}

#[test]
fn autonomy_proposal_snapshot_load_quarantines_fingerprint_mismatch_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let _store = StateStore::open(&state_path).expect("state store should open");
	let proposal = tests::autonomy_proposal_fixture();
	let mut invalid_payload =
		serde_json::to_value(&proposal).expect("proposal should encode as JSON");

	invalid_payload["affected_identifiers"] = serde_json::json!(["OperatorLoopStatus"]);

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
			rusqlite::params![
				"decodex",
				proposal.id(),
				proposal.objective_id(),
				1_i64,
				proposal.state().as_str(),
				proposal.fingerprint(),
				proposal.source_family(),
				proposal.intended_surface(),
				serde_json::to_string(&invalid_payload)
					.expect("invalid proposal payload should serialize"),
				"2026-07-01T00:00:00Z",
				1_i64,
				"2026-07-01T00:00:00Z",
				1_i64,
			],
		)
		.expect("invalid proposal row should insert");

	let reopened =
		StateStore::open(&state_path).expect("invalid proposal should be quarantined on open");

	assert!(
		reopened
			.recent_autonomy_proposals_for_project("decodex", 10)
			.expect("recent proposal list should skip invalid rows")
			.is_empty()
	);
	assert!(
		reopened.autonomy_proposal("decodex", proposal.id()).is_err(),
		"direct reads of the invalid proposal should still fail validation"
	);
}

#[test]
fn decision_contract_reload_migrates_removed_flat_issue_summary_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_decision_contract(
			"decodex",
			Some("XY-852"),
			tests::latent_decision_contract_fixture(),
		)
		.expect("current decision contract should persist");

	let mut removed_field_payload = serde_json::to_value(tests::latent_decision_contract_fixture())
		.expect("fixture should encode as JSON");

	removed_field_payload["contract_id"] = serde_json::json!("removed-flat-issue-contract");

	let readiness = removed_field_payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Flat summary that must be migrated."]),
	);
	readiness.insert(
		String::from("queue_intent"),
		serde_json::json!(["Removed queue intent that must not be re-admitted."]),
	);

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			rusqlite::params![
				"decodex",
				"removed-flat-issue-contract",
				"XY-OLD",
				"draft_latent",
				serde_json::to_string(&removed_field_payload)
					.expect("removed-field payload should serialize"),
				"2026-06-17T00:00:00Z",
				1_i64,
				"2026-06-17T00:00:00Z",
				1_i64,
			],
		)
		.expect("removed-field decision contract row should insert");
	connection
		.execute("UPDATE schema_meta SET value = '11' WHERE key = 'schema_version'", [])
		.expect("schema version should mark removed-field state");

	let reopened =
		StateStore::open(&state_path).expect("removed flat issue summary row should migrate");
	let project_contracts = reopened
		.list_decision_contracts_for_project("decodex")
		.expect("project contracts should list");
	let contract_ids =
		project_contracts.iter().map(|record| record.contract_id()).collect::<Vec<_>>();

	assert_eq!(project_contracts.len(), 2);
	assert!(contract_ids.contains(&"research-x-loop-contract"));
	assert!(contract_ids.contains(&"removed-flat-issue-contract"));

	let migrated_contract = reopened
		.decision_contract("decodex", "removed-flat-issue-contract")
		.expect("migrated contract read should succeed")
		.expect("migrated contract should exist");

	assert_eq!(migrated_contract.source_issue_id(), Some("XY-OLD"));
	assert_eq!(migrated_contract.contract().execution_readiness().proposed_issues().len(), 1);
	assert_eq!(
		migrated_contract.contract().execution_readiness().proposed_issues()[0].objective(),
		"Flat summary that must be migrated."
	);

	let migrated_payload: String = connection
		.query_row(
			"SELECT payload_json FROM decision_contracts WHERE contract_id = 'removed-flat-issue-contract'",
			[],
			|row| row.get(0),
		)
		.expect("migrated payload should read");
	let migrated_value: Value =
		serde_json::from_str(&migrated_payload).expect("migrated payload should parse");

	assert!(
		migrated_value.pointer("/execution_readiness/proposed_issue_summaries").is_none(),
		"removed field should be absent after migration"
	);
	assert!(
		migrated_value.pointer("/execution_readiness/queue_intent").is_none(),
		"removed queue intent should be absent after migration"
	);
}

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

	let removed = store.remove_project("vibe-mono").expect("project should remove");

	assert_eq!(removed.service_id(), "vibe-mono");
	assert!(store.list_projects().expect("projects should list").is_empty());

	let reopened = StateStore::open(&state_path).expect("state store should reopen");

	assert!(
		reopened.list_projects().expect("project registry should load").is_empty(),
		"removed project must not remain in SQLite registry"
	);
}
