use tempfile::TempDir;

use crate::state::{
	ChildAgentActivityBucket, ChildAgentActivitySummary, ProtocolActivitySummary, StateStore,
};

#[test]
fn records_run_activity_summary_for_recent_project_runs() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let child_activity = ChildAgentActivitySummary {
		buckets: vec![ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 12,
			event_count: 3,
			tool_call_count: 0,
			input_tokens: 1_200,
			output_tokens: 240,
			output_bytes: 0,
		}],
		current_bucket: Some(String::from("Model")),
		current_detail: Some(String::from("gpt-5")),
		current_started_unix_epoch: None,
		current_elapsed_seconds: Some(12),
		wall_seconds: 12,
		event_count: 3,
		tool_call_count: 2,
		input_tokens_current: Some(1_200),
		input_tokens_max: Some(1_200),
		input_tokens_cumulative: 1_200,
		output_tokens_cumulative: 240,
		largest_tool_output_bytes: Some(4_096),
		largest_tool_output_tool: Some(String::from("shell")),
		large_output_warnings: vec![String::from("shell output was truncated")],
	};
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		..ProtocolActivitySummary::default()
	};
	let persisted_child_activity = child_activity.clone().sealed_durable();

	{
		let store = StateStore::open(&state_path).expect("persistent state store should open");

		store
			.record_run_attempt("run-1", "PUB-101", 1, "succeeded")
			.expect("run attempt should be recorded");
		store
			.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
			.expect("project ownership should record");
		store
			.record_run_activity_summary(
				"run-1",
				1,
				Some(&child_activity),
				Some(&protocol_activity),
			)
			.expect("activity summary should persist");
	}

	let reopened = StateStore::open(&state_path).expect("persistent state store should reopen");
	let runs = reopened.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert_eq!(runs[0].child_agent_activity(), Some(&persisted_child_activity));
	assert_eq!(runs[0].protocol_activity(), Some(&protocol_activity));
}
