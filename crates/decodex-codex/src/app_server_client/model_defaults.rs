//! Read native creation defaults without selecting a model or creating a thread.
use super::{AppServerClient, ClientError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Defaults from one native configuration source, before explicit user overrides.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct NativeExecutionDefaults {
	/// Configured model; absent means this source supplies no default.
	pub model: Option<String>,
	/// Configured effort, including future provider values.
	pub reasoning_effort: Option<String>,
	/// Configured service tier; no automatic selection or billing consent is implied.
	pub service_tier: Option<String>,
}

/// Keep configured values and managed new-thread defaults separate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeModelDefaults {
	/// Effective user/project configuration at the requested working directory.
	pub configured: NativeExecutionDefaults,
	/// Managed creation defaults, not enforced overrides of explicit user choices.
	pub managed: NativeExecutionDefaults,
}

fn optional(value: Option<&Value>, limit: usize) -> Result<Option<String>, ClientError> {
	match value {
		None | Some(Value::Null) => Ok(None),
		Some(Value::String(v))
			if !v.trim().is_empty() && v.len() <= limit && !v.chars().any(char::is_control) =>
			Ok(Some(v.clone())),
		_ => Err(ClientError::InvalidFrame),
	}
}
fn project(value: &Value, managed: bool) -> Result<NativeExecutionDefaults, ClientError> {
	let source = if managed {
		let requirements = value.get("requirements").ok_or(ClientError::InvalidFrame)?;
		if requirements.is_null() {
			return Ok(NativeExecutionDefaults::default());
		}
		let requirements = requirements.as_object().ok_or(ClientError::InvalidFrame)?;
		let Some(models) = requirements.get("models").filter(|v| !v.is_null()) else {
			return Ok(NativeExecutionDefaults::default());
		};
		let models = models.as_object().ok_or(ClientError::InvalidFrame)?;
		let Some(defaults) = models.get("newThread").filter(|v| !v.is_null()) else {
			return Ok(NativeExecutionDefaults::default());
		};
		defaults.as_object().ok_or(ClientError::InvalidFrame)?
	} else {
		value.get("config").and_then(Value::as_object).ok_or(ClientError::InvalidFrame)?
	};
	Ok(NativeExecutionDefaults {
		model: optional(source.get("model"), 512)?,
		reasoning_effort: optional(
			source.get(if managed { "modelReasoningEffort" } else { "model_reasoning_effort" }),
			128,
		)?,
		service_tier: optional(
			source.get(if managed { "serviceTier" } else { "service_tier" }),
			128,
		)?,
	})
}

impl NativeExecutionDefaults {
	/// Project only creation fields from a native config/read response.
	pub fn from_config_response(value: &Value) -> Result<Self, ClientError> {
		project(value, false)
	}

	/// Project managed new-thread defaults, preserving absence without inventing a fallback.
	pub fn from_requirements_response(value: &Value) -> Result<Self, ClientError> {
		project(value, true)
	}
}

impl AppServerClient {
	/// Read only bounded creation-default fields at an exact absolute directory.
	/// The caller must verify account/process/directory ownership before and after this read.
	/// No values are applied and no thread, turn, config write or automatic retry is created.
	pub async fn initial_model_defaults(
		&self,
		cwd: &str,
	) -> Result<NativeModelDefaults, ClientError> {
		if !std::path::Path::new(cwd).is_absolute()
			|| cwd.len() > 4096
			|| cwd.chars().any(char::is_control)
		{
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(std::time::Duration::from_secs(8), async {
			let configured =
				self.request("config/read", json!({"cwd":cwd,"includeLayers":false})).await?;
			let configured = NativeExecutionDefaults::from_config_response(&configured)?;
			let managed = self.request("configRequirements/read", json!({})).await?;
			Ok(NativeModelDefaults {
				configured,
				managed: NativeExecutionDefaults::from_requirements_response(&managed)?,
			})
		})
		.await
		.map_err(|_| ClientError::Io)?
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	#[test]
	fn projection_preserves_sources_and_omits_unrelated_sensitive_fields() {
		let configured=project(&json!({"config":{"model":"project-model","model_reasoning_effort":"future-effort","service_tier":"flex","model_providers":{"secret":"never-public"}}}),false).unwrap();
		let managed=project(&json!({"requirements":{"models":{"newThread":{"model":"managed-model","modelReasoningEffort":"low","serviceTier":"default"}}}}),true).unwrap();
		assert_eq!(configured.model.as_deref(), Some("project-model"));
		assert_eq!(configured.reasoning_effort.as_deref(), Some("future-effort"));
		assert_eq!(configured.service_tier.as_deref(), Some("flex"));
		assert_eq!(managed.model.as_deref(), Some("managed-model"));
		assert_eq!(managed.reasoning_effort.as_deref(), Some("low"));
		assert!(!serde_json::to_string(&configured).unwrap().contains("never-public"));
		assert_eq!(
			project(&json!({"requirements":null}), true).unwrap(),
			NativeExecutionDefaults::default()
		);
		assert_eq!(
			project(&json!({"config":{}}), false).unwrap(),
			NativeExecutionDefaults::default()
		);
		for invalid in [
			json!({}),
			json!({"config":null}),
			json!({"config":{"model":42}}),
			json!({"config":{"service_tier":"\n"}}),
		] {
			assert!(project(&invalid, false).is_err());
		}
		for invalid in [
			json!({}),
			json!({"requirements":[]}),
			json!({"requirements":{"models":{"newThread":42}}}),
		] {
			assert!(project(&invalid, true).is_err());
		}
	}
	#[tokio::test]
	async fn reads_exact_directory_and_requirements_without_launch_or_write() {
		let (local, remote) = tokio::io::duplex(8192);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let server = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();
			for (method, params, result) in [
				(
					"config/read",
					json!({"cwd":"/workspace/saved","includeLayers":false}),
					json!({"config":{"model":"project"}}),
				),
				(
					"configRequirements/read",
					json!({}),
					json!({"requirements":{"models":{"newThread":{"model":"managed"}}}}),
				),
			] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				assert_eq!(request["params"], params);
				w.write_all(
					format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
				)
				.await
				.unwrap();
			}
		});
		assert!(client.initial_model_defaults("relative").await.is_err());
		let values = client.initial_model_defaults("/workspace/saved").await.unwrap();
		assert_eq!(values.configured.model.as_deref(), Some("project"));
		assert_eq!(values.managed.model.as_deref(), Some("managed"));
		server.await.unwrap();
	}
}
