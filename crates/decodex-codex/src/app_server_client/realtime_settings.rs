//! Resolve voice defaults from the owning server for each new conversation.
use super::{AppServerClient, ClientError};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

impl AppServerClient {
	/// Read the active thread's effective voice. Never uses a previous call's preference.
	/// Older servers and unknown voice names retain native start behavior.
	pub async fn realtime_voice_for_thread(
		&self,
		thread: &str,
	) -> Result<Option<String>, ClientError> {
		tokio::time::timeout(Duration::from_secs(15), async {
			let history = self.thread_read(json!({"threadId":thread})).await?;
			if history["thread"]["id"] != thread {
				return Err(ClientError::InvalidFrame);
			}
			let cwd = history["thread"]["cwd"]
				.as_str()
				.filter(|cwd| Path::new(cwd).is_absolute())
				.ok_or(ClientError::InvalidFrame)?;
			let config =
				match self.request("config/read", json!({"cwd":cwd,"includeLayers":true})).await {
					Ok(value) => value,
					Err(ClientError::Remote(error))
						if error.code == -32601
							|| (error.code == -32600
								&& error.message.contains("config/read")
								&& (error.message.contains("unknown variant")
									|| error.message.contains("unknown method"))) =>
						return Ok(None),
					Err(error) => return Err(error),
				};
			if !config["config"].is_object() {
				return Err(ClientError::InvalidFrame);
			}
			match &config["config"]["realtime"]["voice"] {
				Value::Null => {
					// V3 uses the V1 catalog. Cove is the upstream built-in default.
					let catalog = self.request("thread/realtime/listVoices", json!({})).await;
					let default = catalog
						.as_ref()
						.ok()
						.and_then(|value| value["voices"]["defaultV1"].as_str())
						.filter(|voice| known_voice(voice))
						.unwrap_or("cove");
					Ok(Some(default.into()))
				},
				Value::String(voice) => Ok(known_voice(voice).then(|| voice.clone())),
				_ => Err(ClientError::InvalidFrame),
			}
		})
		.await
		.map_err(|_| ClientError::Io)?
	}
}

pub(super) fn known_voice(voice: &str) -> bool {
	matches!(
		voice,
		"alloy"
			| "arbor" | "ash"
			| "ballad"
			| "breeze"
			| "cedar" | "coral"
			| "cove" | "echo"
			| "ember" | "juniper"
			| "maple" | "marin"
			| "sage" | "shimmer"
			| "sol" | "spruce"
			| "vale" | "verse"
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[tokio::test]
	async fn voice_start_reads_each_project_and_distinguishes_unsupported_from_failed_config() {
		for (config, catalog, expected) in [
			(
				json!({"result":{"config":{"realtime":{"voice":"juniper"}}}}),
				json!({}),
				Ok(Some("juniper")),
			),
			(
				json!({"result":{"config":{}}}),
				json!({"result":{"voices":{"defaultV1":"maple"}}}),
				Ok(Some("maple")),
			),
			(
				json!({"result":{"config":{}}}),
				json!({"error":{"code":-32601,"message":"missing"}}),
				Ok(Some("cove")),
			),
			(
				json!({"result":{"config":{"realtime":{"voice":"future_voice"}}}}),
				json!({}),
				Ok(None),
			),
			(json!({"error":{"code":-32601,"message":"missing"}}), json!({}), Ok(None)),
			(
				json!({"error":{"code":-32600,"message":"config/read unknown variant"}}),
				json!({}),
				Ok(None),
			),
			(
				json!({"error":{"code":-32600,"message":"invalid configuration"}}),
				json!({}),
				Err(()),
			),
			(json!({"result":{"config":{"realtime":{"voice":42}}}}), json!({}), Err(())),
		] {
			let (local, remote) = tokio::io::duplex(8192);
			let (read, write) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let server = tokio::spawn(async move {
				let (read, mut write) = tokio::io::split(remote);
				let mut lines = BufReader::new(read).lines();
				let mut cwd = String::new();
				while let Some(line) = lines.next_line().await.unwrap() {
					let request: Value = serde_json::from_str(&line).unwrap();
					let mut response = match request["method"].as_str().unwrap() {
						"thread/read" => {
							let thread = request["params"]["threadId"].as_str().unwrap();
							cwd = format!("/projects/{thread}");
							json!({"result":{"thread":{"id":thread,"cwd":cwd}}})
						},
						"config/read" => {
							assert_eq!(request["params"], json!({"cwd":cwd,"includeLayers":true}));
							config.clone()
						},
						"thread/realtime/listVoices" => catalog.clone(),
						other => panic!("unexpected request {other}"),
					};
					response["id"] = request["id"].clone();
					write.write_all(format!("{response}\n").as_bytes()).await.unwrap();
				}
			});
			for thread in ["first", "second", "first"] {
				let actual = client.realtime_voice_for_thread(thread).await;
				assert_eq!(actual.as_ref().map(|voice| voice.as_deref()).map_err(|_| ()), expected);
			}
			server.abort();
		}
	}
}
