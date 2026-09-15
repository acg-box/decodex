//! Bound durable result JSON without slicing its serialized representation.

use serde_json::{Value, json};

const MAX_BYTES: usize = 48_000;

pub(super) fn terminal(params: &Value) -> Value {
	let turn = &params["turn"];
	let mut error = turn["error"].clone();
	if error.to_string().len() > 4096 {
		let message = error["message"].as_str().unwrap_or("Terminal error details omitted.");
		error = json!({"message":message.chars().take(512).collect::<String>(),"truncated":true});
	}
	let omitted = params
		.as_object()
		.is_some_and(|fields| fields.keys().any(|key| key != "threadId" && key != "turn"))
		|| turn.as_object().is_some_and(|fields| {
			fields.keys().any(|key| key != "id" && key != "status" && key != "error")
		}) || error != turn["error"];
	json!({"threadId":params["threadId"],"turn":{"id":turn["id"],"status":turn["status"],"error":error},"detailsOmitted":omitted})
}

pub(super) fn collect(turn: Option<&Value>) -> (Vec<Value>, bool) {
	let items = turn.and_then(|turn| turn["items"].as_array()).into_iter().flatten();
	let mut messages = Vec::new();
	let mut remaining = MAX_BYTES - 2; // Array brackets.
	for item in items.filter(|item| item["type"] == "agentMessage") {
		let separator = usize::from(!messages.is_empty());
		let budget = remaining.saturating_sub(separator);
		let size = item.to_string().len();
		if size <= budget {
			messages.push(item.clone());
			remaining -= size + separator;
			continue;
		}
		if let Some(message) = truncate_message(item, budget) {
			messages.push(message);
		}
		return (messages, true);
	}
	(messages, false)
}

fn truncate_message(item: &Value, budget: usize) -> Option<Value> {
	let text = item["text"].as_str()?;
	let mut message = item.clone();
	message["text"] = json!("");
	if message.to_string().len() > budget {
		// Oversized metadata must not hide an otherwise readable result.
		message = json!({"type":"agentMessage","text":""});
	}
	if message.to_string().len() > budget {
		return None;
	}
	let mut low = 0;
	let mut high = text.len();
	while low < high {
		let middle = low + (high - low).div_ceil(2);
		let mut end = middle;
		while !text.is_char_boundary(end) {
			end -= 1;
		}
		message["text"] = json!(&text[..end]);
		if message.to_string().len() <= budget {
			low = middle;
		} else {
			high = middle - 1;
		}
	}
	while !text.is_char_boundary(low) {
		low -= 1;
	}
	message["text"] = json!(&text[..low]);
	Some(message)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn long_escaped_multibyte_output_remains_structured_readable_and_bounded() {
		let text = "界🙂\"\\\n\u{0001}".repeat(12_000);
		let turn = json!({"items":[{"type":"agentMessage","id":"message-1","text":text}]});
		let (messages, truncated) = collect(Some(&turn));
		assert!(truncated);
		let encoded = serde_json::to_string(&messages).unwrap();
		assert!(encoded.len() <= MAX_BYTES);
		let decoded: Value = serde_json::from_str(&encoded).unwrap();
		let retained = decoded[0]["text"].as_str().unwrap();
		assert!(!retained.is_empty());
		assert!(text.starts_with(retained));
		assert_eq!(decoded[0]["id"], "message-1");
		assert!(retained.len() < text.len());
	}

	#[test]
	fn combined_evidence_fits_store_limit_with_maximum_escaped_ids_and_error() {
		let id = "\u{0001}".repeat(512);
		let turn = json!({"id":id,"status":"failed","error":{"message":"e".repeat(4000)},
			"items":[{"type":"agentMessage","text":"界\"".repeat(MAX_BYTES)}]});
		let (messages, truncated) = collect(Some(&turn));
		let evidence = json!({"terminal":terminal(&json!({"threadId":id,"turn":turn})),
			"threadReadback":{"threadId":id,"turnId":id,"assistantMessages":messages,
			"truncated":truncated,"exactTurnReadback":true}});
		assert!(evidence.to_string().len() <= 65536);
	}

	#[test]
	fn ordinary_messages_keep_fields_and_non_assistant_items_are_excluded() {
		let turn = json!({"items":[
			{"type":"commandExecution","text":"not assistant"},
			{"type":"agentMessage","id":"one","text":"first","phase":"commentary"},
			{"type":"agentMessage","id":"two","text":"done","phase":"final_answer"}
		]});
		let (messages, truncated) = collect(Some(&turn));
		assert!(!truncated);
		assert_eq!(messages, vec![turn["items"][1].clone(), turn["items"][2].clone()]);
		assert_eq!(collect(None), (Vec::new(), false));
	}

	#[test]
	fn oversized_metadata_preserves_text_and_many_messages_obey_array_budget() {
		let turn = json!({"items":[{"type":"agentMessage","id":"x".repeat(MAX_BYTES),"text":"useful result"}]});
		let (messages, truncated) = collect(Some(&turn));
		assert!(truncated);
		assert_eq!(messages[0]["text"], "useful result");
		let turn = json!({"items":vec![json!({"type":"agentMessage","text":"done"}); 2000]});
		let (messages, truncated) = collect(Some(&turn));
		assert!(truncated);
		assert!(serde_json::to_string(&messages).unwrap().len() <= MAX_BYTES);
	}
}
