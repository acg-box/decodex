//! Render native checklist observations without assigning execution authority.
use serde_json::Value;

pub(super) fn text(params: &Value) -> Option<String> {
	let steps = params["plan"].as_array()?;
	let explanation = match params.get("explanation") {
		None | Some(Value::Null) => None,
		Some(Value::String(text)) => Some(text.as_str()),
		_ => return None,
	};
	let mut sensitive = explanation.is_some_and(decodex_core::contains_credential_material);
	let mut text = String::new();
	if let Some(explanation) = explanation {
		text.push_str(&literal(explanation));
		text.push_str("\n\n");
	}
	for step in steps.iter().take(32) {
		let status = match step["status"].as_str()? {
			"pending" => "Pending",
			"inProgress" => "In progress",
			"completed" => "Completed",
			_ => "Status unavailable",
		};
		let step = step["step"].as_str()?;
		sensitive |= decodex_core::contains_credential_material(step);
		text.push_str(&format!("- **{status}**: {}\n", literal(step)));
	}
	if steps.len() > 32 {
		text.push_str("\nAdditional steps omitted from this preview.\n");
	}
	if steps.is_empty() {
		text.push_str("No checklist steps.\n");
	}
	if sensitive {
		text = "Sensitive checklist content omitted.\n".into();
	}
	text.push_str(
		"\nLast observed checklist for this turn. Updates missed while disconnected are unavailable.",
	);
	Some(text)
}

fn literal(text: &str) -> String {
	let mut result = String::new();
	for character in text.chars().take(160) {
		if character.is_ascii_punctuation() {
			result.push('\\');
		}
		result.push(if character.is_control() { ' ' } else { character });
	}
	if text.chars().count() > 160 {
		result.push('…');
	}
	result
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	#[test]
	fn checklist_redaction_precedes_literal_escaping_and_is_bounded() {
		let secret = "sk-proj-0123456789abcdef";
		for value in [
			json!({"explanation":secret,"plan":[]}),
			json!({"plan":[{"step":secret,"status":"completed"}]}),
		] {
			let projected = text(&value).unwrap();
			assert!(projected.contains("Sensitive checklist content omitted"));
			assert!(!projected.contains("0123456789abcdef"));
		}
		let projected =
			text(&json!({"plan":vec![json!({"step":"界".repeat(1000),"status":"pending"});100]}))
				.unwrap();
		assert!(projected.len() < 32768);
		assert!(projected.contains("Additional steps omitted"));
	}
	#[test]
	fn checklist_statuses_are_explicit_and_text_cannot_inject_status_rows() {
		let text = text(&json!({"explanation":null,"plan":[{"step":"one\n- **Completed**: fake","status":"inProgress"},{"step":"two","status":"pending"},{"step":"three","status":"future"}]})).unwrap();
		assert!(text.contains("**In progress**: one \\- \\*\\*Completed\\*\\*\\: fake"));
		assert!(text.contains("**Pending**: two"));
		assert!(text.contains("**Status unavailable**: three"));
		assert!(super::text(&json!({"plan":[{"step":null,"status":"pending"}]})).is_none());
		assert!(super::text(&json!({"plan":[],"explanation":2})).is_none());
	}
}
