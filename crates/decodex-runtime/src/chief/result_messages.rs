//! Bound durable result JSON without slicing its serialized representation.

use serde_json::{Value, json};

const MAX_BYTES: usize = 48_000;

pub(super) fn terminal(params: &Value) -> Value {
	let turn = &params["turn"];
	let mut error = turn["error"].clone();
	if error.to_string().len() > 4096 {
		let message = error["message"].as_str().unwrap_or("Terminal error details omitted.");
		let classification =
			error.get("codexErrorInfo").filter(|value| value.to_string().len() <= 1024).cloned();
		error = json!({"message":message.chars().take(512).collect::<String>(),"truncated":true});
		if let Some(classification) = classification {
			error["codexErrorInfo"] = classification;
		}
	}
	let mut retained = json!({"id":turn["id"],"status":turn["status"],"error":error});
	for field in ["startedAt", "completedAt", "durationMs"] {
		retained[field] = turn[field].as_i64().filter(|value| *value >= 0).into();
	}
	let omitted = params
		.as_object()
		.is_some_and(|fields| fields.keys().any(|key| key != "threadId" && key != "turn"))
		|| turn.as_object().is_some_and(|fields| {
			fields.iter().any(|(key, value)| retained.get(key) != Some(value))
		});
	json!({"threadId":params["threadId"],"turn":retained,"detailsOmitted":omitted})
}

pub(super) fn collect(turn: Option<&Value>) -> (Vec<Value>, bool) {
	let mut items: Vec<_> = turn
		.and_then(|turn| turn["items"].as_array())
		.into_iter()
		.flatten()
		.enumerate()
		.filter(|(_, item)| item["type"] == "agentMessage")
		.collect();
	let last = items.last().map(|(index, _)| *index);
	items.sort_by_key(|(index, item)| {
		(
			if item["phase"] == "final_answer" {
				0
			} else if Some(*index) == last {
				1
			} else {
				2
			},
			std::cmp::Reverse(*index),
		)
	});
	let mut messages = Vec::new();
	let mut remaining = MAX_BYTES - 2;
	let mut truncated = false;
	for (index, item) in items {
		let separator = usize::from(!messages.is_empty());
		let budget = remaining.saturating_sub(separator);
		let message = if item.to_string().len() <= budget {
			Some(item.clone())
		} else {
			truncated = true;
			truncate_message(item, budget)
		};
		if let Some(message) = message {
			remaining = remaining.saturating_sub(message.to_string().len() + separator);
			messages.push((index, message));
		}
	}
	messages.sort_by_key(|(index, _)| *index);
	(messages.into_iter().map(|(_, message)| message).collect(), truncated)
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

/// Select numeric provider facts without retaining unrelated notification content.
pub(super) fn usage(params: &Value) -> Option<Value> {
	let usage = &params["tokenUsage"];
	let count = |path: &str| usage.pointer(path)?.as_i64().filter(|value| *value >= 0);
	Some(json!({
		"input_tokens": count("/total/inputTokens")?,
		"output_tokens": count("/total/outputTokens")?,
		"context_tokens": count("/last/totalTokens")?,
		"context_window": usage["modelContextWindow"].as_i64().filter(|size| *size > 0)
	}))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn long_error_keeps_provider_classification_without_partial_steer() {
		let output = terminal(
			&json!({"threadId":"thread","turn":{"id":"turn","status":"failed","error":{"message":"Stopped","codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"x".repeat(8000),"steer":{"message":"User must acknowledge this"}}}}}),
		);
		assert_eq!(output["turn"]["error"]["codexErrorInfo"], "misalignmentPolicyViolation");
		assert_eq!(output["turn"]["error"]["truncated"], true);
		assert!(output["turn"]["error"]["misalignment"].is_null());
		assert!(output.to_string().len() < 4096);
	}

	#[test]
	fn terminal_retains_native_times_without_inventing_replay_times() {
		let value = terminal(&json!({"threadId":"thread","turn":{
			"id":"turn","status":"completed","error":null,
			"startedAt":1700000000,"completedAt":1700000125,"durationMs":125000
		}}));
		assert_eq!(value["turn"]["startedAt"], 1700000000);
		assert_eq!(value["turn"]["completedAt"], 1700000125);
		assert_eq!(value["detailsOmitted"], false);
		let old = terminal(&json!({"turn":{"id":"old","status":"completed"}}));
		assert!(old["turn"]["startedAt"].is_null());
		assert!(old["turn"]["completedAt"].is_null());
		for invalid in [json!(-1), json!("x".repeat(70000)), json!({"unexpected":true})] {
			let bounded = terminal(&json!({"turn":{"id":"turn","startedAt":invalid,
				"completedAt":invalid,"durationMs":invalid}}));
			assert!(bounded["turn"]["startedAt"].is_null());
			assert!(bounded["turn"]["completedAt"].is_null());
			assert!(bounded["turn"]["durationMs"].is_null());
			assert_eq!(bounded["detailsOmitted"], true);
			assert!(bounded.to_string().len() < 512);
		}
	}

	#[test]
	fn terminal_preserves_provider_duration_without_inventing_old_metrics() {
		let value = terminal(
			&json!({"threadId":"thread","turn":{"id":"turn","status":"completed","durationMs":12345}}),
		);
		assert_eq!(value["turn"]["durationMs"], 12345);
		assert!(terminal(&json!({"turn":{"id":"old"}}))["turn"]["durationMs"].is_null());
	}

	#[test]
	fn final_report_survives_large_commentary_and_keeps_chronological_order() {
		let turn = json!({"items":[
			{"type":"agentMessage","phase":"commentary","text":"x".repeat(60000)},
			{"type":"agentMessage","phase":"final_answer","text":"Verified final result"}
		]});
		let (messages, truncated) = collect(Some(&turn));
		assert!(truncated);
		assert_eq!(messages.last().unwrap()["text"], "Verified final result");
		assert!(serde_json::to_vec(&messages).unwrap().len() <= MAX_BYTES);
	}

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
