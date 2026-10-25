//! Render native checklist observations without assigning execution authority.
use serde_json::Value;

/// A bounded public milestone from the native plan, separate from its transcript rendering.
pub(super) fn progress(params: &Value) -> Option<Value> {
	let steps = params["plan"].as_array()?;

	if steps.is_empty()
		|| steps
			.iter()
			.any(|s| !matches!(s["status"].as_str(), Some("pending" | "inProgress" | "completed")))
	{
		return None;
	}

	let complete = steps.iter().all(|s| s["status"] == "completed");
	let step = steps
		.iter()
		.find(|s| s["status"] == "inProgress")
		.or_else(|| steps.iter().find(|s| s["status"] == "pending"))
		.or_else(|| steps.last())?;
	let summary = step["step"].as_str()?;

	if decodex_core::contains_credential_material(summary) {
		return None;
	}

	Some(
		serde_json::json!({"summary": summary.chars().take(240).collect::<String>(), "complete":complete}),
	)
}

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

	use crate::agent::checklist;
	#[test]
	fn delivery_milestone_uses_native_step_state_and_excludes_empty_plans() {
		let plan = serde_json::json!({"plan":[{"step":"Inspect","status":"completed"},{"step":"Write the guide","status":"inProgress"}]});

		assert_eq!(
			checklist::progress(&plan),
			Some(serde_json::json!({"summary":"Write the guide","complete":false}))
		);
		assert!(checklist::progress(&serde_json::json!({"plan":[]})).is_none());
	}
	#[test]
	fn checklist_redaction_precedes_literal_escaping_and_is_bounded() {
		let secret = "sk-proj-0123456789abcdef";

		for value in [
			serde_json::json!({"explanation":secret,"plan":[]}),
			serde_json::json!({"plan":[{"step":secret,"status":"completed"}]}),
		] {
			let projected = checklist::text(&value).unwrap();

			assert!(projected.contains("Sensitive checklist content omitted"));
			assert!(!projected.contains("0123456789abcdef"));
		}

		let projected =
			checklist::text(&serde_json::json!({"plan":vec![serde_json::json!({"step":"界".repeat(1_000),"status":"pending"});100]}))
				.unwrap();

		assert!(projected.len() < 32_768);
		assert!(projected.contains("Additional steps omitted"));
	}
	#[test]
	fn checklist_statuses_are_explicit_and_text_cannot_inject_status_rows() {
		let text = checklist::text(&serde_json::json!({"explanation":null,"plan":[{"step":"one\n- **Completed**: fake","status":"inProgress"},{"step":"two","status":"pending"},{"step":"three","status":"future"}]})).unwrap();

		assert!(text.contains("**In progress**: one \\- \\*\\*Completed\\*\\*\\: fake"));
		assert!(text.contains("**Pending**: two"));
		assert!(text.contains("**Status unavailable**: three"));
		assert!(
			super::text(&serde_json::json!({"plan":[{"step":null,"status":"pending"}]})).is_none()
		);
		assert!(super::text(&serde_json::json!({"plan":[],"explanation":2})).is_none());
	}
}
