//! Bounded public configuration diagnostics. Never retain arbitrary provider fields.
use serde::Deserialize;
use serde_json::Value;

use crate::agent_usage_estimate::Source;
use decodex_codex::app_server_client::RpcError;
use decodex_core::ProcessGenerationId;
use decodex_database::{SqliteStore, StoreError};

pub(crate) fn from_frame(bytes: &[u8]) -> Option<Value> {
	#[derive(Deserialize)]
	struct Fields {
		summary: String,
		details: Option<String>,
	}

	#[derive(Deserialize)]
	struct Frame {
		params: Fields,
	}

	if bytes.len() > 32_768 {
		return None;
	}

	let frame: Frame = serde_json::from_slice(bytes).ok()?;

	notification(
		&serde_json::json!({"summary":frame.params.summary,"details":frame.params.details}),
	)
}

pub(crate) fn project(params: &Value) -> Option<Value> {
	let summary = params.get("summary")?.as_str()?;

	if summary.trim().is_empty() || summary.len() > 8_192 {
		return None;
	}

	let details = match params.get("details") {
		None | Some(Value::Null) => None,
		Some(Value::String(text)) if text.len() <= 16_384 => Some(text.as_str()),
		_ => return None,
	};
	let clean = |text: &str| {
		if decodex_core::contains_credential_material(text) {
			"[Configuration detail hidden because it may contain credentials]".to_owned()
		} else {
			text.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect()
		}
	};

	Some(serde_json::json!({"summary":clean(summary),"details":details.map(clean)}))
}

pub(crate) fn notification(params: &Value) -> Option<Value> {
	Some(serde_json::json!({"method":"configWarning","params":project(params)?}))
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
	let projected = project(&serde_json::json!({"summary":message}))?;

	Some(serde_json::json!({"threadId":thread,"message":projected["summary"]}))
}

pub(crate) fn warning_from_frame(bytes: &[u8]) -> Option<Value> {
	if bytes.len() > 32_768 {
		return None;
	}

	let frame: Value = serde_json::from_slice(bytes).ok()?;

	Some(serde_json::json!({"method":"warning","params":warning(frame.get("params")?)?}))
}

pub(crate) async fn record(
	store: &SqliteStore,
	root: &str,
	generation: &ProcessGenerationId,
	params: &Value,
) -> Result<(), StoreError> {
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

	store.record_agent_config_warning(root.into(), generation.as_str().into(), digest, text).await
}

/// Shared host dispatch for process and task diagnostics.
pub(crate) async fn record_notification(
	store: &SqliteStore,
	root: &str,
	generation: &ProcessGenerationId,
	method: &str,
	params: &Value,
) -> Result<(), StoreError> {
	match method {
		"warning" => record_warning(store, root, generation, params).await,
		"configWarning" => record(store, root, generation, params).await,
		"mcpServer/startupStatus/updated" => {
			if let Some(value) = mcp_reauthentication_notice(params) {
				record_warning(store, root, generation, &value).await?;
			}

			Ok(())
		},
		_ => Ok(()),
	}
}

pub(crate) async fn record_warning(
	store: &SqliteStore,
	root: &str,
	generation: &ProcessGenerationId,
	params: &Value,
) -> Result<(), StoreError> {
	use sha2::{Digest as _, Sha256};

	let Some(value) = warning(params) else {
		return Ok(());
	};
	let text = format!("Codex warning: {}", value["message"].as_str().unwrap_or_default());
	let digest = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();

	store
		.record_agent_native_warning(
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
	store: &SqliteStore,
	source: &Source,
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
		&serde_json::json!({"threadId":source.key.thread,"message":message}),
	)
	.await;
}

fn mcp_reauthentication_notice(params: &Value) -> Option<Value> {
	if params["status"] != "failed" || params["failureReason"] != "reauthenticationRequired" {
		return None;
	}

	let name = params["name"].as_str()?.trim();

	if name.is_empty() || name.len() > 512 {
		return None;
	}

	warning(
		&serde_json::json!({"threadId":params["threadId"],"message":format!("MCP server {name} needs you to sign in again. Open Codex for this account to reconnect it.")}),
	)
}

fn settings_error_message(operation: &str, error: &RpcError) -> Option<String> {
	let projected = project(&serde_json::json!({"summary":error.message}))?;

	Some(format!(
		"{operation}: {} (code {}). No automatic retry was made; refresh the saved settings before further action.",
		projected["summary"].as_str()?,
		error.code
	))
}

