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
	let mut text = format!("Codex warning: {}", value["summary"].as_str().unwrap_or_default());
	if let Some(details) = value["details"].as_str().filter(|s| !s.trim().is_empty()) {
		text.push_str("\n\n");
		text.push_str(details);
	}
	let digest = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
	store.record_chief_config_warning(root.into(), generation.as_str().into(), digest, text).await
}

/// Shared host dispatch for process and task diagnostics.
pub(crate) async fn record_notification(
	store: &decodex_database::SqliteStore,
	root: &str,
	generation: &decodex_core::ProcessGenerationId,
	method: &str,
	params: &Value,
) -> Result<(), decodex_database::StoreError> {
	match method {
		"warning" => record_warning(store, root, generation, params).await,
		"configWarning" => record(store, root, generation, params).await,
		_ => Ok(()),
	}
}

/// Keep only the public message and exact optional thread identity.
pub(crate) fn warning(params: &Value) -> Option<Value> {
	let thread = match params.get("threadId") {
		None | Some(Value::Null) => None,
		Some(Value::String(id))
			if !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control) =>
			Some(id),
		_ => return None,
	};
	let message = params.get("message")?.as_str()?;
	let projected = project(&json!({"summary":message}))?;
	Some(json!({"threadId":thread,"message":projected["summary"]}))
}

pub(crate) fn warning_from_frame(bytes: &[u8]) -> Option<Value> {
	if bytes.len() > 32768 {
		return None;
	}
	let frame: Value = serde_json::from_slice(bytes).ok()?;
	Some(json!({"method":"warning","params":warning(frame.get("params")?)?}))
}

pub(crate) async fn record_warning(
	store: &decodex_database::SqliteStore,
	root: &str,
	generation: &decodex_core::ProcessGenerationId,
	params: &Value,
) -> Result<(), decodex_database::StoreError> {
	use sha2::{Digest as _, Sha256};
	let Some(value) = warning(params) else {
		return Ok(());
	};
	let text = format!("Codex warning: {}", value["message"].as_str().unwrap_or_default());
	let digest = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
	store
		.record_chief_native_warning(
			root.into(),
			generation.as_str().into(),
			value["threadId"].as_str().map(str::to_owned),
			digest,
			text,
		)
		.await
}

/// Retain an actionable configuration RPC cause without changing dispatch certainty.
/// Private error data and Debug output are never projected into the transcript.
pub(crate) async fn record_settings_error(
	store: &decodex_database::SqliteStore,
	source: &crate::chief_usage_estimate::Source,
	operation: &'static str,
	error: &decodex_codex::app_server_client::ClientError,
) {
	let decodex_codex::app_server_client::ClientError::Remote(error) = error else {
		return;
	};
	let Some(message) = settings_error_message(operation, error) else {
		return;
	};
	let _ = record_warning(
		store,
		&source.key.work,
		&source.key.generation,
		&json!({"threadId":source.key.thread,"message":message}),
	)
	.await;
}

fn settings_error_message(
	operation: &str,
	error: &decodex_codex::app_server_client::RpcError,
) -> Option<String> {
	let projected = project(&json!({"summary":error.message}))?;
	Some(format!(
		"{operation}: {} (code {}). No automatic retry was made; refresh the saved settings before further action.",
		projected["summary"].as_str()?,
		error.code
	))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn settings_errors_keep_actionable_causes_but_hide_private_material() {
		use decodex_codex::app_server_client::RpcError;
		let mut error=RpcError{code:-32603,message:"failed to load configuration: /fixture/config.toml:1:24: unclosed array, expected `]`".into(),data:Some(json!({"private":"do-not-display"}))};
		let message = settings_error_message("Account setting failed", &error).unwrap();
		assert!(message.contains("/fixture/config.toml:1:24: unclosed array"));
		assert!(message.contains("code -32603"));
		assert!(message.contains("No automatic retry"));
		assert!(!message.contains("do-not-display"));
		error.message = "Bearer fixture-private-access-token-123456789".into();
		let message = settings_error_message("Account setting failed", &error).unwrap();
		assert!(message.contains("hidden"));
		assert!(!message.contains("fixture-private"));
		error.message = "x".repeat(8193);
		assert!(settings_error_message("Account setting failed", &error).is_none());
	}

	#[test]
	fn native_warning_keeps_only_safe_message_and_exact_optional_thread() {
		let value = warning(
			&json!({"threadId":"task","message":"Read failed\nRetained previous text","private":"hidden"}),
		)
		.unwrap();
		assert_eq!(
			value,
			json!({"threadId":"task","message":"Read failed\nRetained previous text"})
		);
		assert!(warning(&json!({"threadId":12,"message":"warning"})).is_none());
		assert!(warning(&json!({"threadId":"","message":"warning"})).is_none());
		assert!(warning(&json!({"message":"x".repeat(8193)})).is_none());
		assert!(warning(&json!({"message":"Bearer fixture-private-access-token-123456789"})).unwrap()["message"].as_str().unwrap().contains("hidden"));
		assert_eq!(
			warning_from_frame(br#"{"method":"warning","params":{"message":"global"}}"#).unwrap()["params"]
				["threadId"],
			Value::Null
		);
	}
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
