use crate::orchestrator::agent_evidence::PrivateEvidenceReadback;

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn append_private_evidence_readback_header(
	output: &mut String,
	readback: &PrivateEvidenceReadback,
) {
	output.push_str(&format!("Project: {}\n", readback.project_id));
	output.push_str("Private Execution Evidence\n");
	output.push_str(&format!("issue_selector: {}\n", readback.issue_selector));
	output.push_str(&format!("issue_id: {}\n", readback.issue_id));
	output.push_str(&format!(
		"issue_identifier: {}\n",
		readback.issue_identifier.as_deref().unwrap_or("none")
	));
	output.push_str(&format!("run_id: {}\n", readback.run_id));
	output.push_str(&format!("attempt: {}\n", readback.attempt_number));
	output.push_str(&format!("source: {}\n", readback.source));
	output.push_str(&format!("evidence_ref: {}\n", readback.evidence_ref));
	output.push_str(&format!("payload_mode: {}\n", readback.payload_mode));
	output.push_str(&format!("event_count: {}\n", readback.event_count));
	output.push_str(&format!(
		"improvement_candidate_count: {}\n",
		readback.improvement_candidates.len()
	));
	output.push_str(&format!("decision_request_count: {}\n", readback.decision_requests.len()));
	output.push_str(&format!("review_checkpoint_count: {}\n", readback.review_checkpoints.len()));
	output.push_str(&format!(
		"architecture_recovery_count: {}\n",
		readback.architecture_recoveries.len()
	));
	output.push_str(&format!("boundary_check_count: {}\n", readback.boundary_checks.len()));
	output.push_str(&format!(
		"latest_event_type: {}\n",
		readback.latest_event_type.as_deref().unwrap_or("none")
	));
	output.push_str(&format!(
		"latest_event_at: {}\n",
		readback.latest_event_at.as_deref().unwrap_or("none")
	));

	if !readback.warnings.is_empty() {
		output.push_str(&format!("warnings: {}\n", readback.warnings.join(", ")));
	}
}
