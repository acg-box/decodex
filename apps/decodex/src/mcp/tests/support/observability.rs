use serde_json::Value;

pub(in crate::mcp::tests) fn sensitive_observability_fixture() -> Value {
	serde_json::json!({
		"schema": "decodex.operator.snapshot/1",
		"project": {
			"repoRoot": "/private/repo",
			"config_path": "/private/project.toml",
			"visible": "kept"
		},
		"runs": [
			{
				"issue": "XY-994",
				"effective_cwd": "/private/worktree",
				"private_evidence": {
					"read_command": "decodex evidence --config /private/project.toml --issue XY-994"
				},
				"github_cli_authority": {
					"github_command_path": "/private/bin/gh",
					"github_token_env_var": "GITHUB_PAT_Y"
				},
				"nested": {
					"readCommand": "decodex evidence --config /private/project.toml",
					"privateEvidenceRef": "private-ref",
					"safe": "kept"
				}
			}
		]
	})
}

pub(in crate::mcp::tests) fn observability_snapshot_fixture() -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.status_resource/1",
		"project_id": "decodex",
		"status_source": "live",
		"run_limit": 10,
		"current_lanes": [observability_current_lane_fixture()],
		"recent_runs": [observability_recent_run_fixture()],
		"post_review_lanes": [observability_post_review_lane_fixture()]
	})
}

pub(in crate::mcp::tests) fn observability_current_lane_fixture() -> Value {
	serde_json::json!({
		"run_id": "run-1",
		"issue_id": "issue-1",
		"issue_identifier": "XY-996",
		"attempt_number": 2,
		"status": "running",
		"attempt_status": "starting",
		"phase": "implementing",
		"run_phase": "implement_to_validation_ready",
		"wait_reason": "model_execution",
		"current_operation": "model_execution",
		"lane_control_next_action": "inspect_or_interrupt_orphaned_live_thread",
		"event_count": 6,
		"last_event_type": "turn/delta",
		"last_event_at": "2026-06-18T00:00:00Z",
		"last_protocol_activity_at": "2026-06-18T00:00:01Z",
		"last_progress_at": "2026-06-18T00:00:02Z",
		"progress_diagnostic": "protocol_only_activity",
		"suspected_stall": false,
		"protocol_activity": observability_protocol_activity_fixture(),
		"child_agent_activity": {
			"event_count": 2,
			"current_bucket": "protocol_activity",
			"path": "/private/activity-marker"
		},
		"phase_acceptance": observability_phase_acceptance_fixture(),
		"private_evidence": {
			"raw": "hidden"
		},
		"worktree_path": "/private/worktree"
	})
}

pub(in crate::mcp::tests) fn observability_protocol_activity_fixture() -> Value {
	serde_json::json!({
		"turn_status": "running",
		"waiting_reason": "model_execution",
		"recent_events": [
			{
				"event_type": "turn/delta",
				"category": "work_progress",
				"detail": "diff updated",
				"private_evidence": "private-ref"
			},
			{
				"event_type": "response/reasoning/summary",
				"category": "reasoning",
				"detail": "hidden chain of thought",
				"text": "private reasoning text",
				"summary": "private reasoning summary",
				"body": "private reasoning body"
			},
			{
				"event_type": "configWarning",
				"category": "warning",
				"detail": "config at /private/worktree using GITHUB_PAT_Y"
			},
			{
				"event_type": "error",
				"category": "protocol_error",
				"detail": "failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK"
			},
			{
				"event_type": "configWarning",
				"category": "warning",
				"detail": "state marker under /srv/decodex/runtime"
			},
			{
				"event_type": "error",
				"category": "protocol_error",
				"detail": "upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456"
			},
			{
				"event_type": "error",
				"category": "protocol_error",
				"detail": "upstream auth failed for 8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U"
			}
		]
	})
}

pub(in crate::mcp::tests) fn observability_phase_acceptance_fixture() -> Value {
	serde_json::json!({
		"phase": "handoff_evidence",
		"decision": "accepted",
		"reason_code": "phase_goal_satisfied",
		"objective_covered": true,
		"effective_delta_present": true,
		"changed_surfaces": ["phase-private-surface"],
		"non_goal_passed": true,
		"validation_passed": true,
		"recorded_at": "2026-06-18T00:00:03Z",
		"run_id": "phase-private-run",
		"attempt_number": 2,
		"next_action": "request_review"
	})
}

