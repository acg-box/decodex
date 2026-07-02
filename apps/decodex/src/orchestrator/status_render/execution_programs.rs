use crate::orchestrator::OperatorStatusSnapshot;

pub(super) fn append_rendered_execution_programs(
	output: &mut String,
	snapshot: &OperatorStatusSnapshot,
) {
	output.push_str("\nExecution Programs\n");

	if snapshot.execution_programs.is_empty() {
		output.push_str("- none\n");

		return;
	}

	for program in &snapshot.execution_programs {
		let mapped_issues = if program.mapped_issue_identifiers.is_empty() {
			String::from("none")
		} else {
			program.mapped_issue_identifiers.join(", ")
		};
		let readback_warning = program
			.readback_warning
			.as_ref()
			.map_or_else(String::new, |warning| format!(" readback_warning={warning}"));
		let intake_kind = program.intake_kind.as_deref().unwrap_or("unknown");
		let public_summary = program.public_summary.as_deref().unwrap_or("none");

		output.push_str(&format!(
			"- program_id: {} status={} source_contract_id: {} intake_kind={} summary=\"{}\" nodes={} planned={} mapped={} ready={} queued={} blocked={} held={} active={} attention={} completed={} stale={} superseded={} dispatchable={} mapped_issues={}{}\n",
			program.program_id,
			program.status,
			program.source_contract_id.as_deref().unwrap_or("none"),
			intake_kind,
			public_summary,
			program.node_count,
			program.planned_count,
			program.mapped_count,
			program.ready_count,
			program.queued_count,
			program.blocked_count,
			program.held_count,
			program.active_count,
			program.needs_attention_count,
			program.completed_count,
			program.stale_count,
			program.superseded_count,
			program.dispatchable_count,
			mapped_issues,
			readback_warning,
		));

		for node in &program.node_readbacks {
			let issue_identifier = node.issue_identifier.as_deref().unwrap_or("unmapped");
			let issue_state = node.issue_state.as_deref().unwrap_or("none");
			let dispatch_action = node.dispatch_action.as_deref().unwrap_or("none");
			let reason_codes = if node.reason_codes.is_empty() {
				String::from("none")
			} else {
				node.reason_codes.join(",")
			};
			let reasons = if node.reasons.is_empty() {
				String::from("none")
			} else {
				node.reasons.join(" | ")
			};

			output.push_str(&format!(
				"  - node: issue={} issue_state={} program_stage={} lifecycle={} readiness={} dispatch_action={} reason_codes={} reasons=\"{}\" next_action=\"{}\"\n",
				issue_identifier,
				issue_state,
				node.program_stage,
				node.lifecycle_state,
				node.readiness_state,
				dispatch_action,
				reason_codes,
				reasons,
				node.next_action,
			));
		}
	}
}
