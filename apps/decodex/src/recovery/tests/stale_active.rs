mod claim_labels;
mod evidence_blockers;
mod reentry;
mod release;
mod telemetry;

use std::{fs, path::Path};

use crate::{
	recovery::{
		STALE_ACTIVE_RECOVERY_SCHEMA, STALE_ACTIVE_RELEASE_EVENT,
		apply_stale_active_release_with_tracker, clear_stale_active_dead_run_claims_before_release,
		diagnose_stale_active_issues, ensure_stale_active_run_claim_guard,
		preflight_stale_active_worktree_cleanup, tests,
	},
	state::{
		self, ChildAgentActivitySummary, ProtocolActivityMarker, ProtocolActivitySummary,
		StateStore,
	},
	tracker::TrackerIssue,
};

fn dead_orphan_activity_summaries() -> (ChildAgentActivitySummary, ProtocolActivitySummary) {
	(
		ChildAgentActivitySummary {
			event_count: 531,
			current_bucket: Some(String::from("Model")),
			..ChildAgentActivitySummary::default()
		},
		ProtocolActivitySummary {
			turn_status: Some(String::from("running")),
			waiting_reason: Some(String::from("model_execution")),
			..ProtocolActivitySummary::default()
		},
	)
}

fn seed_dead_orphan_runtime_telemetry(
	store: &StateStore,
	issue: &TrackerIssue,
	worktree_path: &Path,
) {
	let control_channel_path = worktree_path.join(".decodex-run-control/run-1626-1.channel");
	let (child_activity, protocol_activity) = dead_orphan_activity_summaries();

	tests::init_clean_git_repo_with_remote_default(worktree_path, "x/pubfi-pub-1626");
	state::write_run_activity_marker_for_process(worktree_path, "run-1626", 1, u32::MAX)
		.expect("stale process marker should write");
	state::write_run_protocol_activity_marker(
		worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1626",
			attempt_number: 1,
			thread_id: Some("thread-stale"),
			turn_id: Some("turn-stale"),
			event_count: 531,
			last_event_type: "item/started",
			child_agent_activity: Some(&child_activity),
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("stale protocol marker should write");
	state::write_run_thread_status_marker(
		worktree_path,
		"run-1626",
		1,
		Some("thread-stale"),
		Some("turn-stale"),
		"active",
		&[],
	)
	.expect("stale thread marker should write");
	fs::create_dir_all(control_channel_path.parent().expect("channel parent"))
		.expect("control directory should create");
	fs::write(
		&control_channel_path,
		"schema=decodex.run_control_channel/v1\nrun_id=run-1626\nattempt_number=1\n",
	)
	.expect("control channel file should write");

	store.record_run_attempt("run-1626", &issue.id, 1, "running").expect("run attempt");
	store
		.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
		.expect("temporary lease should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.publish_run_control_channel_for_active_attempt(
			"run-1626",
			1,
			&control_channel_path,
			"local_file",
		)
		.expect("control channel should publish");
	store.clear_lease(&issue.id).expect("stale lane lease should clear");
	store
		.append_event("run-1626", 1, "item/started", r#"{"kind":"model"}"#)
		.expect("protocol event should record");
	store
		.record_run_activity_summary("run-1626", 1, Some(&child_activity), Some(&protocol_activity))
		.expect("activity summary should record");

	append_dead_orphan_private_telemetry(store, &issue.id);
}

fn seed_dead_orphan_runtime_telemetry_without_control_channel(
	store: &StateStore,
	issue: &TrackerIssue,
	worktree_path: &Path,
) {
	let (child_activity, protocol_activity) = dead_orphan_activity_summaries();

	state::write_run_activity_marker_for_process(worktree_path, "run-1626", 1, u32::MAX)
		.expect("stale process marker should write");
	state::write_run_protocol_activity_marker(
		worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1626",
			attempt_number: 1,
			thread_id: Some("thread-stale"),
			turn_id: Some("turn-stale"),
			event_count: 531,
			last_event_type: "item/started",
			child_agent_activity: Some(&child_activity),
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("stale protocol marker should write");
	state::write_run_thread_status_marker(
		worktree_path,
		"run-1626",
		1,
		Some("thread-stale"),
		Some("turn-stale"),
		"active",
		&[],
	)
	.expect("stale thread marker should write");

	store
		.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
		.expect("temporary lease should record");
	store.record_run_attempt("run-1626", &issue.id, 1, "running").expect("run attempt");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store.clear_lease(&issue.id).expect("stale lane lease should clear");
	store
		.append_event("run-1626", 1, "item/started", r#"{"kind":"model"}"#)
		.expect("protocol event should record");
	store
		.record_run_activity_summary("run-1626", 1, Some(&child_activity), Some(&protocol_activity))
		.expect("activity summary should record");

	append_dead_orphan_private_telemetry_without_control_channel_marker(store, &issue.id);
}

fn append_dead_orphan_private_telemetry(store: &StateStore, issue_id: &str) {
	append_dead_orphan_private_telemetry_events(store, issue_id, true);
}

fn append_dead_process_interrupt_control_telemetry(store: &StateStore, issue_id: &str) {
	for (event_type, payload) in [
		(
			"control_action",
			serde_json::json!({
				"action": "interrupt",
				"context": {
					"process_alive": false,
				},
				"outcome": "accepted",
				"reason": "run_lease_control_channel_resolved",
				"schema": "decodex.run_control_action/v1",
			}),
		),
		(
			"lane_control/interrupt/requested",
			serde_json::json!({
				"force": true,
				"method": "turn/interrupt",
				"source": "cli",
			}),
		),
		(
			"control_action",
			serde_json::json!({
				"action": "interrupt",
				"context": {
					"process_alive": false,
				},
				"outcome": "timed_out",
				"reason": "soft_interrupt_response_pending",
				"schema": "decodex.run_control_action/v1",
			}),
		),
		(
			"control_action",
			serde_json::json!({
				"action": "interrupt",
				"context": {
					"process_alive": false,
				},
				"outcome": "fallback",
				"reason": "hard_interrupt_fallback",
				"schema": "decodex.run_control_action/v1",
			}),
		),
	] {
		store
			.append_private_execution_event("pubfi", issue_id, "run-1626", 1, event_type, payload)
			.expect("private dead-process control telemetry should record");
	}
}

fn append_dead_orphan_private_telemetry_without_control_channel_marker(
	store: &StateStore,
	issue_id: &str,
) {
	append_dead_orphan_private_telemetry_events(store, issue_id, false);
}

fn append_dead_orphan_private_telemetry_events(
	store: &StateStore,
	issue_id: &str,
	include_control_channel_marker: bool,
) {
	let mut events = vec![
		(
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
			}),
		),
		(
			"progress_checkpoint",
			serde_json::json!({
				"phase": "probing",
				"pr_url": null,
				"verification": [],
				"head_sha": "1111111111111111111111111111111111111111",
			}),
		),
		(
			"control_action",
			serde_json::json!({
				"action": "interrupt",
				"reason": "run_lease_missing",
			}),
		),
		(
			"lane_control/interrupt",
			serde_json::json!({
				"classification": "hard_interrupt_fallback",
				"processAliveAfter": false,
				"signals": [],
				"status": "sent",
			}),
		),
	];

	if include_control_channel_marker {
		events.insert(
			0,
			(
				"control_channel_published",
				serde_json::json!({
					"schema": "decodex.run_control_channel/v1",
					"status": "active",
				}),
			),
		);
	}

	for (event_type, payload) in events {
		store
			.append_private_execution_event("pubfi", issue_id, "run-1626", 1, event_type, payload)
			.expect("private stale telemetry should record");
	}
}

fn append_app_server_no_progress_failure_evidence(store: &StateStore, issue_id: &str) {
	for (event_type, payload) in [
		(
			"loop_guardrail_checkpoint",
			serde_json::json!({
				"checkpoint_attempt_number": 1,
				"checkpoint_run_id": "run-1626",
				"consecutive_count": 1,
				"details": serde_json::json!({
					"branch_delta_present": false,
					"effective_delta_present": false,
					"reason": "no_effective_diff",
					"source_error_class": "app_server_turn_failed",
				})
				.to_string(),
				"fingerprint": "empty:empty",
				"reason": "no_effective_diff",
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"source_error_class": "app_server_turn_failed",
				"threshold": 3,
			}),
		),
		(
			"harness_outcome",
			serde_json::json!({
				"authority_boundary": {
					"dispositions": [],
					"failed_check_count": 0,
					"improvement_signal_count": 0,
				},
				"contracts": [],
				"execution_programs": [],
				"linear_projection": {
					"event_types": ["run_started"],
					"final_error_class": null,
					"final_event_type": "run_started",
					"final_terminal_path": null,
				},
				"manual_attention": null,
				"phase_goal_outcomes": [{
					"event_type": "phase_goal_set",
					"phase": "implement_to_validation_ready",
					"status": "active",
				}],
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": [],
				},
				"record_version": 1,
				"repair": {
					"attempt_number": 1,
					"repair_attempt_observed": false,
					"repair_phase_events": 0,
				},
				"review": {
					"accepted_finding_count": 0,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"attempt_number": 1,
					"issue_id": issue_id,
					"issue_identifier": "PUB-1626",
					"outcome": "retryable_failure",
					"project_id": "pubfi",
					"run_id": "run-1626",
					"source_intents": [],
				},
				"validation": {
					"failure_classes": [],
					"failure_count": 0,
					"result": "not_recorded",
				},
			}),
		),
	] {
		store
			.append_private_execution_event("pubfi", issue_id, "run-1626", 1, event_type, payload)
			.expect("private no-progress failure evidence should record");
	}
}

fn append_no_diff_guardrail_event(
	store: &StateStore,
	issue_id: &str,
	branch_delta_present: bool,
	effective_delta_present: bool,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"loop_guardrail_checkpoint",
			serde_json::json!({
				"details": serde_json::json!({
					"branch_delta_present": branch_delta_present,
					"effective_delta_present": effective_delta_present,
				})
				.to_string(),
				"reason": "no_effective_diff",
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"source_error_class": "app_server_turn_failed",
			}),
		)
		.expect("private guardrail evidence should record");
}

