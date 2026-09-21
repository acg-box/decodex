//! Read estimates only while the process, account revision and task binding remain current.
use decodex_codex::app_server_client::{AppServerClient, ClientError};
use decodex_core::{AccountId, ProcessGenerationId};
use decodex_protocol::ChiefUsageEstimateResult;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceKey {
	pub generation: ProcessGenerationId,
	pub account: AccountId,
	pub revision: i64,
	pub history_revision: u64,
	pub thread: String,
	pub work: String,
}
pub(crate) struct Source {
	pub key: SourceKey,
	pub client: AppServerClient,
}

pub(crate) async fn read<F, Fut>(source: F) -> ChiefUsageEstimateResult
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use ChiefUsageEstimateResult as Result;
	let Some(before) = source().await else {
		return Result::Unavailable;
	};
	let response = tokio::time::timeout(
		std::time::Duration::from_secs(65),
		before.client.thread_usage_estimate(&before.key.thread),
	)
	.await;
	let Some(after) = source().await else {
		return Result::Unavailable;
	};
	if before.key != after.key {
		return Result::Unavailable;
	}
	match response {
		Ok(Ok(Some(estimate))) => {
			let Ok(estimate) = serde_json::to_value(estimate).and_then(serde_json::from_value)
			else {
				return Result::Unavailable;
			};
			let (Ok(work_id), Ok(account_id)) = (
				decodex_protocol::EntityId::new(before.key.work),
				decodex_protocol::EntityId::new(before.key.account.as_str().to_owned()),
			) else {
				return Result::Unavailable;
			};
			let observed_at_micros = std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.ok()
				.and_then(|v| i64::try_from(v.as_micros()).ok());
			let Some(observed_at_micros) = observed_at_micros else {
				return Result::Unavailable;
			};
			Result::Available { work_id, account_id, observed_at_micros, estimate }
		},
		Ok(Ok(None)) => Result::NotReported,
		Ok(Err(ClientError::Remote(error))) if error.code == -32601 => Result::Unsupported,
		Ok(Err(ClientError::CapacityExceeded)) => Result::CapacityExceeded,
		_ => Result::Unavailable,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::{Value, json};
	use std::sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	};
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	#[tokio::test]
	async fn task_usage_discards_reply_after_account_revision_process_or_thread_changes() {
		for change in ["none", "account", "revision", "history", "process", "thread", "closed"] {
			let (local, remote) = tokio::io::duplex(4096);
			let (reader, writer) = tokio::io::split(local);
			let (client, _) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let request: Value = serde_json::from_str(
					&BufReader::new(reader).lines().next_line().await.unwrap().unwrap(),
				)
				.unwrap();
				assert_eq!(request["params"]["threadId"], "thread");
				writer.write_all(format!("{}\n",json!({"id":request["id"],"result":{"threadUsage":{"threadId":"thread","estimatedUsageCreditsMicros":1,"groups":[]}}})).as_bytes()).await.unwrap();
			});
			let calls = Arc::new(AtomicUsize::new(0));
			let result = read(|| {
				let later = calls.fetch_add(1, Ordering::SeqCst) > 0;
				let client = client.clone();
				async move {
					if later && change == "closed" {
						return None;
					}
					Some(Source {
						client,
						key: SourceKey {
							history_revision: u64::from(later && change == "history"),
							generation: ProcessGenerationId::new(if later && change == "process" {
								"20000000-0000-4000-8000-000000000002"
							} else {
								"10000000-0000-4000-8000-000000000001"
							})
							.unwrap(),
							account: AccountId::new(if later && change == "account" {
								"40000000-0000-4000-8000-000000000004"
							} else {
								"30000000-0000-4000-8000-000000000003"
							})
							.unwrap(),
							revision: if later && change == "revision" { 2 } else { 1 },
							thread: if later && change == "thread" { "other" } else { "thread" }
								.into(),
							work: "work".into(),
						},
					})
				}
			})
			.await;
			server.await.unwrap();
			if change == "none" {
				assert!(matches!(result, ChiefUsageEstimateResult::Available { .. }));
			} else {
				assert_eq!(result, ChiefUsageEstimateResult::Unavailable, "{change}");
			}
		}
	}
}
