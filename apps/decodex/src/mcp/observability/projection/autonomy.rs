use serde_json::{self, Value};

pub(super) fn mcp_public_autonomy_status(run_or_lane: &Value) -> Value {
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
