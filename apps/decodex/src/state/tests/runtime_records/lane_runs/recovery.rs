use crate::state::{ChildAgentActivityBucket, ChildAgentActivitySummary, StateStore};

#[test]
fn lists_project_issue_runs_recovered_from_local_evidence() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");
	let activity = ChildAgentActivitySummary {
		buckets: vec![ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 120,
			event_count: 2,
			tool_call_count: 1,
			input_tokens: 400,
			output_tokens: 80,
			..ChildAgentActivityBucket::default()
		}],
		wall_seconds: 120,
		event_count: 2,
		tool_call_count: 1,
		input_tokens_cumulative: 400,
		output_tokens_cumulative: 80,
		..ChildAgentActivitySummary::default()
	};

	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should record");
	store
		.record_run_activity_summary("run-recovered", 1, Some(&activity), None)
		.expect("activity summary should record");
	store
		.append_event("run-recovered", 1, "turn/completed", "{}")
		.expect("protocol event should record");
	store
		.append_private_execution_event(
			"pubfi",
			"PUB-101",
			"run-recovered",
			1,
			"issue_progress_checkpoint",
			serde_json::json!({ "source": "test" }),
		)
		.expect("private execution evidence should record");

	let runs = store.list_project_issue_runs("pubfi", "PUB-101").expect("issue runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-recovered");
	assert_eq!(runs[0].attempt_number(), 1);
	assert_eq!(runs[0].status(), "recovered");
	assert_eq!(runs[0].recovery_source(), "recovered");
	assert!(
		runs[0]
			.recovery_evidence()
			.iter()
			.any(|evidence| evidence == "private_execution_event:issue_progress_checkpoint")
	);
	assert!(runs[0].recovery_evidence().iter().any(|evidence| evidence == "run_activity_summary"));
	assert!(runs[0].recovery_evidence().iter().any(|evidence| evidence == "protocol_events:1"));
	assert!(runs[0].recovery_gaps().is_empty());
	assert_eq!(runs[0].event_count(), 1);
	assert_eq!(runs[0].child_agent_activity().expect("activity should recover").event_count, 2);
}
