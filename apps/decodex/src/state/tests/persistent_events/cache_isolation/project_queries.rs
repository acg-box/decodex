use tempfile::TempDir;

use crate::{
	state::{ProjectRegistration, StateStore, tests::IN_PROGRESS_STATE},
	tracker::records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

#[test]
fn persistent_project_run_listing_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");

	observer
		.record_run_attempt("run-a", "PUB-101", 1, "running")
		.expect("observer run should record");
	observer
		.upsert_lease("pubfi", "PUB-101", "run-a", IN_PROGRESS_STATE)
		.expect("observer lease should record");
	observer
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("observer worktree should record");
	observer.append_event("run-a", 1, "item/started", "{}").expect("observer event should append");
	writer.record_run_attempt("run-b", "PUB-102", 1, "running").expect("writer run should record");
	writer
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("writer event should append");
	writer
		.record_run_attempt("run-c", "PUB-103", 1, "succeeded")
		.expect("writer project run should record");
	writer
		.upsert_worktree("pubfi", "PUB-103", "x/pubfi-pub-103", "/tmp/worktrees/pub-103")
		.expect("writer project worktree should persist");
	writer
		.append_event("run-c", 1, "thread/archive", "{}")
		.expect("writer project event should append");

	let mut writer_record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-102",
			issue_identifier: "PUB-102",
			run_id: "run-b",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-04-29T10:12:00Z"),
		"closeout",
	);

	writer_record.summary = Some(String::from("Writer closeout."));
	writer_record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/102"));
	writer_record.commit_sha = Some(String::from("2222222222222222222222222222222222222222"));

	writer
		.record_linear_execution_event(&writer_record)
		.expect("writer ledger event should persist");

	let runs = observer.list_leased_runs("pubfi").expect("leased runs should load");
	let recent_runs = observer.list_recent_runs("pubfi", 10).expect("recent runs should load");
	let leases = observer.list_active_shared_leases("pubfi").expect("shared leases should load");
	let worktrees = observer.list_worktrees("pubfi").expect("worktrees should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-a");
	assert_eq!(runs[0].event_count(), 1);
	assert_eq!(runs[0].last_event_type(), Some("item/started"));
	assert!(
		recent_runs.iter().any(|run| run.run_id() == "run-c"
			&& run.event_count() == 1
			&& run.last_event_type() == Some("thread/archive")),
		"project-scoped persistent event summaries should still load for matching runs"
	);
	assert_eq!(leases.len(), 1);
	assert_eq!(leases[0].issue_id(), "PUB-101");
	assert_eq!(worktrees.len(), 2);
	assert_eq!(worktrees[0].issue_id(), "PUB-101");

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"operator run listing should refresh event summaries without materializing unrelated event rows"
	);
	assert!(
		!state.events.contains_key("run-c"),
		"operator run listing should refresh project summaries without materializing project event rows"
	);
	assert!(
		!state.event_summaries.contains_key("run-b"),
		"operator run listing should not refresh summaries for runs outside the requested project"
	);
	assert!(
		!state.linear_execution_events.contains_key(&writer_record.idempotency_key),
		"operator run and worktree listing should not refresh the full persistent ledger into the local cache"
	);
}

#[test]
fn persistent_project_listing_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
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
		updated_at: String::from("2026-05-25T00:00:00Z"),
		updated_at_unix: 1_779_667_200,
	};

	observer.upsert_project(&registration).expect("project should persist");
	writer.record_run_attempt("run-b", "PUB-102", 1, "running").expect("writer run should record");
	writer
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("writer event should append");

	let projects = observer.list_projects().expect("projects should load");

	assert_eq!(projects.len(), 1);
	assert_eq!(projects[0].service_id(), "pubfi");

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"project listing should not refresh the full persistent event journal into the local cache"
	);
	assert!(
		!state.event_summaries.contains_key("run-b"),
		"project listing should not refresh protocol summaries unrelated to the registry"
	);
}
