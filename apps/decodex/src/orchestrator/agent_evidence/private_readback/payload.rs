use serde_json::Value;

use super::super::{PRIVATE_EVIDENCE_PAYLOAD_PREVIEW_LIMIT, PrivateEvidencePayloadSummary};

pub(super) fn summarize_private_evidence_payload(payload: &Value) -> PrivateEvidencePayloadSummary {
	let encoded = serde_json::to_vec(payload).unwrap_or_default();
	let mut keys = Vec::new();
	let mut preview = Vec::new();
	let mut redacted_default_keys = Vec::new();
	let kind = match payload {
		Value::Object(object) => {
			for (key, value) in object {
				keys.push(key.clone());

				if private_evidence_payload_key_is_sensitive(key) {
					redacted_default_keys.push(key.clone());
					preview.push(format!("{key}=<redacted by default>"));
				} else {
					preview
						.push(format!("{key}={}", summarize_private_evidence_payload_value(value)));
				}
			}

			String::from("object")
		},
		Value::Array(values) => {
			preview.push(format!("array_len={}", values.len()));

			String::from("array")
		},
		Value::String(value) => {
			preview.push(truncate_private_evidence_payload_preview(value));

			String::from("string")
		},
		Value::Number(value) => {
			preview.push(value.to_string());

			String::from("number")
		},
		Value::Bool(value) => {
			preview.push(value.to_string());

			String::from("bool")
		},
		Value::Null => String::from("null"),
	};

	PrivateEvidencePayloadSummary {
		kind,
		byte_count: encoded.len(),
		keys,
		preview,
		redacted_default_keys,
	}
}

fn summarize_private_evidence_payload_value(value: &Value) -> String {
	match value {
		Value::Null => String::from("null"),
		Value::Bool(value) => value.to_string(),
		Value::Number(value) => value.to_string(),
		Value::String(value) => truncate_private_evidence_payload_preview(value),
		Value::Array(values) => format!("array(len={})", values.len()),
		Value::Object(object) => format!("object(keys={})", object.len()),
	}
}

fn private_evidence_payload_key_is_sensitive(key: &str) -> bool {
	let key = key.to_ascii_lowercase();

	key.contains("transcript")
		|| key.contains("message")
		|| key.contains("conversation")
		|| key.contains("raw")
		|| key.contains("stdout")
		|| key.contains("stderr")
		|| key.contains("log")
		|| key.contains("token")
		|| key.contains("secret")
}

fn truncate_private_evidence_payload_preview(value: &str) -> String {
	let mut preview = String::new();
	let mut truncated = false;

	for character in value.chars() {
		if preview.len() + character.len_utf8() > PRIVATE_EVIDENCE_PAYLOAD_PREVIEW_LIMIT {
			truncated = true;

			break;
		}

		preview.push(character);
	}

	if truncated {
		preview.push_str("...");
	}

	preview
}
