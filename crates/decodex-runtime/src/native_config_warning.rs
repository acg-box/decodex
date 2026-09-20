//! Bounded public configuration diagnostics. Never retain arbitrary provider fields.
use serde_json::{Value, json};

pub(crate) fn from_frame(bytes: &[u8]) -> Option<Value> {
	#[derive(serde::Deserialize)]
	struct Fields {
		summary: String,
		details: Option<String>,
	}
	#[derive(serde::Deserialize)]
	struct Frame {
		params: Fields,
	}
	if bytes.len() > 32768 {
		return None;
	}
	let frame: Frame = serde_json::from_slice(bytes).ok()?;
	notification(&json!({"summary":frame.params.summary,"details":frame.params.details}))
}

pub(crate) fn project(params: &Value) -> Option<Value> {
	let summary = params.get("summary")?.as_str()?;
	if summary.trim().is_empty() || summary.len() > 8192 {
		return None;
	}
	let details = match params.get("details") {
		None | Some(Value::Null) => None,
		Some(Value::String(text)) if text.len() <= 16384 => Some(text.as_str()),
		_ => return None,
	};
	let clean = |text: &str| {
		if decodex_core::contains_credential_material(text) {
			"[Configuration detail hidden because it may contain credentials]".to_owned()
		} else {
			text.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect()
		}
	};
	Some(json!({"summary":clean(summary),"details":details.map(clean)}))
}

pub(crate) fn notification(params: &Value) -> Option<Value> {
	Some(json!({"method":"configWarning","params":project(params)?}))
}

pub(crate) async fn record(
	store: &decodex_database::SqliteStore,
	root: &str,
	generation: &decodex_core::ProcessGenerationId,
	params: &Value,
) -> Result<(), decodex_database::StoreError> {
	use sha2::{Digest as _, Sha256};
	let Some(value) = project(params) else {
		return Ok(());
	};
	let mut text =
		format!("Codex configuration warning: {}", value["summary"].as_str().unwrap_or_default());
	if let Some(details) = value["details"].as_str().filter(|s| !s.trim().is_empty()) {
		text.push_str("\n\n");
		text.push_str(details);
	}
	let digest = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
	store.record_chief_config_warning(root.into(), generation.as_str().into(), digest, text).await
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn only_bounded_public_diagnostics_are_projected() {
		let warning = project(&json!({"summary":"Ignored \"setting\"","details":"one\ntwo","path":"private","unknown":"secret"})).unwrap();
		assert_eq!(warning, json!({"summary":"Ignored \"setting\"","details":"one\ntwo"}));
		for bad in [
			json!({}),
			json!({"summary":" "}),
			json!({"summary":1}),
			json!({"summary":"x","details":false}),
			json!({"summary":"x".repeat(8193)}),
			json!({"summary":"x","details":"x".repeat(16385)}),
		] {
			assert!(project(&bad).is_none());
		}
		let secret = "Bearer fixture-private-access-token-123456789";
		let safe = project(&json!({"summary":"Invalid header","details":secret})).unwrap();
		assert!(!safe.to_string().contains(secret));
	}
}
