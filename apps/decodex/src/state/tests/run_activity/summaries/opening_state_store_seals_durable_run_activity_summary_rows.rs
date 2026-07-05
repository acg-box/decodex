use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

use crate::state::{ChildAgentActivityBucket, ChildAgentActivitySummary, StateStore};

#[test]
fn opening_state_store_seals_durable_run_activity_summary_rows() {
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
		current_started_unix_epoch: Some(10),
		current_elapsed_seconds: Some(8),
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
	let unsealed_json =
		serde_json::to_string(&child_activity).expect("unsealed activity should serialize");

	StateStore::open(&state_path).expect("persistent state store should bootstrap");

	{
		let connection = Connection::open(&state_path).expect("sqlite connection should reopen");

		connection
			.execute(
				"INSERT INTO run_activity_summaries (
				 run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
				 updated_at, updated_at_unix
				 ) VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
				rusqlite::params!["run-old", 1_i64, unsealed_json, "2026-06-17T00:00:00Z", 1_i64],
			)
			.expect("unsealed activity row should insert");
	}

	StateStore::open(&state_path).expect("persistent state store should seal stored row");

	let sealed_json: String = Connection::open(&state_path)
		.expect("sqlite connection should reopen")
		.query_row(
			"SELECT child_agent_activity_json FROM run_activity_summaries WHERE run_id = ?1",
			["run-old"],
			|row| row.get(0),
		)
		.expect("sealed row should load");
	let sealed_value: Value =
		serde_json::from_str(&sealed_json).expect("sealed activity should remain json");
	let sealed_activity: ChildAgentActivitySummary =
		serde_json::from_str(&sealed_json).expect("sealed activity should deserialize");

	assert!(sealed_value["current_bucket"].is_null());
	assert!(sealed_value["current_detail"].is_null());
	assert!(sealed_value["current_started_unix_epoch"].is_null());
	assert!(sealed_value["current_elapsed_seconds"].is_null());
	assert_eq!(sealed_activity, child_activity.sealed_durable());
}
