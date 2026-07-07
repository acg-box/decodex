use crate::orchestrator::{
	OperatorRunStatus,
	lane_control::reports::{LaneHardInterruptReport, LaneSoftInterruptReport},
};

pub(in crate::orchestrator::lane_control) fn lane_interrupt_next_action(
	soft: &LaneSoftInterruptReport,
	hard: Option<&LaneHardInterruptReport>,
	force: bool,
) -> String {
	if let Some(hard) = hard {
		return if hard.status == "unavailable" {
			String::from("Hard fallback was unavailable; inspect the lane before retrying.")
		} else if hard.status == "sent" || hard.status == "process_not_found" {
			String::from(
				"Inspect the lane to confirm the lease and dirty-worktree reconciliation state.",
			)
		} else {
			String::from(
				"The fallback signal did not stop the recorded process; inspect the host process before retrying.",
			)
		};
	}

	match soft.status.as_str() {
		"delivered" => {
			String::from("Inspect the lane until the app-server turn records completion.")
		},
		"pending" => {
			if force {
				String::from("Soft interrupt is pending; forced fallback was not attempted.")
			} else {
				String::from(
					"Re-run inspect shortly, or retry interrupt with --force if operator intent is to kill the process.",
				)
			}
		},
		"rejected" => String::from(
			"Inspect the lane identity before retrying; resolver rejection is not converted into hard fallback.",
		),
		"failed" | "unavailable" => String::from(
			"Retry with --force only if operator intent is to use hard process-kill fallback.",
		),
		_ => String::from("Inspect the lane for the latest run status."),
	}
}

pub(in crate::orchestrator::lane_control) fn soft_interrupt_allows_hard_fallback(
	soft: &LaneSoftInterruptReport,
	run: &OperatorRunStatus,
) -> bool {
	if run.phase == "terminal_pending" {
		return false;
	}

	match soft.status.as_str() {
		"pending" | "failed" | "unavailable" => {
			soft.error_class.as_deref() != Some("lane_not_active")
				|| run.process_id.is_some() && run.process_alive != Some(false)
		},
		"rejected" => soft.error_class.as_deref() == Some("run_lease_missing"),
		_ => false,
	}
}
