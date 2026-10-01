//! Task-local ordinary model recovery. Native mode and permission policy remain authoritative.
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time;

use crate::app_server_client::{AppServerClient, ClientError, HistoryGuard};

/// One bounded model/effort update with explicit or preserved tier for an owned native task.
/// The owner must select these values from a fresh account-bound backend banner and catalog.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ThreadModelRecoveryUpdate {
	thread_id: String,
	model: String,
	effort: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	service_tier: Option<String>,
}
impl ThreadModelRecoveryUpdate {
	/// Construct a task-local update without permissions, collaboration mode, or global defaults.
	pub fn new(
		thread: &str,
		model: &str,
		effort: &str,
		service_tier: &str,
	) -> Result<Self, ClientError> {
		let update = Self {
			thread_id: thread.into(),
			model: model.into(),
			effort: effort.into(),
			service_tier: Some(service_tier.into()),
		};

		if update.valid() { Ok(update) } else { Err(ClientError::InvalidFrame) }
	}

	/// Change model and effort while preserving the native task's existing service tier.
	pub fn preserving_service_tier(
		thread: &str,
		model: &str,
		effort: &str,
	) -> Result<Self, ClientError> {
		let update = Self {
			thread_id: thread.into(),
			model: model.into(),
			effort: effort.into(),
			service_tier: None,
		};

		if update.valid() { Ok(update) } else { Err(ClientError::InvalidFrame) }
	}

	fn valid(&self) -> bool {
		fn text(value: &str, limit: usize) -> bool {
			!value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
		}

		text(&self.thread_id, 512)
			&& text(&self.model, 256)
			&& self.model != "gpt-reserve"
			&& text(&self.effort, 128)
			&& self.service_tier.as_deref().is_none_or(|tier| text(tier, 128))
	}
}

/// Native acknowledgment that a task-local settings change was queued.
/// This is not proof that a subsequent inference used the requested settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadModelRecoveryQueued;

impl AppServerClient {
	/// Queue one exact task update once. The caller owns source validation, serialization with
	/// manual selection, publication observation and persistence. Never replay input on failure.
	/// Omitted collaboration/permission fields preserve native settings and instructions.
	pub async fn queue_thread_model_recovery(
		&self,
		update: &ThreadModelRecoveryUpdate,
		guard: HistoryGuard,
	) -> Result<ThreadModelRecoveryQueued, ClientError> {
		if !update.valid() {
			return Err(ClientError::InvalidFrame);
		}

		let params = serde_json::to_value(update).map_err(|_| ClientError::InvalidFrame)?;
		let response = time::timeout(
			Duration::from_secs(8),
			self.request_with_history("thread/settings/update", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;

		if response.as_object().is_some_and(|object| object.is_empty()) {
			Ok(ThreadModelRecoveryQueued)
		} else {
			Err(ClientError::InvalidFrame)
		}
	}
}

/// Admit model and effort with an optional explicit tier; omission preserves the native tier.
pub fn is_thread_model_recovery_update(value: &Value) -> bool {
	!value.get("serviceTier").is_some_and(Value::is_null)
		&& serde_json::from_value::<ThreadModelRecoveryUpdate>(value.clone())
			.is_ok_and(|update| update.valid())
}

#[cfg(test)]
mod tests {
	use serde_json;
	use tokio::{
		io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
		time,
	};

	use crate::app_server_client::model_recovery::{
		self, AppServerClient, ThreadModelRecoveryUpdate, Value,
	};

	#[test]
	fn recovery_update_cannot_replace_task_policy_or_enter_reserve() {
		let good = serde_json::json!({"threadId":"thread","model":"replacement","effort":"medium","serviceTier":"default"});

		assert!(model_recovery::is_thread_model_recovery_update(&good));

		for (field, value) in [
			("model", serde_json::json!("gpt-reserve")),
			("model", serde_json::json!("")),
			("threadId", serde_json::json!("\n")),
			("effort", Value::Null),
			("serviceTier", Value::Null),
			("approvalPolicy", serde_json::json!("never")),
			("permissions", serde_json::json!("full-access")),
			("collaborationMode", serde_json::json!({})),
			("cwd", serde_json::json!("/tmp")),
			("disabledPluginIds", serde_json::json!([])),
		] {
			let mut bad = good.clone();

			bad[field] = value;

			assert!(!model_recovery::is_thread_model_recovery_update(&bad));
		}
		for key in ["model", "effort", "threadId"] {
			let mut missing = good.clone();

			missing.as_object_mut().unwrap().remove(key);

			assert!(!model_recovery::is_thread_model_recovery_update(&missing));
		}
	}

	#[test]
	fn preserving_tier_omits_the_field_instead_of_clearing_it() {
		let update =
			ThreadModelRecoveryUpdate::preserving_service_tier("thread", "replacement", "medium")
				.unwrap();
		let wire = serde_json::to_value(update).unwrap();

		assert_eq!(
			wire,
			serde_json::json!({"threadId":"thread","model":"replacement","effort":"medium"})
		);
		assert!(model_recovery::is_thread_model_recovery_update(&wire));
	}

	#[tokio::test]
	async fn model_update_accepts_only_empty_queue_ack_and_never_retries() {
		for response in [
			serde_json::json!({"result":{}}),
			serde_json::json!({"result":{"applied":true}}),
			serde_json::json!({"result":null}),
			serde_json::json!({"error":{"code":-32_601,"message":"unsupported"}}),
		] {
			let expected = response == serde_json::json!({"result":{}});
			let (local, remote) = io::duplex(4_096);
			let (reader, writer) = io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let guard = client.history_guard(0).unwrap();
			let server = tokio::spawn(async move {
				let (reader, mut writer) = io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(request["method"], "thread/settings/update");
				assert_eq!(
					request["params"],
					serde_json::json!({"threadId":"thread","model":"replacement","effort":"medium","serviceTier":"default"})
				);

				let mut response = response;

				response["id"] = request["id"].clone();

				writer.write_all(format!("{response}\n").as_bytes()).await.unwrap();

				assert!(
					time::timeout(std::time::Duration::from_millis(25), lines.next_line())
						.await
						.is_err()
				);
			});
			let update =
				ThreadModelRecoveryUpdate::new("thread", "replacement", "medium", "default")
					.unwrap();

			assert_eq!(client.queue_thread_model_recovery(&update, guard).await.is_ok(), expected);

			server.await.unwrap();
			client.close();
		}
	}
}
