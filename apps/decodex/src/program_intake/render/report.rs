use crate::program_intake::model::{GoalIntakeReport, IssueBatchIntakeReport};

/// Render a compact human-readable intake report.
pub(crate) fn render_issue_batch_intake_report(report: &IssueBatchIntakeReport) -> String {
	let mode = if report.persisted { "apply" } else { "dry-run" };
	let mut output = format!(
		"program intake {mode}: service={} program={} persisted={} scheduler_visible={} ready={} held={} blocked={} stale={} unmapped={}\n",
		report.service_id,
		report.program_id,
		report.persisted,
		report.scheduler_visible,
		report.counts.ready,
		report.counts.held,
		report.counts.blocked,
		report.counts.stale,
		report.counts.unmapped,
	);

	for row in &report.issues {
		let state = row.issue_state.as_deref().unwrap_or("unmapped");
		let action = row.dispatch_action.as_deref().unwrap_or("none");
		let reasons = list_or_none(&row.reasons, "; ");

		output.push_str(&format!(
			"- {} classification={} state={} dispatch_action={} reasons={}\n",
			row.issue_identifier,
			row.classification.as_str(),
			state,
			action,
			reasons
		));
	}

	output
}

/// Render a compact human-readable promoted-goal intake report.
pub(crate) fn render_goal_intake_report(report: &GoalIntakeReport) -> String {
	let mode = if report.applied { "apply" } else { "dry-run" };
	let mut output = format!(
		"goal intake {mode}: service={} contract={} program={} issues={} persisted={}\n",
		report.service_id,
		report.contract_id,
		report.program_id,
		report.issues.len(),
		report.persisted,
	);

	for row in &report.issues {
		let issue = row.issue_identifier.as_deref().unwrap_or("new");
		let dispatch_action = row.dispatch_action.as_deref().unwrap_or("none");
		let dependencies = list_or_none(&row.dependencies, ", ");
		let conflicts = list_or_none(&row.conflict_domains, ", ");
		let reasons = list_or_none(&row.reasons, ", ");

		output.push_str(&format!(
			"- {} action={} issue={} queue_intent={} dispatch_action={} dependencies={} conflict_domains={} reasons={}\n",
			row.node_id,
			row.action.as_str(),
			issue,
			row.queue_intent,
			dispatch_action,
			dependencies,
			conflicts,
			reasons,
		));
	}

	output
}

fn list_or_none(values: &[String], separator: &str) -> String {
	if values.is_empty() { String::from("none") } else { values.join(separator) }
}