fn append_harness_outcome_with_pr_progress(store: &StateStore, issue_id: &str) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"harness_outcome",
			serde_json::json!({
				"manual_attention": null,
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": ["https://github.com/hack-ink/pubfi/pull/1631"],
				},
				"review": {
					"accepted_finding_count": 0,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"outcome": "retryable_failure",
				},
				"validation": {
					"failure_classes": [],
					"failure_count": 0,
					"result": "not_recorded",
				},
			}),
		)
		.expect("private harness progress evidence should record");
}

fn append_harness_outcome_with_review_progress(store: &StateStore, issue_id: &str) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"harness_outcome",
			serde_json::json!({
				"contracts": [],
				"execution_programs": [],
				"manual_attention": null,
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": [],
				},
				"review": {
					"accepted_finding_count": 1,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"outcome": "retryable_failure",
				},
				"validation": {
					"failure_classes": [],
					"failure_count": 0,
					"result": "not_recorded",
				},
			}),
		)
		.expect("private harness review progress evidence should record");
}

fn append_harness_outcome_with_validation_progress(store: &StateStore, issue_id: &str) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"harness_outcome",
			serde_json::json!({
				"contracts": [],
				"execution_programs": [],
				"manual_attention": null,
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": [],
				},
				"review": {
					"accepted_finding_count": 0,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"outcome": "retryable_failure",
				},
				"validation": {
					"failure_classes": ["repo_gate_verify_failed"],
					"failure_count": 1,
					"result": "failed",
				},
			}),
		)
		.expect("private harness validation progress evidence should record");
}

fn append_phase_goal_recovery_event(
	store: &StateStore,
	issue_id: &str,
	phase: &str,
	source_error_class: &str,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"phase_goal_recovery",
			serde_json::json!({
					"schema": "decodex.phase_goal_signal/1",
					"phase": phase,
					"signal": "phase_goal_recovered",
					"payload": {
						"nextPhase": "handoff_evidence",
					"sourceErrorClass": source_error_class,
					"sourceErrorMessage": "runtime failure",
				},
			}),
		)
		.expect("private phase goal recovery evidence should record");
}

fn append_stale_active_release_audit(store: &StateStore, issue_id: &str) {
	append_stale_active_release_audit_for_run(store, issue_id, "run-1626", 1);
}

fn append_stale_active_release_audit_for_run(
	store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			run_id,
			attempt_number,
			STALE_ACTIVE_RELEASE_EVENT,
			serde_json::json!({
				"schema": STALE_ACTIVE_RECOVERY_SCHEMA,
				"event": STALE_ACTIVE_RELEASE_EVENT,
				"phase": "local_cleanup_complete_before_active_label_release",
			}),
		)
		.expect("stale active release audit should record");
}
