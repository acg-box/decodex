//! Native goal observations, independent of application-owned coordination goals.
use super::{AppServerClient, ClientError};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Native scheduler state; a token limit is distinct from account usage limits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeThreadGoalStatus {
	/// The native goal can continue work.
	Active,
	/// The native goal is paused.
	Paused,
	/// The native goal needs external progress.
	Blocked,
	/// Account usage prevents progress.
	UsageLimited,
	/// The explicit goal budget prevents progress.
	BudgetLimited,
	/// The native goal has completed.
	Complete,
}

/// One native goal snapshot; counters belong to this goal, not all thread history.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeThreadGoal {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Native objective text.
	pub objective: String,
	/// Native scheduler state.
	pub status: NativeThreadGoalStatus,
	/// Explicit token budget; null means no configured budget.
	pub token_budget: Option<i64>,
	/// Native goal token counter.
	pub tokens_used: i64,
	/// Native goal elapsed time in seconds.
	pub time_used_seconds: i64,
	/// Native creation timestamp in Unix seconds.
	pub created_at: i64,
	/// Native last-update timestamp in Unix seconds.
	pub updated_at: i64,
}

impl AppServerClient {
	/// Read the exact native goal without changing its status or starting model work.
	pub async fn thread_goal(&self, thread: &str) -> Result<Option<NativeThreadGoal>, ClientError> {
		let response = self.request("thread/goal/get", json!({"threadId":thread})).await?;
		let raw = response.get("goal").ok_or(ClientError::InvalidFrame)?;
		if raw.is_null() {
			return Ok(None);
		}
		let goal: NativeThreadGoal =
			serde_json::from_value(raw.clone()).map_err(|_| ClientError::InvalidFrame)?;
		if goal.thread_id != thread
			|| goal.tokens_used < 0
			|| goal.time_used_seconds < 0
			|| goal.token_budget.is_some_and(|budget| budget <= 0)
		{
			return Err(ClientError::InvalidFrame);
		}
		Ok(Some(goal))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::Value;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	#[tokio::test]
	async fn native_goal_reads_distinguish_absence_limits_and_malformed_receipts() {
		let base = json!({"threadId":"thread","objective":"Native objective","status":"paused","tokenBudget":null,"tokensUsed":23,"timeUsedSeconds":7,"createdAt":10,"updatedAt":17});
		let mut cases = vec![(json!({"goal":null}), true), (json!({}), false)];
		for status in ["active", "paused", "blocked", "usageLimited", "budgetLimited", "complete"] {
			let mut goal = base.clone();
			goal["status"] = json!(status);
			cases.push((json!({"goal":goal}), true));
		}
		for (key, value) in [
			("threadId", json!("other")),
			("tokensUsed", json!(-1)),
			("timeUsedSeconds", json!(-1)),
			("tokenBudget", json!(0)),
			("status", json!("future")),
		] {
			let mut goal = base.clone();
			goal[key] = value;
			cases.push((json!({"goal":goal}), false));
		}
		for (result, valid) in cases {
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
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			});
			assert_eq!(client.thread_goal("thread").await.is_ok(), valid);
			server.await.unwrap();
		}
	}
}
