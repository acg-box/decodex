use decodex_protocol::{TaskRecap, WireText};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recap {
	summary: String,
	#[serde(deserialize_with = "Option::deserialize")]
	next_action: Option<String>,
}
pub(super) fn parse(value: &str) -> Option<TaskRecap> {
	let decoded: Recap = serde_json::from_str(value).ok()?;
	let next = decoded.next_action.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
	let recap = TaskRecap {
		summary: WireText::new(decoded.summary.trim()).ok()?,
		next_action: next.map(WireText::new).transpose().ok()?,
	};
	recap.is_valid().then_some(recap)
}
pub(super) fn schema() -> Value {
	json!({"type":"object","properties":{"summary":{"type":"string","minLength":1,"maxLength":700},"next_action":{"type":["string","null"],"maxLength":200}},"required":["summary","next_action"],"additionalProperties":false})
}
