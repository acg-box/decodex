use serde_json::Value;

use crate::mcp::resources::templates::builder;

pub(in crate::mcp::resources) fn openwiki_resource_templates() -> Vec<Value> {
	builder::resource_template_values(&[
		(
			"decodex://openwiki/specs/{topic}",
			"Decodex OpenWiki specs",
			"Checked-in Decodex OpenWiki contracts and data concepts.",
			"text/markdown",
		),
		(
			"decodex://openwiki/workflows/{topic}",
			"Decodex OpenWiki workflows",
			"Checked-in Decodex OpenWiki runtime and operator workflows.",
			"text/markdown",
		),
		(
			"decodex://openwiki/operations/{topic}",
			"Decodex OpenWiki operations",
			"Checked-in Decodex OpenWiki commands and validation guidance.",
			"text/markdown",
		),
		(
			"decodex://openwiki/architecture/{topic}",
			"Decodex OpenWiki architecture",
			"Checked-in Decodex OpenWiki architecture notes.",
			"text/markdown",
		),
	])
}
