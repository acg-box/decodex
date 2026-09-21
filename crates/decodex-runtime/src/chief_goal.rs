//! Native-owned goal state queried through the currently admitted process.
use super::ChiefHost;
use decodex_codex::app_server_client::ClientError;
use decodex_protocol::{ChiefGoalResult, ChiefNativeGoal};
use serde_json::{Value, json};

struct Source {
	identity: decodex_protocol::EntityId,
	thread: String,
	client: decodex_codex::app_server_client::AppServerClient,
}

impl ChiefHost {
	pub(crate) async fn goal_state(&self, work: &str) -> ChiefGoalResult {
		read(|| self.goal_source(work)).await
	}

	async fn goal_source(&self, work: &str) -> Result<Option<Source>, ()> {
		let identity = self.runtime_source().await.ok_or(())?;
		let (generation, client) = self.runtime.chief_catalog_client().ok_or(())?;
		let owner = self.store.get_chief_work_item(work.into()).await.map_err(|_| ())?;
		let Some(thread) = owner.codex_thread_id else {
			return Ok(None);
		};
		if !self
			.store
			.chief_thread_is_owned(work.into(), thread.clone(), Some(generation.as_str().into()))
			.await
			.unwrap_or(false)
		{
			return Err(());
		}
		Ok(Some(Source { identity, thread, client }))
	}
}

async fn read<F, Fut>(source: F) -> ChiefGoalResult
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Result<Option<Source>, ()>>,
{
	let before = match source().await {
		Ok(Some(source)) => source,
		Ok(None) => return ChiefGoalResult::Unbound,
		Err(()) => return ChiefGoalResult::Unavailable,
	};
	let result = tokio::time::timeout(
		std::time::Duration::from_secs(8),
		before.client.request("thread/goal/get", json!({"threadId":before.thread})),
	)
	.await;
	let Ok(Some(after)) = source().await else {
		return ChiefGoalResult::Unavailable;
	};
	if before.identity != after.identity || before.thread != after.thread {
		return ChiefGoalResult::Unavailable;
	}
	match result {
		Ok(Ok(value)) => match project(&value, &before.thread) {
			Ok(goal) => ChiefGoalResult::Available {
				source: before.identity,
				thread_id: before.thread,
				goal,
			},
			Err(()) => ChiefGoalResult::Unavailable,
		},
		Ok(Err(ClientError::Remote(error)))
			if error.code == -32601
				|| (error.code == -32600 && error.message == "goals feature is disabled") =>
			ChiefGoalResult::Unsupported,
		_ => ChiefGoalResult::Unavailable,
	}
}

fn project(value: &Value, thread: &str) -> Result<Option<ChiefNativeGoal>, ()> {
	let goal = value.get("goal").ok_or(())?;
	if goal.is_null() {
		return Ok(None);
	}
	let goal: ChiefNativeGoal = serde_json::from_value(goal.clone()).map_err(|_| ())?;
	if !goal.is_valid() || goal.thread_id != thread {
		return Err(());
	}
	Ok(Some(goal))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn native_goal_read_requires_exact_identity_and_complete_counters() {
		let goal = json!({"threadId":"thread","objective":"Finish task","status":"active","tokenBudget":100,"tokensUsed":12,"timeUsedSeconds":3,"createdAt":1,"updatedAt":2});
		assert_eq!(project(&json!({"goal":goal}), "thread").unwrap().unwrap().tokens_used, 12);
		assert!(project(&json!({"goal":goal}), "foreign").is_err());
		assert_eq!(project(&json!({"goal":null}), "thread"), Ok(None));
		assert!(project(&json!({}), "thread").is_err());
		for (field, value) in [
			("tokensUsed", json!(-1)),
			("timeUsedSeconds", Value::Null),
			("updatedAt", json!(0)),
			("tokenBudget", json!(0)),
			("objective", json!("")),
		] {
			let mut invalid = goal.clone();
			invalid[field] = value;
			assert!(project(&json!({"goal":invalid}), "thread").is_err(), "{field}");
		}
	}
	#[tokio::test]
	async fn goal_reply_is_discarded_when_source_or_binding_changes() {
		use decodex_codex::app_server_client::AppServerClient;
		use std::sync::atomic::{AtomicUsize, Ordering};
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		for change in
			["none", "account-process-revision", "history", "thread", "unbound", "unavailable"]
		{
			let (local, remote) = tokio::io::duplex(4096);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let request: Value = serde_json::from_str(
					&BufReader::new(reader).lines().next_line().await.unwrap().unwrap(),
				)
				.unwrap();
				assert_eq!(request["method"], "thread/goal/get");
				assert_eq!(request["params"], json!({"threadId":"thread"}));
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":{"goal":null}}))
							.as_bytes(),
					)
					.await
					.unwrap();
			});
			let calls = AtomicUsize::new(0);
			let result = read(|| {
				let later = calls.fetch_add(1, Ordering::SeqCst) > 0;
				let client = client.clone();
				async move {
					if later && change == "unbound" {
						return Ok(None);
					}
					if later && change == "unavailable" {
						return Err(());
					}
					let changed = later && matches!(change, "account-process-revision" | "history");
					Ok(Some(Source {
						identity: decodex_protocol::EntityId::new(if changed {
							"replacement"
						} else {
							"original"
						})
						.unwrap(),
						thread: if later && change == "thread" { "replacement" } else { "thread" }
							.into(),
						client,
					}))
				}
			})
			.await;
			server.await.unwrap();
			assert_eq!(calls.load(Ordering::SeqCst), 2);
			if change == "none" {
				assert!(matches!(result, ChiefGoalResult::Available { goal: None, .. }));
			} else {
				assert_eq!(result, ChiefGoalResult::Unavailable, "{change}");
			}
		}
	}
	#[tokio::test]
	async fn failed_goal_reads_never_become_authoritative_empty() {
		use decodex_codex::app_server_client::AppServerClient;
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		for (code, message, expected) in [
			(-32601, "unknown method", ChiefGoalResult::Unsupported),
			(-32600, "goals feature is disabled", ChiefGoalResult::Unsupported),
			(-32600, "thread unavailable", ChiefGoalResult::Unavailable),
			(-32603, "read failed", ChiefGoalResult::Unavailable),
		] {
			let (local, remote) = tokio::io::duplex(4096);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let request: Value = serde_json::from_str(
					&BufReader::new(reader).lines().next_line().await.unwrap().unwrap(),
				)
				.unwrap();
				assert_eq!(request["method"], "thread/goal/get");
				writer
					.write_all(
						format!(
							"{}\n",
							json!({"id":request["id"],"error":{"code":code,"message":message}})
						)
						.as_bytes(),
					)
					.await
					.unwrap();
			});
			let result = read(|| {
				let client = client.clone();
				async move {
					Ok(Some(Source {
						identity: decodex_protocol::EntityId::new("source").unwrap(),
						thread: "thread".into(),
						client,
					}))
				}
			})
			.await;
			server.await.unwrap();
			assert_eq!(result, expected);
		}
	}
}
