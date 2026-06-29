use serde_json::{self, Value};

pub(in crate::mcp::resources) fn resource_template_values(
	templates: &[(&str, &str, &str, &str)],
) -> Vec<Value> {
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
