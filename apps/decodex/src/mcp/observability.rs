use std::collections::BTreeSet;

use serde_json::{self, Value};

use super::{DEFAULT_MCP_STATUS_LIMIT, McpError};

pub(super) fn mcp_status_live_resource(snapshot: Value) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.status_live/1",
		"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
		"status_source": snapshot.get("status_source").cloned().unwrap_or(Value::Null),
		"run_limit": snapshot.get("run_limit").cloned().unwrap_or(Value::Null),
		"current_lanes": mcp_run_activity_summaries(snapshot.get("current_lanes")),
		"recent_runs": mcp_run_activity_summaries(snapshot.get("recent_runs")),
		"post_review_lanes": mcp_public_post_review_lanes(snapshot.get("post_review_lanes"))
	})
}

pub(super) fn mcp_activity_tail_resource(snapshot: Value) -> Value {
	let limit = snapshot
		.get("run_limit")
		.and_then(Value::as_u64)
		.and_then(|limit| usize::try_from(limit).ok())
		.unwrap_or(DEFAULT_MCP_STATUS_LIMIT);
	let mut activity = Vec::new();

	for run in mcp_all_runs(&snapshot).into_iter().take(limit) {
		activity.push(mcp_run_activity_summary(run));
	}

	serde_json::json!({
		"schema": "decodex.mcp.activity_tail/1",
		"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
		"activity": activity
	})
}

pub(super) fn mcp_public_lane_control_readback_resource(readback: Value) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.lane_control_readback/1",
		"project_id": readback.get("project_id").cloned().unwrap_or(Value::Null),
		"read_only": readback.get("read_only").cloned().unwrap_or(Value::Null),
		"mutating_tools": readback.get("mutating_tools").cloned().unwrap_or_else(|| serde_json::json!([])),
		"current_lanes": mcp_run_activity_summaries(readback.get("current_lanes")),
		"recent_runs": mcp_run_activity_summaries(readback.get("recent_runs")),
		"post_review_lanes": mcp_public_post_review_lanes(readback.get("post_review_lanes"))
	})
}

pub(super) fn mcp_public_lane_inspect_resource(report: Value) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.lane_inspect/1",
		"projectId": report.get("projectId").cloned().unwrap_or(Value::Null),
		"issue": report.get("issue").cloned().unwrap_or(Value::Null),
		"runId": report.get("runId").cloned().unwrap_or(Value::Null),
		"matchedRunCount": report.get("matchedRunCount").cloned().unwrap_or(Value::Null),
		"runs": mcp_public_lane_inspect_runs(report.get("runs"))
	})
}

fn mcp_public_lane_inspect_runs(runs: Option<&Value>) -> Vec<Value> {
	runs.and_then(Value::as_array).into_iter().flatten().map(mcp_public_lane_inspect_run).collect()
}

fn mcp_public_lane_inspect_run(run: &Value) -> Value {
	serde_json::json!({
		"projectId": run.get("projectId").cloned().unwrap_or(Value::Null),
		"issueId": run.get("issueId").cloned().unwrap_or(Value::Null),
		"issueIdentifier": run.get("issueIdentifier").cloned().unwrap_or(Value::Null),
		"runId": run.get("runId").cloned().unwrap_or(Value::Null),
		"attemptNumber": run.get("attemptNumber").cloned().unwrap_or(Value::Null),
		"status": run.get("status").cloned().unwrap_or(Value::Null),
		"attemptStatus": run.get("attemptStatus").cloned().unwrap_or(Value::Null),
		"phase": run.get("phase").cloned().unwrap_or(Value::Null),
		"waitReason": run.get("waitReason").cloned().unwrap_or(Value::Null),
		"currentOperation": run.get("currentOperation").cloned().unwrap_or(Value::Null),
		"laneControlNextAction": run
			.get("laneControlNextAction")
			.cloned()
			.unwrap_or(Value::Null),
		"laneControlConditions": run
			.get("laneControlConditions")
			.cloned()
			.unwrap_or_else(|| serde_json::json!([])),
		"lastEventType": run.get("lastEventType").cloned().unwrap_or(Value::Null),
		"lastEventAt": run.get("lastEventAt").cloned().unwrap_or(Value::Null),
		"eventCount": run.get("eventCount").cloned().unwrap_or(Value::Null)
	})
}

