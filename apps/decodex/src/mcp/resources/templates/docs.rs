use serde_json::Value;

use crate::mcp::resources::templates::builder;

pub(in crate::mcp::resources) fn docs_resource_templates() -> Vec<Value> {
	builder::resource_template_values(&[
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
	])
}
