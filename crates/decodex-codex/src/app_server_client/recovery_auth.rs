//! Read process authentication metadata without requesting exported credentials.
use std::time::Duration;

use serde_json::Value;
use tokio::time;

use crate::app_server_client::{AppServerClient, ClientError, HistoryGuard};

/// Whether this native process can consume a ChatGPT account's recovery banner.
/// This is not proof of account identity, task ownership, or banner freshness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRecoveryAuth {
	/// Native provider requires OpenAI auth and currently uses ChatGPT authentication.
	ChatGpt,
	/// Custom provider, non-ChatGPT authentication, or signed-out process.
	Inapplicable,
	/// Required metadata is missing or malformed; no automatic switch is authorized.
	Unavailable,
}

impl AppServerClient {
	/// Read metadata once on the exact process. Never request a token or forced refresh.
	/// The caller must also revalidate account binding and native task/provider settings.
	pub async fn native_recovery_auth(
		&self,
		guard: HistoryGuard,
	) -> Result<NativeRecoveryAuth, ClientError> {
		let response = time::timeout(
			Duration::from_secs(8),
			self.request_with_history(
				"getAuthStatus",
				serde_json::json!({"includeToken":false,"refreshToken":false}),
				guard.clone(),
			),
		)
		.await
		.map_err(|_| ClientError::Io)??;

		if !guard.is_live() {
			return Err(ClientError::StaleHistory);
		}

		Ok(project(&response))
	}
}

fn project(value: &Value) -> NativeRecoveryAuth {
	if value.get("authToken").is_some_and(|token| !token.is_null()) {
		return NativeRecoveryAuth::Unavailable;
	}

	match value["requiresOpenaiAuth"].as_bool() {
		Some(false) => NativeRecoveryAuth::Inapplicable,
		Some(true) => match value.get("authMethod") {
			Some(Value::String(kind))
				if matches!(kind.as_str(), "chatgpt" | "chatgptAuthTokens") =>
				NativeRecoveryAuth::ChatGpt,
			Some(Value::Null) => NativeRecoveryAuth::Inapplicable,
			Some(Value::String(kind))
				if matches!(
					kind.as_str(),
					"apikey"
						| "headers"
						| "agentIdentity"
						| "personalAccessToken"
						| "workloadIdentity"
				) =>
				NativeRecoveryAuth::Inapplicable,
			_ => NativeRecoveryAuth::Unavailable,
		},
		None => NativeRecoveryAuth::Unavailable,
	}
}

#[cfg(test)]
mod tests {
	use tokio::{
		io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
		sync::oneshot,
		time,
	};

	use crate::app_server_client::recovery_auth::{
		self, AppServerClient, NativeRecoveryAuth, Value,
	};

	#[test]
	fn native_auth_never_infers_chatgpt_from_provider_name_or_partial_metadata() {
		for kind in ["chatgpt", "chatgptAuthTokens"] {
			assert_eq!(
				recovery_auth::project(
					&serde_json::json!({"authMethod":kind,"authToken":null,"requiresOpenaiAuth":true})
				),
				NativeRecoveryAuth::ChatGpt
			);
			assert_eq!(
				recovery_auth::project(
					&serde_json::json!({"authMethod":kind,"requiresOpenaiAuth":false})
				),
				NativeRecoveryAuth::Inapplicable
			);
		}
		for value in [
			serde_json::json!({}),
			serde_json::json!({"authMethod":"chatgpt"}),
			serde_json::json!({"authMethod":"future","requiresOpenaiAuth":true}),
			serde_json::json!({"authMethod":true,"requiresOpenaiAuth":true}),
			serde_json::json!({"authMethod":"chatgpt","requiresOpenaiAuth":true,"authToken":"synthetic-unrequested-token"}),
		] {
			assert_eq!(recovery_auth::project(&value), NativeRecoveryAuth::Unavailable);
		}
		for kind in [
			Value::Null,
			serde_json::json!("apikey"),
			serde_json::json!("headers"),
			serde_json::json!("agentIdentity"),
			serde_json::json!("personalAccessToken"),
		] {
			assert_eq!(
				recovery_auth::project(
					&serde_json::json!({"authMethod":kind,"requiresOpenaiAuth":true})
				),
				NativeRecoveryAuth::Inapplicable
			);
		}
	}

	#[tokio::test]
	async fn auth_metadata_read_does_not_request_tokens_refresh_or_mutation() {
		let (local, remote) = io::duplex(4_096);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let (release, released) = oneshot::channel::<()>();
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

			assert_eq!(request["method"], "getAuthStatus");
			assert_eq!(
				request["params"],
				serde_json::json!({"includeToken":false,"refreshToken":false})
			);

			w.write_all(format!("{}\n",serde_json::json!({"id":request["id"],"result":{"authMethod":"chatgptAuthTokens","authToken":null,"requiresOpenaiAuth":true}})).as_bytes()).await.unwrap();

			let _ = released.await;

			assert!(
				time::timeout(std::time::Duration::from_millis(20), lines.next_line())
					.await
					.is_err()
			);
		});
		let guard = client.thread_settings_guard("thread").unwrap();

		assert_eq!(client.native_recovery_auth(guard).await.unwrap(), NativeRecoveryAuth::ChatGpt);

		release.send(()).unwrap();
		server.await.unwrap();
	}
}
