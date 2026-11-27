//! A successful Codex Board receipt keeps its native topic and author.
use serde_json::{Value, json};

pub(super) fn receipt(item: &Value) -> Option<String> {
	let (result, tool, arguments) = match item["type"].as_str()? {
		"mcpToolCall"
			if item["server"] == "codex_message_board"
				&& item["status"] == "completed"
				&& item["error"].is_null()
				&& item["result"]["isError"] != true =>
			(
				std::borrow::Cow::Borrowed(&item["result"]["structuredContent"]),
				item["tool"].as_str()?,
				&item["arguments"],
			),
		"functionCallOutput"
			if item["namespace"] == "collaboration" && item["success"] != false =>
		{
			let output = super::tool_output::parts(item)?;
			(
				std::borrow::Cow::Owned(serde_json::from_str(&output[1..].join("\n")).ok()?),
				item["name"].as_str()?,
				&Value::Null,
			)
		},
		_ => return None,
	};
	if !matches!(tool, "post" | "read_post") {
		return None;
	}
	let field = |key: &str| {
		result[key]
			.as_str()
			.filter(|v| !v.is_empty() && v.len() <= 512 && !v.chars().any(char::is_control))
	};
	let text = if tool == "post" { arguments["text"].as_str() } else { result["text"].as_str() }
		.unwrap_or_default();
	// A tail page is not the beginning of the post. Preserve the identity, not a misleading
	// excerpt.
	let prefix =
		result["next_offset_chars"].as_u64().is_none_or(|end| end == text.chars().count() as u64);
	let excerpt =
		if prefix { text.get(..text.floor_char_boundary(3000.min(text.len())))? } else { "" };
	let truncated = excerpt.len() != text.len()
		|| result["n_chars"].as_u64().is_some_and(|n| n > excerpt.chars().count() as u64);
	let mut body = json!({"thread_id":field("thread_id")?, "message_id":field("message_id")?,
		"channel_name":field("channel_name")?, "author":field("author")?, "created_at":field("created_at")?,
		"text":excerpt, "truncated":truncated});
	// Visible timeline items have a byte limit even when JSON escaping expands their content.
	if body.to_string().len() > 8192 {
		body["text"] = excerpt.chars().take(512).collect::<String>().into();
		body["truncated"] = true.into();
	}
	Some(body.to_string())
}
