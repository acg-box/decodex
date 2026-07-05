use tempfile::TempDir;

use crate::{
	state::{StateStore, tests::runtime_records::IN_PROGRESS_STATE},
	tracker::records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

#[test]
fn state_store_open_persists_runtime_history_across_instances() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.db");
	let first = StateStore::open(&state_path).expect("first state store should open");

	first
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should persist");
	first.record_run_attempt("run-1", "PUB-101", 1, "running").expect("run attempt should record");
	first.update_run_thread("run-1", "thread-1").expect("thread should persist");
	first.append_event("run-1", 1, "thread/run/created", "{}").expect("event should persist");
	first
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should persist");

	let mut ledger_record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-101",
			issue_identifier: "PUB-101",
			run_id: "run-1",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-04-29T10:10:00Z"),
		"closeout",
	);

	ledger_record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/101"));
	ledger_record.commit_sha = Some(String::from("1111111111111111111111111111111111111111"));
	ledger_record.summary = Some(String::from("Completed retained closeout."));

	first
		.record_linear_execution_event(&ledger_record)
		.expect("linear execution event should persist");

	assert!(state_path.exists(), "persistent runtime DB should be created");

	let second = StateStore::open(&state_path).expect("second state store should open");
	let latest = second
		.latest_run_attempt_for_issue("PUB-101")
		.expect("latest run lookup should succeed")
		.expect("persistent store should recover run history");

	assert_eq!(latest.run_id(), "run-1");
	assert_eq!(latest.thread_id(), Some("thread-1"));
	assert_eq!(second.event_count("run-1").expect("event count should load"), 1);
	assert!(
		second.lease_for_issue("PUB-101").expect("lease lookup should succeed").is_some(),
		"persistent store should recover run leases"
	);
	assert!(
		second.worktree_for_issue("PUB-101").expect("worktree lookup should succeed").is_some(),
		"persistent store should recover retained worktree mappings"
	);

	let ledger_records = second
		.list_linear_execution_events("pubfi", "PUB-101")
		.expect("linear execution events should load");

	assert_eq!(ledger_records, vec![ledger_record]);
}
