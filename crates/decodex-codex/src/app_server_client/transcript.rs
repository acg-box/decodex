//! Markdown presentation of complete native conversation items.
use serde_json::Value;

use crate::app_server_client::{ClientError, MAX_FRAME_BYTES};

pub(super) fn render(turns: &[Value]) -> Result<String, ClientError> {
	let mut output = String::from("# Conversation\n\n");
	let mut review_mode = false;

	for (index, turn) in turns.iter().enumerate() {
		let hidden_review = index > 0 && nested_review(&turns[index - 1], turn);

		for item in turn["items"].as_array().ok_or(ClientError::InvalidFrame)? {
			match item["type"].as_str() {
				Some("userMessage") if review_mode || hidden_review => {},
				Some("userMessage") => {
					let content = item["content"].as_array().ok_or(ClientError::InvalidFrame)?;
					let mut body = String::new();

					for part in content {
						match part["type"].as_str() {
							Some("text") => {
								body.push_str(
									part["text"].as_str().ok_or(ClientError::InvalidFrame)?,
								);
								body.push('\n');
							},
							Some("image" | "localImage") => body.push_str("[Image attachment]\n"),
							Some("skill" | "mention") => {
								body.push_str(part["name"].as_str().unwrap_or("[Reference]"));
								body.push('\n');
							},
							_ => body.push_str("[Non-text attachment]\n"),
						}
					}

					section(&mut output, "User", &body);
				},
				Some("agentMessage") => section(
					&mut output,
					"Assistant",
					item["text"].as_str().ok_or(ClientError::InvalidFrame)?,
				),
				Some("plan") => section(&mut output, "Plan", item["text"].as_str().unwrap_or("")),
				Some("reasoning") => {
					// Raw reasoning is not part of the visible conversation export.
					if let Some(summary) = item["summary"].as_array() {
						let text = summary
							.iter()
							.filter_map(Value::as_str)
							.collect::<Vec<_>>()
							.join("\n\n");

						section(&mut output, "Reasoning summary", &text);
					}
				},
				Some("commandExecution") => {
					let command = item["command"].as_str().unwrap_or("");
					let result = item["aggregatedOutput"].as_str().unwrap_or("");

					section(&mut output, "Command", &fenced(&format!("{command}\n{result}")));
				},
				Some("fileChange") => {
					let mut body = String::new();

					for change in item["changes"].as_array().ok_or(ClientError::InvalidFrame)? {
						body.push_str(change["path"].as_str().unwrap_or("File"));
						body.push('\n');
						body.push_str(&fenced(change["diff"].as_str().unwrap_or("")));
						body.push('\n');
					}

					section(&mut output, "File changes", &body);
				},
				Some("mcpToolCall" | "dynamicToolCall") => {
					let name = item["tool"].as_str().unwrap_or("Tool");
					let status = item["status"].as_str().unwrap_or("unknown");
					let mut body = format!("{name} · {status}\n\n");

					if let Some(args) = item.get("arguments") {
						body.push_str(&fenced(&args.to_string()));
					}

					let content =
						item.pointer("/result/content").or_else(|| item.get("contentItems"));

					if let Some(content) = content.and_then(Value::as_array) {
						for part in content {
							body.push_str("\n\n");
							body.push_str(
								part["text"]
									.as_str()
									.or_else(|| part["inputText"].as_str())
									.unwrap_or("[Non-text tool result]"),
							);
						}
					}

					for value in [item.pointer("/result/structuredContent"), item.get("error")] {
						if let Some(value) = value.filter(|v| !v.is_null()) {
							body.push_str("\n\n");
							body.push_str(&fenced(&value.to_string()));
						}
					}

					section(&mut output, "Tool", &body);
				},
				Some("enteredReviewMode") => review_mode = true,
				Some("exitedReviewMode") => review_mode = false,
				Some(kind) => section(&mut output, "Activity", &format!("[{kind}]")),
				None => return Err(ClientError::InvalidFrame),
			}

			if output.len() > MAX_FRAME_BYTES {
				return Err(ClientError::CapacityExceeded);
			}
		}
	}

	Ok(output)
}

// Native review sessions can leave an interrupted synthetic turn after review closes.
fn nested_review(previous: &Value, turn: &Value) -> bool {
	if previous["status"] != "completed"
		|| turn["status"] != "interrupted"
		|| !turn["completedAt"].is_null()
	{
		return false;
	}

	let Some(items) = previous["items"].as_array() else { return false };

	if !["enteredReviewMode", "exitedReviewMode"]
		.iter()
		.all(|kind| items.iter().any(|item| item["type"] == *kind))
	{
		return false;
	}

	let Some(items) = turn["items"].as_array() else { return false };
	let messages = items.iter().filter(|item| item["type"] == "userMessage").collect::<Vec<_>>();

	messages.len() == 2 && messages[0]["content"] == messages[1]["content"]
}

fn section(out: &mut String, title: &str, body: &str) {
	if body.trim().is_empty() {
		return;
	}

	out.push_str(&format!("## {title}\n\n{body}\n\n"));
}
fn fenced(text: &str) -> String {
	let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
	let fence = "`".repeat(3.max(longest + 1));

	format!("{fence}\n{text}\n{fence}")
}

#[cfg(test)]
mod tests {
	use serde_json;

	use crate::app_server_client::transcript;
	#[test]
	fn markdown_omits_synthetic_nested_review_prompts_but_retains_next_user() {
		let synthetic = serde_json::json!({"type":"userMessage","content":[{"type":"text","text":"Synthetic review prompt"}]});
		let text=transcript::render(&[
			serde_json::json!({"status":"completed","items":[{"type":"enteredReviewMode"},{"type":"exitedReviewMode"}]}),
			serde_json::json!({"status":"interrupted","completedAt":null,"items":[synthetic.clone(),synthetic]}),
			serde_json::json!({"status":"completed","items":[{"type":"userMessage","content":[{"type":"text","text":"Real question"}]}]}),
		]).unwrap();

		assert!(!text.contains("Synthetic review prompt"));
		assert!(text.contains("Real question"));
	}
	#[test]
	fn markdown_preserves_messages_and_hides_raw_reasoning_and_review_prompts() {
		let text = "# Heading\n\n```rust\nlet a = 1;\n```\n\n[Link](https://example.com)";
		let result = transcript::render(&[serde_json::json!({"items":[
			{"type":"userMessage","content":[{"type":"text","text":"Question"},{"type":"localImage","path":"/private/image.png"}]},
			{"type":"reasoning","summary":["Public summary"],"content":["Hidden raw reasoning"]},
			{"type":"enteredReviewMode","review":"Hidden review prompt"},
			{"type":"agentMessage","text":text},
			{"type":"commandExecution","command":"cat file","aggregatedOutput":"```\noutput"}
		]})]).unwrap();

		assert!(result.contains(text));
		assert!(result.contains("Public summary"));
		assert!(result.contains("````\ncat file\n```\noutput\n````"));
		assert!(result.contains("[Image attachment]"));
		assert!(!result.contains("Hidden"));
		assert!(!result.contains("/private/image.png"));
	}
}
