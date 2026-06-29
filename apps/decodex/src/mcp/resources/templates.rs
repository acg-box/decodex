use serde_json::{self, Value};

pub(super) fn docs_resource_templates() -> Vec<Value> {
	resource_template_values(&[
		(
			"decodex://docs/spec/{topic}",
			"Decodex specs",
			"Checked-in normative Decodex specification concepts.",
			"text/markdown",
		),
		(
			"decodex://docs/runbook/{topic}",
			"Decodex runbooks",
			"Checked-in Decodex operator procedures.",
			"text/markdown",
		),
		(
			"decodex://docs/reference/{topic}",
			"Decodex references",
			"Checked-in Decodex implementation and current-state references.",
			"text/markdown",
		),
		(
			"decodex://docs/decisions/{topic}",
			"Decodex decisions",
			"Checked-in Decodex design-rationale concepts.",
			"text/markdown",
		),
		(
			"decodex://research/{concept}",
			"Decodex research concepts",
			"Checked-in Markdown Research Contract concepts.",
			"text/markdown",
		),
	])
}

pub(super) fn runtime_resource_templates() -> Vec<Value> {
	resource_template_values(&[
		(
			"decodex://decision-contracts/{contract_id}",
			"Runtime Decision Contracts",
			"Local runtime Decision Contract readback by contract id.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/status",
			"Project status",
			"Local runtime project status readback.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/status_live",
			"Project live status",
			"Remote-safe current operation, phase, event counts, progress diagnostics, and validation status.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/activity_tail",
			"Project activity tail",
			"Remote-safe activity readback for current and recent runs.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/lane_inspect/{issue}",
			"Lane inspect readback",
			"Read-only lane inspect alias for remote-safe current-lane state.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/lane-control/{issue}",
			"Lane-control readback",
			"Inspect one local lane before requesting guarded lane-control actions.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/events",
			"Run event readback",
			"Remote-safe event counts for a run visible in the current/recent status snapshot.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/protocol_activity",
			"Run protocol activity",
			"Remote-safe protocol activity for a run visible in the current/recent status snapshot, without hidden reasoning or raw payloads.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/child_agent_activity",
			"Run child-agent activity",
			"Remote-safe child-agent activity for a run visible in the current/recent status snapshot.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/runs/{run_id}/progress_diagnostics",
			"Run progress diagnostics",
			"Remote-safe progress and suspected-stall diagnostics for a run visible in the current/recent status snapshot.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/pr_review_state",
			"PR/review state",
			"Remote-safe PR and review-state readback.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy",
			"Autonomy summaries",
			"Read-only project autonomy objective, signal, proposal, and evidence summaries.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/objectives/{objective_id}/current",
			"Current autonomy objective",
			"Read-only current accepted Objective Contract summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/objectives/{objective_id}/{version}",
			"Autonomy objective version",
			"Read-only Objective Contract version summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/signals",
			"Autonomy signals",
			"Read-only recent autonomy signal summaries.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/signals/{signal_id}",
			"Autonomy signal",
			"Read-only autonomy signal summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/proposals",
			"Autonomy proposals",
			"Read-only recent autonomy proposal summaries.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/proposals/{proposal_id}",
			"Autonomy proposal",
			"Read-only autonomy proposal summary.",
			"application/json",
		),
		(
			"decodex://projects/{project_id}/autonomy/evidence",
			"Autonomy evidence summaries",
			"Read-only evidence summary counts and refs derived from recent signals and proposals.",
			"application/json",
		),
	])
}

fn resource_template_values(templates: &[(&str, &str, &str, &str)]) -> Vec<Value> {
	templates
		.iter()
		.map(|(uri_template, name, description, mime_type)| {
			serde_json::json!({
				"uriTemplate": uri_template,
				"name": name,
				"description": description,
				"mimeType": mime_type
			})
		})
		.collect()
}
