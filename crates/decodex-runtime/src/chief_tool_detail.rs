//! Readable native tool results. Media bodies stay in native history, not text details.
use serde_json::Value;

pub(super) fn parts(item: &Value) -> Vec<String> {
	let mut parts = Vec::new();
	for field in ["server", "namespace", "tool"] {
		append_text(&mut parts, &item[field]);
	}
	if let Some(context) = item.get("appContext").filter(|value| value.is_object()) {
		for (field, label) in [
			("appName", "App"),
			("actionName", "Action"),
			("connectorId", "Connector"),
			("linkId", "Link"),
			("resourceUri", "Resource"),
		] {
			if let Some(value) = context[field].as_str().filter(|value| !value.is_empty()) {
				parts.push(format!("{label}: {value}"));
			}
		}
	}
	let content = item.pointer("/result/content").or_else(|| item.get("contentItems"));
	for block in content.and_then(Value::as_array).into_iter().flatten() {
		content_parts(block, &mut parts);
	}
	if let Some(structured) = item.pointer("/result/structuredContent").filter(|v| !v.is_null()) {
		parts.push(format!("Structured result:\n{structured}"));
	}
	if item["status"] == "failed" || item["success"] == false {
		parts.push("Tool reported failure".into());
	}
	if let Some(error) = item.get("error").filter(|v| !v.is_null()) {
		if let Some(message) = error["message"].as_str() {
			parts.push(message.into());
		} else {
			parts.push(format!("Tool error: {error}"));
		}
	}
	parts
}

fn append_text(parts: &mut Vec<String>, value: &Value) {
	if let Some(text) = value.as_str() {
		parts.push(text.into());
	}
}

fn content_parts(block: &Value, parts: &mut Vec<String>) {
	match block["type"].as_str() {
		Some("text" | "inputText") if block["text"].is_string() =>
			append_text(parts, &block["text"]),
		Some("image" | "inputImage" | "audio" | "inputAudio") => {
			let image = matches!(block["type"].as_str(), Some("image" | "inputImage"));
			parts.push(if image { "Returned image" } else { "Returned audio" }.into());
			// Code-mode blocks can include useful text alongside media.
			append_text(parts, &block["text"]);
		},
		Some("resource") => {
			parts.push("Embedded resource".into());
			append_text(parts, &block["resource"]["uri"]);
			append_text(parts, &block["resource"]["mimeType"]);
			append_text(parts, &block["resource"]["text"]);
			if block["resource"].get("blob").is_some() {
				parts.push("Binary resource content retained in native history".into());
			}
		},
		Some("resource_link") => {
			parts.push("Resource link".into());
			for field in ["uri", "name", "title", "description", "mimeType"] {
				append_text(parts, &block[field]);
			}
		},
		_ => parts.push(format!("Tool content: {block}")),
	}
}
