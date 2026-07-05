use crate::{config::ServiceConfig, tracker, workflow::WorkflowDocument};

pub(crate) fn format_no_eligible_issue_message(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> String {
	let tracker_policy = workflow.frontmatter().tracker();

	format!(
		"No eligible issue found for the configured project.\n{}",
		format_no_eligible_issue_hint(
			project.service_id(),
			tracker_policy.opt_out_label(),
			tracker_policy.needs_attention_label(),
		)
	)
}

pub(crate) fn format_status_no_eligible_issue_hint(service_id: &str) -> String {
	format!(
		"Hint: check `Todo`, label {}, no opt-out/manual-only or needs-attention labels, non-terminal state, no open dependency blockers, and no active issue claim. For Program Intake targets, check `decodex status --live` Program Intake readback; if no persisted dispatchable node is listed, run `{}` first.",
		format_no_eligible_queue_label_hint(service_id),
		format_program_intake_apply_hint(service_id),
	)
}

pub(crate) fn format_no_eligible_issue_hint(
	service_id: &str,
	opt_out_label: &str,
	needs_attention_label: &str,
) -> String {
	format!(
		"Hint: check `Todo`, label {}, no `{opt_out_label}`/`{needs_attention_label}`, non-terminal state, no open dependency blockers, and no active issue claim. For Program Intake targets, check `decodex status --live` Program Intake readback; if no persisted dispatchable node is listed, run `{}` first.",
		format_no_eligible_queue_label_hint(service_id),
		format_program_intake_apply_hint(service_id),
	)
}

pub(crate) fn format_no_eligible_queue_label_hint(service_id: &str) -> String {
	let queue_label = tracker::automation_queue_label(service_id);

	if service_id == "all" {
		String::from("`decodex:queued:<service-id>`")
	} else {
		format!("`decodex:queued:<service-id>` (this project: `{queue_label}`)")
	}
}

fn format_program_intake_apply_hint(service_id: &str) -> String {
	if service_id == "all" {
		String::from("decodex intake issues --project <service-id> --apply <ISSUE>")
	} else {
		format!("decodex intake issues --project {service_id} --apply <ISSUE>")
	}
}
