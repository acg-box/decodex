//! Refresh-only native diagnostics from the existing supervised process.
use decodex_codex::app_server_client::{AppServerClient, ClientError};
use decodex_core::ProcessGenerationId;
use decodex_protocol::NativeProcessDiagnostics as Result;
use serde_json::{Value, json};

pub(crate) async fn read(
	source: impl Fn() -> Option<(ProcessGenerationId, AppServerClient)>,
) -> Result {
	let Some((generation, client)) = source() else {
		return Result::Inactive;
	};
	let response = tokio::time::timeout(
		std::time::Duration::from_secs(2),
		client.request("server/diagnostics", json!({})),
	)
	.await;
	if source().is_none_or(|(current, _)| current != generation) {
		return Result::Unavailable;
	}
	match response {
		Ok(Ok(value)) => project(&value).unwrap_or(Result::Unavailable),
		Ok(Err(ClientError::Remote(error))) if error.code == -32601 => Result::Unsupported,
		_ => Result::Unavailable,
	}
}

fn project(value: &Value) -> Option<Result> {
	#[derive(serde::Deserialize)]
	#[serde(rename_all = "camelCase")]
	struct Process {
		id: u32,
		resident_memory_bytes: Option<u64>,
		physical_footprint_bytes: Option<u64>,
	}
	let process: Process = serde_json::from_value(value["process"].clone()).ok()?;
	(process.id != 0).then_some(Result::Available {
		process_id: process.id,
		resident_memory_bytes: process.resident_memory_bytes,
		physical_footprint_bytes: process.physical_footprint_bytes,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn resource_samples_keep_unknown_memory_and_ignore_unselected_gauges() {
		assert_eq!(
			project(
				&json!({"process":{"id":17,"residentMemoryBytes":42},"gauges":[{"name":"PRIVATE","value":7}]})
			),
			Some(Result::Available {
				process_id: 17,
				resident_memory_bytes: Some(42),
				physical_footprint_bytes: None
			})
		);
		for value in [
			json!({}),
			json!({"process":{"id":0}}),
			json!({"process":{"id":17,"residentMemoryBytes":-1}}),
		] {
			assert!(project(&value).is_none());
		}
	}
	#[tokio::test]
	async fn diagnostic_read_does_not_start_a_process_and_rejects_changed_generation() {
		use std::sync::atomic::{AtomicBool, Ordering};
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		assert_eq!(read(|| None).await, Result::Inactive);
		for state in ["same", "stopped", "replaced"] {
			let (local, remote) = tokio::io::duplex(4096);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let line = BufReader::new(reader).lines().next_line().await.unwrap().unwrap();
				let request: Value = serde_json::from_str(&line).unwrap();
				assert_eq!(request["method"], "server/diagnostics");
				writer
					.write_all(
						format!(
							"{}\n",
							json!({"id":request["id"],"result":{"process":{"id":17},"gauges":[]}})
						)
						.as_bytes(),
					)
					.await
					.unwrap();
			});
			let called = AtomicBool::new(false);
			let result = read(|| {
				let after_response = called.swap(true, Ordering::SeqCst);
				if after_response && state == "stopped" {
					None
				} else {
					Some((
						ProcessGenerationId::new(if after_response && state == "replaced" {
							"10000000-0000-4000-8000-000000000002"
						} else {
							"10000000-0000-4000-8000-000000000001"
						})
						.unwrap(),
						client.clone(),
					))
				}
			})
			.await;
			server.await.unwrap();
			assert_eq!(
				result,
				if state != "same" {
					Result::Unavailable
				} else {
					Result::Available {
						process_id: 17,
						resident_memory_bytes: None,
						physical_footprint_bytes: None,
					}
				}
			);
		}
	}
}