#[cfg(test)]
mod tests {
	use crate::native_config_warning::{self, Value};
	use decodex_codex::app_server_client::RpcError;

	#[test]
	fn mcp_reauthentication_requires_explicit_native_cause() {
		let mut params = serde_json::json!({"threadId":"thread","name":"docs","status":"failed","failureReason":"reauthenticationRequired","error":"PRIVATE_ERROR"});
		let notice = native_config_warning::mcp_reauthentication_notice(&params).unwrap();

		assert_eq!(notice["threadId"], "thread");
		assert!(notice["message"].as_str().unwrap().contains("Open Codex for this account"));
		assert!(!notice.to_string().contains("PRIVATE_ERROR"));

		params["failureReason"] = Value::Null;

		assert!(native_config_warning::mcp_reauthentication_notice(&params).is_none());

		params["failureReason"] = serde_json::json!("reauthenticationRequired");
		params["status"] = serde_json::json!("ready");

		assert!(native_config_warning::mcp_reauthentication_notice(&params).is_none());
	}
	#[test]
	fn settings_errors_keep_actionable_causes_but_hide_private_material() {
		let mut error=RpcError{code:-32_603,message:"failed to load configuration: /fixture/config.toml:1:24: unclosed array, expected `]`".into(),data:Some(serde_json::json!({"private":"do-not-display"}))};
		let message =
			native_config_warning::settings_error_message("Account setting failed", &error)
				.unwrap();

		assert!(message.contains("/fixture/config.toml:1:24: unclosed array"));
		assert!(message.contains("code -32603"));
		assert!(message.contains("No automatic retry"));
		assert!(!message.contains("do-not-display"));

		error.message = "Bearer fixture-private-access-token-123456789".into();

		let message =
			native_config_warning::settings_error_message("Account setting failed", &error)
				.unwrap();

		assert!(message.contains("hidden"));
		assert!(!message.contains("fixture-private"));

		error.message = "x".repeat(8_193);

		assert!(
			native_config_warning::settings_error_message("Account setting failed", &error)
				.is_none()
		);
	}

	#[test]
	fn native_warning_keeps_only_safe_message_and_exact_optional_thread() {
		let value = native_config_warning::warning(
			&serde_json::json!({"threadId":"task","message":"Read failed\nRetained previous text","private":"hidden"}),
		)
		.unwrap();

		assert_eq!(
			value,
			serde_json::json!({"threadId":"task","message":"Read failed\nRetained previous text"})
		);
		assert!(
			native_config_warning::warning(&serde_json::json!({"threadId":12,"message":"warning"}))
				.is_none()
		);
		assert!(
			native_config_warning::warning(&serde_json::json!({"threadId":"","message":"warning"}))
				.is_none()
		);
		assert!(
			native_config_warning::warning(&serde_json::json!({"message":"x".repeat(8_193)}))
				.is_none()
		);
		assert!(
			native_config_warning::warning(
				&serde_json::json!({"message":"Bearer fixture-private-access-token-123456789"})
			)
			.unwrap()["message"]
				.as_str()
				.unwrap()
				.contains("hidden")
		);
		assert_eq!(
			native_config_warning::warning_from_frame(
				br#"{"method":"warning","params":{"message":"global"}}"#
			)
			.unwrap()["params"]["threadId"],
			Value::Null
		);
	}
	#[test]
	fn only_bounded_public_diagnostics_are_projected() {
		let warning = native_config_warning::project(&serde_json::json!({"summary":"Ignored \"setting\"","details":"one\ntwo","path":"private","unknown":"secret"})).unwrap();

		assert_eq!(
			warning,
			serde_json::json!({"summary":"Ignored \"setting\"","details":"one\ntwo"})
		);

		for bad in [
			serde_json::json!({}),
			serde_json::json!({"summary":" "}),
			serde_json::json!({"summary":1}),
			serde_json::json!({"summary":"x","details":false}),
			serde_json::json!({"summary":"x".repeat(8_193)}),
			serde_json::json!({"summary":"x","details":"x".repeat(16_385)}),
		] {
			assert!(native_config_warning::project(&bad).is_none());
		}

		let secret = "Bearer fixture-private-access-token-123456789";
		let safe = native_config_warning::project(
			&serde_json::json!({"summary":"Invalid header","details":secret}),
		)
		.unwrap();

		assert!(!safe.to_string().contains(secret));
	}
}
