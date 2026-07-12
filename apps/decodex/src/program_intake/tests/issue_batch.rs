use tempfile::TempDir;

use crate::{
	execution_program::ExecutionProgram,
	program_intake::{
		self, IssueBatchIntakeClassification,
		tests::{
			test_support,
			test_support::{FakeTracker, TestIssueExt as _},
		},
	},
	state::StateStore,
};

#[test]
fn issue_batch_dry_run_classifies_without_persisting() {
	let store = StateStore::open_in_memory().expect("store should open");
	let workflow = test_support::workflow();
	let config = test_support::test_config();
	let tracker = FakeTracker::default().with_issues([
		test_support::issue("XY-1", "Todo"),
		test_support::issue("XY-2", "In Progress"),
		test_support::issue("XY-3", "Done"),
		test_support::issue("XY-4", "Todo")
			.with_blocker("XY-20", "Todo")
			.with_blocker("XY-10", "Todo")
			.with_label("repo:zeta")
			.with_label("repo:alpha"),
	]);
	let report = program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![
			String::from("XY-4"),
			String::from("XY-2"),
			String::from("XY-404"),
			String::from("XY-1"),
			String::from("XY-3"),
		],
		true,
		false,
	)
	.expect("dry-run should classify");

	assert!(!report.persisted);
	assert!(!report.scheduler_visible);

	let rendered = program_intake::render_issue_batch_intake_report(&report);

	assert!(rendered.contains("persisted=false"));
	assert!(rendered.contains("scheduler_visible=false"));
	assert_eq!(report.counts.ready, 1);
	assert_eq!(report.counts.held, 1);
	assert_eq!(report.counts.blocked, 1);
	assert_eq!(report.counts.stale, 1);
	assert_eq!(report.counts.unmapped, 1);
	assert_eq!(report.issues[0].issue_identifier, "XY-1");
	assert_eq!(report.issues[0].classification, IssueBatchIntakeClassification::Ready);

	let blocked = report
		.issues
		.iter()
		.find(|issue| issue.issue_identifier == "XY-4")
		.expect("blocked issue should be reported");

	assert_eq!(blocked.blockers, vec![String::from("XY-10"), String::from("XY-20")]);
	assert_eq!(
		blocked.conflict_domains,
		vec![
			String::from("module:alpha"),
			String::from("module:zeta"),
			String::from("tracker_ownership:XY-4"),
		]
	);
	assert!(store.list_execution_programs("decodex").expect("program list should read").is_empty());
}

#[test]
fn issue_batch_dry_run_uses_runtime_context_for_dispatch_action() {
	let store = StateStore::open_in_memory().expect("store should open");
	let workflow = test_support::workflow();
	let config = test_support::test_config();
	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-1", "Todo")]);

	store
		.upsert_worktree("decodex", "id-XY-1", "x/decodex-xy-1", "/tmp/decodex/.worktrees/XY-1")
		.expect("retained worktree should record");

	let report = program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![String::from("XY-1")],
		true,
		false,
	)
	.expect("dry-run should classify with runtime context");

	assert_eq!(report.counts.ready, 0);
	assert_eq!(report.counts.held, 1);
	assert_eq!(report.issues[0].classification, IssueBatchIntakeClassification::Held);
	assert_eq!(report.issues[0].dispatch_action, None);
}

#[test]
fn issue_batch_dry_run_uses_runtime_conflict_occupancy() {
	let store = StateStore::open_in_memory().expect("store should open");
	let workflow = test_support::workflow();
	let config = test_support::test_config();
	let tracker = FakeTracker::default().with_issues([
		test_support::issue("XY-0", "In Progress").with_label("repo:alpha"),
		test_support::issue("XY-1", "Todo").with_label("repo:alpha"),
	]);

	program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![String::from("XY-0")],
		false,
		true,
	)
	.expect("persist should write occupied program");

	store
		.upsert_worktree("decodex", "id-XY-0", "x/decodex-xy-0", "/tmp/decodex/.worktrees/XY-0")
		.expect("retained worktree should record");

	let report = program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![String::from("XY-1")],
		true,
		false,
	)
	.expect("dry-run should classify with occupied conflict domains");

	assert_eq!(report.counts.ready, 0);
	assert_eq!(report.counts.blocked, 1);
	assert_eq!(report.issues[0].classification, IssueBatchIntakeClassification::Blocked);
	assert_eq!(report.issues[0].dispatch_action, None);
}

#[test]
fn project_registration_is_persist_only_for_command_path() {
	let store = StateStore::open_in_memory().expect("store should open");
	let temp_dir = TempDir::new().expect("temp dir should create");
	let config_path = test_support::write_project_files(temp_dir.path());

	program_intake::register_intake_project_config_for_persist(&store, &config_path, false)
		.expect("dry-run registration should no-op");

	assert!(store.list_projects().expect("projects should list").is_empty());

	program_intake::register_intake_project_config_for_persist(&store, &config_path, true)
		.expect("persist registration should write");

	let projects = store.list_projects().expect("projects should list");

	assert_eq!(projects.len(), 1);
	assert_eq!(projects[0].service_id(), "decodex");
	assert!(projects[0].enabled());
}

