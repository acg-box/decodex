//! Read estimates only while the process, account revision and task binding remain current.
use std::{
	future::Future,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::time;

use decodex_codex::app_server_client::{AppServerClient, ClientError};
use decodex_core::{AccountId, ProcessGenerationId};
use decodex_protocol::{AgentUsageEstimateResult, EntityId};

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

pub(crate) async fn read<F, Fut>(source: F) -> AgentUsageEstimateResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return AgentUsageEstimateResult::Unavailable;
	};
	let response = time::timeout(
		Duration::from_secs(65),
		before.client.thread_usage_estimate(&before.key.thread),
	)
	.await;
	let Some(after) = source().await else {
		return AgentUsageEstimateResult::Unavailable;
	};

	if before.key != after.key {
		return AgentUsageEstimateResult::Unavailable;
	}

	match response {
		Ok(Ok(Some(estimate))) => {
			let Ok(estimate) = serde_json::to_value(estimate).and_then(serde_json::from_value)
			else {
				return AgentUsageEstimateResult::Unavailable;
			};
			let (Ok(work_id), Ok(account_id)) = (
				EntityId::new(before.key.work),
				EntityId::new(before.key.account.as_str().to_owned()),
			) else {
				return AgentUsageEstimateResult::Unavailable;
			};
			let observed_at_micros = SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.ok()
				.and_then(|v| i64::try_from(v.as_micros()).ok());
			let Some(observed_at_micros) = observed_at_micros else {
				return AgentUsageEstimateResult::Unavailable;
			};

			AgentUsageEstimateResult::Available {
				work_id,
				account_id,
				observed_at_micros,
				estimate,
			}
		},
		Ok(Ok(None)) => AgentUsageEstimateResult::NotReported,
		Ok(Err(ClientError::Remote(error))) if error.code == -32_601 =>
			AgentUsageEstimateResult::Unsupported,
		Ok(Err(ClientError::CapacityExceeded)) => AgentUsageEstimateResult::CapacityExceeded,
		_ => AgentUsageEstimateResult::Unavailable,
	}
}

#[cfg(test)]
mod tests {
	use std::sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	};

	use serde_json::{self, Value};
	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	use crate::agent_usage_estimate::{
		self, AccountId, AgentUsageEstimateResult, AppServerClient, ProcessGenerationId, Source,
		SourceKey,
	};

	#[tokio::test]
	async fn task_usage_discards_reply_after_account_revision_process_or_thread_changes() {
		for change in ["none", "account", "revision", "history", "process", "thread", "closed"] {
			let (local, remote) = io::duplex(4_096);
			let (reader, writer) = io::split(local);
			let (client, _) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = io::split(remote);
				let request: Value = serde_json::from_str(
					&BufReader::new(reader).lines().next_line().await.unwrap().unwrap(),
				)
				.unwrap();

				assert_eq!(request["params"]["threadId"], "thread");

				writer.write_all(format!("{}\n",serde_json::json!({"id":request["id"],"result":{"threadUsage":{"threadId":"thread","estimatedUsageCreditsMicros":1,"groups":[]}}})).as_bytes()).await.unwrap();
			});
			let calls = Arc::new(AtomicUsize::new(0));
			let result = agent_usage_estimate::read(|| {
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
				assert!(matches!(result, AgentUsageEstimateResult::Available { .. }));
			} else {
				assert_eq!(result, AgentUsageEstimateResult::Unavailable, "{change}");
			}
		}
	}
}
