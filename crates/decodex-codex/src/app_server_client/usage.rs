//! Native backend estimates, distinct from live token counters and account quotas.
use super::{AppServerClient, ClientError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Provider estimates for one exact thread; integer micros preserve precision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUsageEstimate {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Provider-reported estimated credits in millionths of one credit.
	pub estimated_usage_credits_micros: u64,
	/// Provider-reported estimated USD in millionths, absent when not reported.
	pub estimated_usage_usd_micros: Option<u64>,
	/// Provider groups; do not reconstruct a total from overlapping token subtotals.
	pub groups: Vec<ThreadUsageEstimateGroup>,
}

/// A model, effort and speed group, retaining missing counts as unknown.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUsageEstimateGroup {
	/// Provider model identifier, when reported.
	pub model: Option<String>,
	/// Provider effort, when reported.
	pub reasoning_effort: Option<String>,
	/// Provider speed, when reported.
	pub speed: Option<String>,
	/// Estimated credits in millionths.
	pub estimated_usage_credits_micros: u64,
	/// Net new input, if reported.
	pub net_new_input_tokens: Option<u64>,
	/// Cached input, if reported.
	pub cached_input_tokens: Option<u64>,
	/// Total input, including cached input when so defined by the provider.
	pub input_tokens: Option<u64>,
	/// Output, if reported.
	pub output_tokens: Option<u64>,
	/// Provider total; never synthesized from other fields.
	pub total_tokens: Option<u64>,
}

impl AppServerClient {
	/// Read backend estimates without a model turn. None means unavailable data, not zero.
	pub async fn thread_usage_estimate(
		&self,
		thread: &str,
	) -> Result<Option<ThreadUsageEstimate>, ClientError> {
		let response = self.request("account/usage/read", json!({"threadId":thread})).await?;
		if !response.is_object() {
			return Err(ClientError::InvalidFrame);
		}
		let Some(raw) = response.get("threadUsage").filter(|value| !value.is_null()) else {
			return Ok(None);
		};
		decode(raw, thread).map(Some)
	}
}

fn decode(raw: &Value, thread: &str) -> Result<ThreadUsageEstimate, ClientError> {
	if serde_json::to_vec(raw).map_err(|_| ClientError::InvalidFrame)?.len() > 65536 {
		return Err(ClientError::CapacityExceeded);
	}
	let result: ThreadUsageEstimate =
		serde_json::from_value(raw.clone()).map_err(|_| ClientError::InvalidFrame)?;
	if result.groups.len() > 128 {
		return Err(ClientError::CapacityExceeded);
	}
	if result.thread_id != thread {
		return Err(ClientError::InvalidFrame);
	}
	let valid = |value: u64| value <= i64::MAX as u64;
	if !valid(result.estimated_usage_credits_micros)
		|| !result.estimated_usage_usd_micros.is_none_or(valid)
		|| result.groups.iter().any(|group| {
			!valid(group.estimated_usage_credits_micros)
				|| [
					group.net_new_input_tokens,
					group.cached_input_tokens,
					group.input_tokens,
					group.output_tokens,
					group.total_tokens,
				]
				.into_iter()
				.flatten()
				.any(|v| !valid(v))
				|| [&group.model, &group.reasoning_effort, &group.speed]
					.into_iter()
					.flatten()
					.any(|v| v.len() > 4096)
		}) {
		return Err(ClientError::InvalidFrame);
	}
	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	#[tokio::test]
	async fn native_thread_usage_retains_precision_unknowns_and_exact_identity() {
		for (returned, accepted) in [("thread", true), ("another-thread", false)] {
			let (local, remote) = tokio::io::duplex(65536);
			let (read, write) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let task = tokio::spawn(async move {
				let (read, mut write) = tokio::io::split(remote);
				let request: Value = serde_json::from_str(
					&BufReader::new(read).lines().next_line().await.unwrap().unwrap(),
				)
				.unwrap();
				assert_eq!(request["method"], "account/usage/read");
				assert_eq!(request["params"], json!({"threadId":"thread"}));
				write.write_all(format!("{}\n",json!({"id":request["id"],"result":{"threadUsage":{"threadId":returned,"estimatedUsageCreditsMicros":9007199254740993u64,"estimatedUsageUsdMicros":null,"groups":[{"estimatedUsageCreditsMicros":0,"inputTokens":0,"cachedInputTokens":null}]}}})).as_bytes()).await.unwrap();
			});
			let result = client.thread_usage_estimate("thread").await;
			task.await.unwrap();
			if accepted {
				let usage = result.unwrap().unwrap();
				assert_eq!(usage.estimated_usage_credits_micros, 9007199254740993);
				assert_eq!(usage.estimated_usage_usd_micros, None);
				assert_eq!(usage.groups[0].input_tokens, Some(0));
				assert_eq!(usage.groups[0].cached_input_tokens, None);
			} else {
				assert!(matches!(result, Err(ClientError::InvalidFrame)));
			}
		}
	}
	#[tokio::test]
	async fn native_thread_usage_keeps_missing_null_and_failure_distinct() {
		for (result, absent) in
			[(json!({}), true), (json!({"threadUsage":null}), true), (json!([]), false)]
		{
			let (local, remote) = tokio::io::duplex(4096);
			let (read, write) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let task = tokio::spawn(async move {
				let (read, mut write) = tokio::io::split(remote);
				let request: Value = serde_json::from_str(
					&BufReader::new(read).lines().next_line().await.unwrap().unwrap(),
				)
				.unwrap();
				write
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			});
			let response = client.thread_usage_estimate("thread").await;
			task.await.unwrap();
			if absent {
				assert!(response.unwrap().is_none());
			} else {
				assert!(matches!(response, Err(ClientError::InvalidFrame)));
			}
		}
	}

	#[test]
	fn thread_usage_rejects_negative_and_out_of_range_estimates() {
		for value in [json!(-1), json!(u64::MAX)] {
			assert!(
				decode(
					&json!({"threadId":"thread","estimatedUsageCreditsMicros":value,"groups":[]}),
					"thread"
				)
				.is_err()
			);
		}
	}
}