#[test]
fn issue_batch_persist_writes_program_and_adjacent_intake_state() {
	let store = StateStore::open_in_memory().expect("store should open");
	let workflow = test_support::workflow();
	let config = test_support::test_config();
	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-1", "Todo")]);
	let report = program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![String::from("XY-1")],
		false,
		true,
	)
	.expect("persist should write local state");

	assert!(report.persisted);
	assert!(report.scheduler_visible);

	let rendered = program_intake::render_issue_batch_intake_report(&report);

	assert!(rendered.contains("persisted=true"));
	assert!(rendered.contains("scheduler_visible=true"));
	assert_eq!(store.list_execution_programs("decodex").expect("programs").len(), 1);
	let authority = store
		.intake_authority_for_program("decodex", &report.program_id)
		.expect("authority read")
		.expect("authority");
	assert!(matches!(
		authority.authority(),
		crate::lane_authority::IntakeAuthorityKind::IssueBatch { .. }
	));
	assert_eq!(store.list_program_intake_plans("decodex").expect("plans").len(), 1);
	assert_eq!(
		store.list_program_issue_mappings("decodex", &report.program_id).expect("mappings").len(),
		1
	);
	assert_eq!(
		store.list_program_intake_plans("decodex").expect("plans")[0].intake_kind(),
		"issue_batch_intake"
	);
}

#[test]
fn issue_batch_reapply_keeps_stable_program_identity_and_removes_exact_legacy_duplicates() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("store should open");
	let workflow = test_support::workflow();
	let config = test_support::test_config();
	let issue = test_support::issue("XY-1", "Todo");
	let first = program_intake::run_issue_batch_intake(
		&store,
		&FakeTracker::default().with_issues([issue.clone()]),
		&config,
		&workflow,
		vec![String::from("XY-1")],
		false,
		true,
	)
	.expect("first persist should write local state");
	let first_program = store
		.execution_program("decodex", &first.program_id)
		.expect("program lookup should read")
		.expect("program should exist");
	let first_authority_fingerprint = store
		.intake_authority_for_program("decodex", &first.program_id)
		.expect("authority read")
		.expect("authority")
		.fingerprint()
		.to_owned();
	let legacy = ExecutionProgram::from_issue_batch_intake(
		"issue-batch-decodex-legacy",
		"decodex",
		"legacy-snapshot-fingerprint",
		"Legacy duplicate issue-batch intake.",
		first_program.program().nodes().to_vec(),
	)
	.expect("legacy duplicate should build");

	store.upsert_execution_program("decodex", legacy).expect("legacy duplicate should persist");

	assert_eq!(store.list_execution_programs("decodex").expect("programs").len(), 2);

	let mut refreshed_issue = issue;

	refreshed_issue.updated_at = String::from("2026-07-10T12:00:00Z");

	let reapplied = program_intake::run_issue_batch_intake(
		&store,
		&FakeTracker::default().with_issues([refreshed_issue]),
		&config,
		&workflow,
		vec![String::from("XY-1")],
		false,
		true,
	)
	.expect("reapply should replace exact duplicates");

	assert_eq!(reapplied.program_id, first.program_id);
	assert_eq!(
		store
			.intake_authority_for_program("decodex", &reapplied.program_id)
			.expect("authority read")
			.expect("authority")
			.fingerprint(),
		first_authority_fingerprint,
		"reapply must not rewrite accepted Intake Authority",
	);
	assert_eq!(store.list_execution_programs("decodex").expect("programs").len(), 1);
	assert!(
		store
			.execution_program("decodex", "issue-batch-decodex-legacy")
			.expect("legacy lookup should read")
			.is_none()
	);
	assert_eq!(store.list_program_intake_plans("decodex").expect("plans").len(), 1);

	drop(store);

	let reopened = StateStore::open(&state_path).expect("store should reopen");

	assert_eq!(reopened.list_execution_programs("decodex").expect("programs").len(), 1);
	let authority = reopened
		.intake_authority_for_program("decodex", &reapplied.program_id)
		.expect("authority read")
		.expect("authority should survive restart");
	authority.validate().expect("restarted authority should validate");
	assert!(
		reopened
			.execution_program("decodex", "issue-batch-decodex-legacy")
			.expect("legacy lookup should read")
			.is_none()
	);
}

#[test]
fn issue_batch_identity_survives_initially_unresolved_identifier() {
	let store = StateStore::open_in_memory().expect("store should open");
	let workflow = test_support::workflow();
	let config = test_support::test_config();
	let missing = program_intake::run_issue_batch_intake(
		&store,
		&FakeTracker::default(),
		&config,
		&workflow,
		vec![String::from("XY-404")],
		false,
		true,
	)
	.expect("unresolved intake should persist");
	let resolved = program_intake::run_issue_batch_intake(
		&store,
		&FakeTracker::default().with_issues([test_support::issue("XY-404", "Todo")]),
		&config,
		&workflow,
		vec![String::from("XY-404")],
		false,
		true,
	)
	.expect("resolved reapply should persist");

	assert_eq!(resolved.program_id, missing.program_id);
	assert_eq!(store.list_execution_programs("decodex").expect("programs").len(), 1);
	assert_eq!(
		store.list_program_issue_mappings("decodex", &resolved.program_id).expect("mappings").len(),
		1
	);
}
