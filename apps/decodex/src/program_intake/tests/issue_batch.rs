use tempfile::TempDir;

use crate::{
	program_intake::tests::test_support,
	program_intake::tests::test_support::{FakeTracker, TestIssueExt as _},
	program_intake::{self, IssueBatchIntakeClassification},
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
	assert_eq!(store.list_execution_programs("decodex").expect("programs").len(), 1);
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