pub(in crate::mcp::tests) fn observability_review_status_fixture(
	head_sha: &str,
	active_fingerprint: &str,
	stop_fingerprint: &str,
	round: i64,
) -> Value {
	serde_json::json!({
		"phase": "handoff",
		"status": "pending",
		"checkpoint": {
			"head_sha": head_sha,
			"round": round,
			"nonclean_rounds": 2,
			"active_fingerprints": [active_fingerprint],
			"stop_fingerprint": stop_fingerprint,
			"updated_at": "2026-06-18T00:00:04Z"
		},
		"privateEvidenceRef": "private-review-ref"
	})
}

pub(in crate::mcp::tests) fn observability_recent_run_fixture() -> Value {
	serde_json::json!({
		"run_id": "run-1",
		"issue_id": "issue-1",
		"issue_identifier": "XY-996",
		"status": "running",
		"loop_status": {
			"review": {
				"status": "duplicate_recent"
			}
		}
	})
}

pub(in crate::mcp::tests) fn observability_post_review_lane_fixture() -> Value {
	serde_json::json!({
		"project_id": "decodex",
		"issue_id": "issue-1",
		"issue_identifier": "XY-996",
		"issue_state": "In Review",
		"branch_name": "private-branch-name",
		"worktree_path": "/private/review-worktree",
		"classification": "review_pending",
		"reason": "external_review_pending",
		"pr_url": "https://example/pr/1",
		"pr_head_sha": "private-pr-head",
		"pr_state": "OPEN",
		"review_state": "pending",
		"review_decision": "REVIEW_REQUIRED",
		"mergeable": "MERGEABLE",
		"check_state": "PENDING",
		"unresolved_review_threads": 1,
		"shadowed_by_current_lane": false,
		"readback_warning": "none",
		"readback_root_cause": "none",
		"loop_status": {
			"review": observability_review_status_fixture(
				"private-lane-head-sha",
				"lane-fingerprint-private",
				"lane-stop-fingerprint-private",
				4
			)
		},
		"private_evidence_ref": "private-pr-ref"
	})
}

pub(in crate::mcp::tests) fn assert_observability_is_sanitized(value: &Value) {
	let serialized = serde_json::to_string(value).expect("value should serialize");

	for sensitive in [
		"repoRoot",
		"config_path",
		"effective_cwd",
		"private_evidence",
		"privateEvidenceRef",
		"read_command",
		"readCommand",
		"github_cli_authority",
		"github_command_path",
		"github_token_env_var",
		"/private",
		"GITHUB_PAT_Y",
	] {
		assert!(!serialized.contains(sensitive), "sanitized value leaked {sensitive}");
	}

	assert!(serialized.contains("kept"));
}

pub(in crate::mcp::tests) fn assert_no_sensitive_observability_content(value: &Value) {
	let serialized = serde_json::to_string(value).expect("value should serialize");

	for sensitive in [
		"/private",
		"/Users/x",
		"private_evidence",
		"privateEvidenceRef",
		"private_evidence_ref",
		"private-ref",
		"private-review-ref",
		"private-pr-ref",
		"worktree_path",
		"worktreePath",
		"raw",
		"hidden chain of thought",
		"private reasoning text",
		"private reasoning summary",
		"private reasoning body",
		"GITHUB_PAT_Y",
		"LINEAR_API_KEY_HACKINK",
		"/srv/decodex/runtime",
		"ghp_abcdefghijklmnopqrstuvwxyz123456",
		"8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U",
		"active_fingerprints",
		"stop_fingerprint",
		"head_sha",
		"changed_surfaces",
		"recorded_at",
		"phase-private-surface",
		"phase-private-run",
		"private-head-sha",
		"fingerprint-private",
		"stop-fingerprint-private",
		"private-branch-name",
		"private-pr-head",
		"private-lane-head-sha",
		"lane-fingerprint-private",
		"lane-stop-fingerprint-private",
	] {
		assert!(!serialized.contains(sensitive), "sanitized value leaked {sensitive}");
	}
}