pub(super) fn mcp_run_resource(
	snapshot: &Value,
	run_id: &str,
	kind: &str,
) -> Result<Value, McpError> {
	let Some(run) = mcp_find_run(snapshot, run_id) else {
		return Err(McpError::resource_not_found());
	};
	let value = match kind {
		"events" => serde_json::json!({
			"schema": "decodex.mcp.run_events/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"issue_id": run.get("issue_id").cloned().unwrap_or(Value::Null),
			"issue_identifier": run.get("issue_identifier").cloned().unwrap_or(Value::Null),
			"event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
			"last_event_type": run.get("last_event_type").cloned().unwrap_or(Value::Null),
			"last_event_at": run.get("last_event_at").cloned().unwrap_or(Value::Null),
			"last_protocol_activity_at": run
				.get("last_protocol_activity_at")
				.cloned()
				.unwrap_or(Value::Null)
		}),
		"protocol_activity" => serde_json::json!({
			"schema": "decodex.mcp.protocol_activity/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"protocol_activity": mcp_public_protocol_activity(run),
			"event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
			"last_event_type": run.get("last_event_type").cloned().unwrap_or(Value::Null)
		}),
		"child_agent_activity" => serde_json::json!({
			"schema": "decodex.mcp.child_agent_activity/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"child_agent_activity": run.get("child_agent_activity").cloned().unwrap_or(Value::Null)
		}),
		"progress_diagnostics" => serde_json::json!({
			"schema": "decodex.mcp.progress_diagnostics/1",
			"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
			"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
			"phase": run.get("phase").cloned().unwrap_or(Value::Null),
			"run_phase": run.get("run_phase").cloned().unwrap_or(Value::Null),
			"current_operation": run.get("current_operation").cloned().unwrap_or(Value::Null),
			"last_progress_at": run.get("last_progress_at").cloned().unwrap_or(Value::Null),
			"progress_diagnostic": run.get("progress_diagnostic").cloned().unwrap_or(Value::Null),
			"suspected_stall": run.get("suspected_stall").cloned().unwrap_or(Value::Null)
		}),
		_ => unreachable!("MCP run resource kind is selected by static match arms"),
	};

	Ok(value)
}

pub(super) fn mcp_pr_review_state_resource(snapshot: Value) -> Value {
	let review_lanes = mcp_public_post_review_lanes(snapshot.get("post_review_lanes"));
	let current_lane_reviews = mcp_current_lane_runs(&snapshot)
		.into_iter()
		.filter_map(|run| {
			let review = mcp_loop_review_status(run)?;

			Some(serde_json::json!({
				"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
				"issue_id": run.get("issue_id").cloned().unwrap_or(Value::Null),
				"issue_identifier": run.get("issue_identifier").cloned().unwrap_or(Value::Null),
				"review": mcp_public_review_status(review)
			}))
		})
		.collect::<Vec<_>>();

	serde_json::json!({
		"schema": "decodex.mcp.pr_review_state/1",
		"project_id": snapshot.get("project_id").cloned().unwrap_or(Value::Null),
		"post_review_lanes": review_lanes,
		"current_lane_reviews": current_lane_reviews
	})
}

fn mcp_run_activity_summaries(runs: Option<&Value>) -> Vec<Value> {
	runs.and_then(Value::as_array).into_iter().flatten().map(mcp_run_activity_summary).collect()
}

fn mcp_public_post_review_lanes(lanes: Option<&Value>) -> Vec<Value> {
	lanes.and_then(Value::as_array).into_iter().flatten().map(mcp_public_post_review_lane).collect()
}

