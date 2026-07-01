use crate::orchestrator::lane_control::reports::{LaneInspectReport, LaneInterruptReport};

pub(super) fn render_lane_inspect_report(report: &LaneInspectReport) -> String {
	let mut output = format!(
		"Lane inspect for {} in project {} ({} run{})\n",
		report.issue,
		report.project_id,
		report.matched_run_count,
		if report.matched_run_count == 1 { "" } else { "s" }
	);

	for run in &report.runs {
		output.push_str(&format!(
			"- {} attempt {}: status={}, phase={}, runLease={}, owner={}, liveness={}\n",
			run.run_id,
			run.attempt_number,
			run.status,
			run.phase,
			run.run_lease,
			run.ownership_state,
			run.execution_liveness
		));
		output.push_str(&format!(
			"  laneControl: livenessState={}, policyState={}, terminalization={}, nextAction={}, conditions={}\n",
			run.liveness_state,
			run.policy_state,
			run.terminalization_state,
			run.lane_control_next_action,
			if run.lane_control_conditions.is_empty() {
				String::from("none")
			} else {
				run.lane_control_conditions.join(",")
			}
		));
		output.push_str(&format!(
			"  appServer: thread={}, turn={}, softInterruptAvailable={}\n",
			run.thread_id.as_deref().unwrap_or("none"),
			run.turn_id.as_deref().unwrap_or("none"),
			run.soft_interrupt_available
		));
		output.push_str(&format!(
			"  process: pid={}, alive={}, hardInterruptAvailable={} (requires --force)\n",
			run.process_id.map_or_else(|| String::from("none"), |id| id.to_string()),
			run.process_alive.map_or_else(|| String::from("unknown"), |alive| alive.to_string()),
			run.hard_interrupt_available
		));
	}

	output
}

pub(super) fn render_lane_interrupt_report(report: &LaneInterruptReport) -> String {
	let mut output = format!(
		"Lane interrupt {} for run {}: {}\n",
		report.classification, report.run_id, report.soft_interrupt.message
	);

	if let Some(hard) = &report.hard_interrupt {
		output.push_str(&format!(
			"Hard fallback {}: {} ({})\n",
			hard.status,
			hard.message,
			if hard.signals.is_empty() {
				String::from("no signals")
			} else {
				hard.signals.join(",")
			}
		));
	}

	output.push_str(&format!("Next action: {}\n", report.next_action));

	output
}
