use serde_json::Value;

use crate::mcp::resources::templates::builder;

pub(in crate::mcp::resources) fn autonomy_resource_templates() -> Vec<Value> {
	builder::resource_template_values(&[
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
			"decodex://projects/{project_id}/autonomy/proposals/affected/{namespace}/{value}",
			"Autonomy proposal by affected identifier",
			"Read-only exact proposal lookup by a namespaced affected identifier.",
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