pub(super) fn mcp_public_post_review_lane(lane: &Value) -> Value {
	serde_json::json!({
		"project_id": lane.get("project_id").cloned().unwrap_or(Value::Null),
		"issue_id": lane.get("issue_id").cloned().unwrap_or(Value::Null),
		"issue_identifier": lane.get("issue_identifier").cloned().unwrap_or(Value::Null),
		"issue_state": lane.get("issue_state").cloned().unwrap_or(Value::Null),
		"classification": lane.get("classification").cloned().unwrap_or(Value::Null),
		"reason": lane.get("reason").cloned().unwrap_or(Value::Null),
		"pr_url": lane.get("pr_url").cloned().unwrap_or(Value::Null),
		"pr_state": lane.get("pr_state").cloned().unwrap_or(Value::Null),
		"review_decision": lane.get("review_decision").cloned().unwrap_or(Value::Null),
		"mergeable": lane.get("mergeable").cloned().unwrap_or(Value::Null),
		"check_state": lane.get("check_state").cloned().unwrap_or(Value::Null),
		"unresolved_review_threads": lane
			.get("unresolved_review_threads")
			.cloned()
			.unwrap_or(Value::Null),
		"shadowed_by_current_lane": lane
			.get("shadowed_by_current_lane")
			.cloned()
			.unwrap_or(Value::Null),
		"readback_warning": lane.get("readback_warning").cloned().unwrap_or(Value::Null),
		"readback_root_cause": lane.get("readback_root_cause").cloned().unwrap_or(Value::Null),
		"loop_review": lane
			.get("loop_status")
			.and_then(mcp_loop_review_status_from_loop_status)
			.map(mcp_public_review_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_all_runs(snapshot: &Value) -> Vec<&Value> {
	let mut runs = Vec::new();
	let mut seen_run_ids = BTreeSet::new();

	for key in ["current_lanes", "recent_runs"] {
		if let Some(items) = snapshot.get(key).and_then(Value::as_array) {
			for (index, run) in items.iter().enumerate() {
				let run_key = run
					.get("run_id")
					.and_then(Value::as_str)
					.map(str::to_owned)
					.unwrap_or_else(|| format!("{key}:{index}"));

				if seen_run_ids.insert(run_key) {
					runs.push(run);
				}
			}
		}
	}

	runs
}

fn mcp_current_lane_runs(snapshot: &Value) -> Vec<&Value> {
	snapshot.get("current_lanes").and_then(Value::as_array).into_iter().flatten().collect()
}

fn mcp_find_run<'a>(snapshot: &'a Value, run_id: &str) -> Option<&'a Value> {
	mcp_all_runs(snapshot)
		.into_iter()
		.find(|run| run.get("run_id").and_then(Value::as_str) == Some(run_id))
}

pub(super) fn mcp_run_activity_summary(run: &Value) -> Value {
	serde_json::json!({
		"run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
		"issue_id": run.get("issue_id").cloned().unwrap_or(Value::Null),
		"issue_identifier": run.get("issue_identifier").cloned().unwrap_or(Value::Null),
		"attempt_number": run.get("attempt_number").cloned().unwrap_or(Value::Null),
		"status": run.get("status").cloned().unwrap_or(Value::Null),
		"attempt_status": run.get("attempt_status").cloned().unwrap_or(Value::Null),
		"phase": run.get("phase").cloned().unwrap_or(Value::Null),
		"run_phase": run.get("run_phase").cloned().unwrap_or(Value::Null),
		"wait_reason": run.get("wait_reason").cloned().unwrap_or(Value::Null),
		"current_operation": run.get("current_operation").cloned().unwrap_or(Value::Null),
		"lane_control_next_action": run
			.get("lane_control_next_action")
			.cloned()
			.unwrap_or(Value::Null),
		"event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
		"last_event_type": run.get("last_event_type").cloned().unwrap_or(Value::Null),
		"last_event_at": run.get("last_event_at").cloned().unwrap_or(Value::Null),
		"last_protocol_activity_at": run
			.get("last_protocol_activity_at")
			.cloned()
			.unwrap_or(Value::Null),
		"last_progress_at": run.get("last_progress_at").cloned().unwrap_or(Value::Null),
		"protocol_activity": mcp_public_protocol_activity(run),
		"child_agent_activity": run.get("child_agent_activity").cloned().unwrap_or(Value::Null),
		"progress_diagnostic": run.get("progress_diagnostic").cloned().unwrap_or(Value::Null),
		"phase_acceptance": run
			.get("phase_acceptance")
			.map(mcp_public_phase_acceptance_status)
			.unwrap_or(Value::Null),
		"autonomy": mcp_public_autonomy_status(run),
		"loop_review": run
			.get("loop_status")
			.and_then(mcp_loop_review_status_from_loop_status)
			.map(mcp_public_review_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_public_autonomy_status(run_or_lane: &Value) -> Value {
	let Some(loop_status) = run_or_lane.get("loop_status").filter(|status| status.is_object())
	else {
		return Value::Null;
	};

	serde_json::json!({
		"status": loop_status.get("autonomy").cloned().unwrap_or(Value::Null),
		"summary": loop_status.get("summary").cloned().unwrap_or(Value::Null),
		"objective": loop_status
			.get("autonomy_objective")
			.map(mcp_public_autonomy_objective)
			.unwrap_or(Value::Null),
		"signals": mcp_public_autonomy_signals(loop_status.get("autonomy_signals")),
		"proposals": mcp_public_autonomy_proposals(loop_status.get("autonomy_proposals")),
		"lineage": mcp_public_autonomy_lineage(loop_status.get("autonomy_lineage")),
		"report": loop_status
			.get("autonomy_report")
			.map(mcp_public_autonomy_report)
			.unwrap_or(Value::Null)
	})
}

fn mcp_public_autonomy_objective(objective: &Value) -> Value {
	serde_json::json!({
		"objective_id": objective.get("objective_id").cloned().unwrap_or(Value::Null),
		"objective_version": objective
			.get("objective_version")
			.cloned()
			.unwrap_or(Value::Null),
		"state": objective.get("state").cloned().unwrap_or(Value::Null),
		"source_ref": objective.get("source_ref").cloned().unwrap_or(Value::Null),
		"completeness": objective.get("completeness").cloned().unwrap_or(Value::Null),
		"known_gaps": objective.get("known_gaps").cloned().unwrap_or_else(|| serde_json::json!([]))
	})
}

fn mcp_public_autonomy_signals(signals: Option<&Value>) -> Vec<Value> {
	signals
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.map(|signal| {
			let (source_refs, primary_source_refs) = mcp_public_autonomy_signal_refs(signal);

			serde_json::json!({
				"signal_id": signal.get("signal_id").cloned().unwrap_or(Value::Null),
				"objective_id": signal.get("objective_id").cloned().unwrap_or(Value::Null),
				"objective_version": signal.get("objective_version").cloned().unwrap_or(Value::Null),
				"kind": signal.get("kind").cloned().unwrap_or(Value::Null),
				"source_type": signal.get("source_type").cloned().unwrap_or(Value::Null),
				"source_refs": source_refs,
				"source_ref_count": signal_ref_count(signal, "source_refs", "source_ref_count"),
				"primary_source_refs": primary_source_refs,
				"primary_source_ref_count": signal_ref_count(
					signal,
					"primary_source_refs",
					"primary_source_ref_count"
				),
				"freshness": signal.get("freshness").cloned().unwrap_or(Value::Null),
				"evidence_class": signal.get("evidence_class").cloned().unwrap_or(Value::Null),
				"confidence": signal.get("confidence").cloned().unwrap_or(Value::Null),
				"redaction_level": signal
					.get("redaction_level")
					.cloned()
					.unwrap_or(Value::Null),
				"completeness": signal.get("completeness").cloned().unwrap_or(Value::Null),
				"known_gaps": signal
					.get("known_gaps")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"updated_at": signal.get("updated_at").cloned().unwrap_or(Value::Null)
			})
		})
		.collect()
}

fn mcp_public_autonomy_signal_refs(signal: &Value) -> (Value, Value) {
	if signal.get("redaction_level").and_then(Value::as_str) == Some("local_private") {
		return (serde_json::json!([]), serde_json::json!([]));
	}

	(
		signal.get("source_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
		signal.get("primary_source_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
	)
}

fn signal_ref_count(signal: &Value, refs_key: &str, count_key: &str) -> u64 {
	signal.get(count_key).and_then(Value::as_u64).unwrap_or_else(|| {
		signal.get(refs_key).and_then(Value::as_array).map_or(0, |refs| refs.len() as u64)
	})
}

fn mcp_public_autonomy_proposals(proposals: Option<&Value>) -> Vec<Value> {
	proposals
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.map(|proposal| {
			serde_json::json!({
				"proposal_id": proposal.get("proposal_id").cloned().unwrap_or(Value::Null),
				"objective_id": proposal.get("objective_id").cloned().unwrap_or(Value::Null),
				"objective_version": proposal.get("objective_version").cloned().unwrap_or(Value::Null),
				"state": proposal.get("state").cloned().unwrap_or(Value::Null),
				"summary": proposal.get("summary").cloned().unwrap_or(Value::Null),
				"source_family": proposal.get("source_family").cloned().unwrap_or(Value::Null),
				"intended_surface": proposal
					.get("intended_surface")
					.cloned()
					.unwrap_or(Value::Null),
				"source_signal_ids": proposal
					.get("source_signal_ids")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"refusal_reasons": proposal
					.get("refusal_reasons")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"refusals": proposal
					.get("refusals")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"completeness": proposal.get("completeness").cloned().unwrap_or(Value::Null),
				"known_gaps": proposal
					.get("known_gaps")
					.cloned()
					.unwrap_or_else(|| serde_json::json!([])),
				"updated_at": proposal.get("updated_at").cloned().unwrap_or(Value::Null)
			})
		})
		.collect()
}

fn mcp_public_autonomy_lineage(lineage: Option<&Value>) -> Vec<Value> {
	lineage.and_then(Value::as_array).into_iter().flatten().cloned().collect()
}

fn mcp_public_autonomy_report(report: &Value) -> Value {
	serde_json::json!({
		"surface": report.get("surface").cloned().unwrap_or(Value::Null),
		"authority": report.get("authority").cloned().unwrap_or(Value::Null),
		"audit_authority": report.get("audit_authority").cloned().unwrap_or(Value::Null),
		"source_refs": report.get("source_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
		"redaction_level": report.get("redaction_level").cloned().unwrap_or(Value::Null),
		"completeness": report.get("completeness").cloned().unwrap_or(Value::Null),
		"known_gaps": report.get("known_gaps").cloned().unwrap_or_else(|| serde_json::json!([]))
	})
}

fn mcp_public_phase_acceptance_status(acceptance: &Value) -> Value {
	serde_json::json!({
		"phase": acceptance.get("phase").cloned().unwrap_or(Value::Null),
		"decision": acceptance.get("decision").cloned().unwrap_or(Value::Null),
		"reason_code": acceptance.get("reason_code").cloned().unwrap_or(Value::Null),
		"objective_covered": acceptance.get("objective_covered").cloned().unwrap_or(Value::Null),
		"effective_delta_present": acceptance
			.get("effective_delta_present")
			.cloned()
			.unwrap_or(Value::Null),
		"non_goal_passed": acceptance.get("non_goal_passed").cloned().unwrap_or(Value::Null),
		"validation_passed": acceptance.get("validation_passed").cloned().unwrap_or(Value::Null),
		"next_action": acceptance.get("next_action").cloned().unwrap_or(Value::Null)
	})
}

fn mcp_public_review_status(review: &Value) -> Value {
	serde_json::json!({
		"phase": review.get("phase").cloned().unwrap_or(Value::Null),
		"status": review.get("status").cloned().unwrap_or(Value::Null),
		"checkpoint": review
			.get("checkpoint")
			.map(mcp_public_review_checkpoint_status)
			.unwrap_or(Value::Null)
	})
}

fn mcp_loop_review_status(run_or_lane: &Value) -> Option<&Value> {
	run_or_lane.get("loop_status").and_then(mcp_loop_review_status_from_loop_status)
}

fn mcp_loop_review_status_from_loop_status(loop_status: &Value) -> Option<&Value> {
	loop_status.get("review").filter(|review| review.is_object())
}

fn mcp_public_review_checkpoint_status(checkpoint: &Value) -> Value {
	serde_json::json!({
		"round": checkpoint.get("round").cloned().unwrap_or(Value::Null),
		"nonclean_rounds": checkpoint.get("nonclean_rounds").cloned().unwrap_or(Value::Null),
		"updated_at": checkpoint.get("updated_at").cloned().unwrap_or(Value::Null)
	})
}

fn mcp_public_protocol_activity(run: &Value) -> Value {
	let mut activity = run.get("protocol_activity").cloned().unwrap_or(Value::Null);

	redact_reasoning_protocol_activity(&mut activity);

	activity
}

fn redact_reasoning_protocol_activity(value: &mut Value) {
	match value {
		Value::Object(object) => {
			let is_reasoning_event = object
				.get("category")
				.and_then(Value::as_str)
				.is_some_and(|category| category.eq_ignore_ascii_case("reasoning"))
				|| object.get("event_type").and_then(Value::as_str).is_some_and(|event_type| {
					event_type.to_ascii_lowercase().contains("reasoning")
				});

			if is_reasoning_event {
				object.insert(
					String::from("detail"),
					Value::String(String::from("redacted_reasoning")),
				);
				object.remove("text");
				object.remove("summary");
				object.remove("content");
				object.remove("body");
			}

			for child in object.values_mut() {
				redact_reasoning_protocol_activity(child);
			}
		},
		Value::Array(items) =>
			for item in items {
				redact_reasoning_protocol_activity(item);
			},
		_ => {},
	}
}

pub(super) fn sanitize_mcp_observability_value(value: &mut Value) {
	match value {
		Value::Object(object) => {
			for key in [
				"worktreePath",
				"worktree_path",
				"channelPath",
				"channel_path",
				"requestPath",
				"request_path",
				"configPath",
				"config_path",
				"repoRoot",
				"repo_root",
				"effectiveCwd",
				"effective_cwd",
				"cwd",
				"privateEvidence",
				"private_evidence",
				"privateEvidenceRef",
				"private_evidence_ref",
				"privateEvidenceRefs",
				"private_evidence_refs",
				"executionProgramId",
				"execution_program_id",
				"executionProgramNodeIds",
				"execution_program_node_ids",
				"graphId",
				"graph_id",
				"nodeId",
				"node_id",
				"programId",
				"program_id",
				"readCommand",
				"read_command",
				"githubCliAuthority",
				"github_cli_authority",
				"githubCommandPath",
				"github_command_path",
				"ghCommandPath",
				"gh_command_path",
				"githubTokenEnvVar",
				"github_token_env_var",
				"path",
			] {
				object.remove(key);
			}
			for child in object.values_mut() {
				sanitize_mcp_observability_value(child);
			}
		},
		Value::String(text) =>
			if observability_string_contains_sensitive_text(text) {
				*text = String::from("redacted_sensitive_detail");
			},
		Value::Array(items) =>
			for item in items {
				sanitize_mcp_observability_value(item);
			},
		_ => {},
	}
}

pub(super) fn mcp_sanitized_value(mut value: Value) -> Value {
	sanitize_mcp_observability_value(&mut value);

	value
}

fn observability_string_contains_sensitive_text(value: &str) -> bool {
	let lower = value.to_ascii_lowercase();
	let upper = value.to_ascii_uppercase();

	lower.contains("/private")
		|| lower.contains("/users/")
		|| lower.contains("/var/folders/")
		|| lower.contains("/tmp/")
		|| lower.contains("file://")
		|| observability_string_contains_absolute_path(value)
		|| observability_string_contains_windows_path(value)
		|| observability_string_contains_secret_like_token(value)
		|| upper.contains("GITHUB_PAT_")
		|| upper.contains("LINEAR_API_KEY")
		|| upper.contains("OPENAI_API_KEY")
		|| lower.contains("authorization:")
		|| lower.contains("bearer ")
		|| lower.contains("token=")
		|| lower.contains("api_key")
}

fn observability_string_contains_absolute_path(value: &str) -> bool {
	let mut previous = None;
	let mut chars = value.char_indices().peekable();

	while let Some((index, character)) = chars.next() {
		if character != '/' {
			previous = Some(character);

			continue;
		}
		if previous == Some(':') || previous == Some('/') {
			previous = Some(character);

			continue;
		}

		let path_boundary = index == 0
			|| previous.is_some_and(|previous| {
				previous.is_whitespace()
					|| matches!(previous, '"' | '\'' | '`' | '(' | '[' | '{' | '=')
			});
		let path_component = chars
			.peek()
			.map(|(_, next)| next.is_ascii_alphanumeric() || matches!(next, '.' | '_' | '-'))
			.unwrap_or(false);

		if path_boundary && path_component {
			return true;
		}

		previous = Some(character);
	}

	false
}

fn observability_string_contains_windows_path(value: &str) -> bool {
	let bytes = value.as_bytes();

	bytes.windows(3).enumerate().any(|(index, window)| {
		let boundary = index == 0 || {
			let previous = bytes[index - 1];

			previous.is_ascii_whitespace()
				|| matches!(previous, b'"' | b'\'' | b'`' | b'(' | b'[' | b'{' | b'=')
		};

		boundary
			&& window[0].is_ascii_alphabetic()
			&& window[1] == b':'
			&& matches!(window[2], b'\\' | b'/')
	})
}

fn observability_string_contains_secret_like_token(value: &str) -> bool {
	value
		.split(|character: char| {
			!(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':' | '/'))
		})
		.any(|token| {
			let lower = token.to_ascii_lowercase();

			(lower.starts_with("ghp_") && token.len() >= 20)
				|| (lower.starts_with("github_pat_") && token.len() >= 20)
				|| (lower.starts_with("sk-") && token.len() >= 20)
				|| (lower.starts_with("sk-proj-") && token.len() >= 20)
				|| (lower.starts_with("xoxb-") && token.len() >= 20)
				|| (lower.starts_with("xoxp-") && token.len() >= 20)
				|| observability_token_looks_high_entropy_secret(token)
				|| observability_token_looks_like_jwt(token)
		})
}

fn observability_token_looks_high_entropy_secret(token: &str) -> bool {
	if token.len() < 32 || token.len() > 256 {
		return false;
	}
	if !token.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) {
		return false;
	}

	let mut has_lower = false;
	let mut has_upper = false;
	let mut digit_count = 0_usize;
	let mut seen = [false; 128];
	let mut unique_count = 0_usize;

	for byte in token.bytes() {
		has_lower |= byte.is_ascii_lowercase();
		has_upper |= byte.is_ascii_uppercase();

		if byte.is_ascii_digit() {
			digit_count += 1;
		}
		if byte.is_ascii() && !seen[byte as usize] {
			seen[byte as usize] = true;
			unique_count += 1;
		}
	}

	has_lower && has_upper && digit_count >= 4 && unique_count >= 16
}

fn observability_token_looks_like_jwt(token: &str) -> bool {
	let mut segments = token.split('.');
	let Some(header) = segments.next() else {
		return false;
	};
	let Some(payload) = segments.next() else {
		return false;
	};
	let Some(signature) = segments.next() else {
		return false;
	};

	segments.next().is_none()
		&& header.starts_with("eyJ")
		&& payload.len() >= 16
		&& signature.len() >= 16
}
