//! Refresh-only native diagnostics from the existing supervised process.
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tokio::time;

use decodex_codex::app_server_client::{AppServerClient, ClientError};
use decodex_core::ProcessGenerationId;
use decodex_protocol::NativeProcessDiagnostics as Result;

pub(crate) async fn read(
	source: impl Fn() -> Option<(ProcessGenerationId, AppServerClient)>,
) -> Result {
	let Some((generation, client)) = source() else {
		return Result::Inactive;
	};
	let response = time::timeout(
		Duration::from_secs(2),
		client.request("server/diagnostics", serde_json::json!({})),
	)
	.await;

	if source().is_none_or(|(current, _)| current != generation) {
		return Result::Unavailable;
	}

	match response {
		Ok(Ok(value)) => project(&value).unwrap_or(Result::Unavailable),
		Ok(Err(ClientError::Remote(error))) if error.code == -32_601 => Result::Unsupported,
		_ => Result::Unavailable,
	}
}

fn project(value: &Value) -> Option<Result> {
	#[derive(Deserialize)]
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
	use std::sync::atomic::{AtomicBool, Ordering};

	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	use crate::native_diagnostics::{self, AppServerClient, ProcessGenerationId, Result, Value};

	#[test]
	fn resource_samples_keep_unknown_memory_and_ignore_unselected_gauges() {
		assert_eq!(
			native_diagnostics::project(
				&serde_json::json!({"process":{"id":17,"residentMemoryBytes":42},"gauges":[{"name":"PRIVATE","value":7}]})
			),
			Some(Result::Available {
				process_id: 17,
				resident_memory_bytes: Some(42),
				physical_footprint_bytes: None
			})
		);

		for value in [
			serde_json::json!({}),
			serde_json::json!({"process":{"id":0}}),
			serde_json::json!({"process":{"id":17,"residentMemoryBytes":-1}}),
		] {
			assert!(native_diagnostics::project(&value).is_none());
		}
	}
	#[tokio::test]
	async fn diagnostic_read_does_not_start_a_process_and_rejects_changed_generation() {
		assert_eq!(native_diagnostics::read(|| None).await, Result::Inactive);

		for state in ["same", "stopped", "replaced"] {
			let (local, remote) = io::duplex(4_096);
			let (reader, writer) = io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = io::split(remote);
				let line = BufReader::new(reader).lines().next_line().await.unwrap().unwrap();
				let request: Value = serde_json::from_str(&line).unwrap();

				assert_eq!(request["method"], "server/diagnostics");

				writer
					.write_all(
						format!(
							"{}\n",
							serde_json::json!({"id":request["id"],"result":{"process":{"id":17},"gauges":[]}})
						)
						.as_bytes(),
					)
					.await
					.unwrap();
			});
			let called = AtomicBool::new(false);
			let result = native_diagnostics::read(|| {
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
