//! Exact-turn reviewer publication. Native policy and pending approvals remain authoritative.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Explicit review routing for subsequently captured steps in one live turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveReviewer {
	/// Route new approval requests to the user.
	User,
	/// Route new approval requests through native automatic review.
	AutoReview,
}

/// Native publication receipt, not a readback of a later inference or approval.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveSettingsOutcome {
	/// Published for subsequent captures; pending requests and future defaults are unchanged.
	Applied,
	/// The exact task is no longer available. Never select a replacement task automatically.
	TargetUnavailable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ReviewerUpdate {
	thread_id: String,
	turn_id: String,
	approvals_reviewer: LiveReviewer,
}

/// Admit only an exact-turn reviewer edit through the retained native connection.
pub fn is_live_reviewer_update(value: &Value) -> bool {
	serde_json::from_value::<ReviewerUpdate>(value.clone()).is_ok_and(|update| {
		let _reviewer = update.approvals_reviewer;
		valid_id(&update.thread_id) && valid_id(&update.turn_id)
	})
}

fn valid_id(value: &str) -> bool {
	!value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

impl AppServerClient {
	/// Publish one reviewer-only change to the supplied task, with no model feature gate.
	/// The caller must bind the history guard and exact task to its current source and user action.
	/// Errors after submission are uncertain, never permission to retry or change thread defaults.
	pub async fn update_live_reviewer(
		&self,
		thread: &str,
		turn: &str,
		reviewer: LiveReviewer,
		guard: HistoryGuard,
	) -> Result<LiveSettingsOutcome, ClientError> {
		if !valid_id(thread) || !valid_id(turn) {
			return Err(ClientError::InvalidFrame);
		}
		let params = json!({"threadId":thread,"turnId":turn,"approvalsReviewer":reviewer});
		let value = tokio::time::timeout(
			std::time::Duration::from_secs(8),
			self.request_with_history("turn/settings/update", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Receipt {
			status: LiveSettingsOutcome,
		}
		serde_json::from_value::<Receipt>(value)
			.map(|receipt| receipt.status)
			.map_err(|_| ClientError::InvalidFrame)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[test]
	fn reviewer_update_cannot_smuggle_other_settings_or_missing_identity() {
		let good = json!({"threadId":"t","turnId":"u","approvalsReviewer":"user"});
		assert!(is_live_reviewer_update(&good));
		for (field, bad) in [
			("threadId", json!("")),
			("turnId", json!("\n")),
			("approvalsReviewer", Value::Null),
			("approvalsReviewer", json!("guardian_subagent")),
			("model", json!("other")),
			("approvalPolicy", json!("never")),
		] {
			let mut value = good.clone();
			value[field] = bad;
			assert!(!is_live_reviewer_update(&value));
		}
	}

	#[tokio::test]
	async fn exact_live_reviewer_receipt_is_not_a_future_default_or_retry() {
		for status in ["applied", "targetUnavailable", "unknown", "rejected", "lost"] {
			let (local, remote) = tokio::io::duplex(4096);
			let (r, w) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(r, w);
			let guard = client.history_guard(0).unwrap();
			let server = tokio::spawn(async move {
				let (r, mut w) = tokio::io::split(remote);
				let mut lines = BufReader::new(r).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "turn/settings/update");
				assert_eq!(
					request["params"],
					json!({"threadId":"thread","turnId":"original","approvalsReviewer":"auto_review"})
				);
				if status == "lost" {
					return;
				}
				let reply = if status == "rejected" {
					json!({"id":request["id"],"error":{"code":-32600,"message":"managed reviewer requirement"}})
				} else {
					json!({"id":request["id"],"result":{"status":status}})
				};
				w.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
				assert!(
					lines.next_line().await.unwrap().is_none(),
					"must not retry or mutate defaults"
				);
			});
			let result = client
				.update_live_reviewer("thread", "original", LiveReviewer::AutoReview, guard)
				.await;
			match status {
				"applied" => assert_eq!(result.unwrap(), LiveSettingsOutcome::Applied),
				"targetUnavailable" =>
					assert_eq!(result.unwrap(), LiveSettingsOutcome::TargetUnavailable),
				"rejected" => assert!(
					matches!(result, Err(ClientError::Remote(error)) if error.code == -32600)
				),
				"lost" => assert!(matches!(result, Err(ClientError::Closed | ClientError::Io))),
				_ => assert!(matches!(result, Err(ClientError::InvalidFrame))),
			}
			drop(client);
			server.await.unwrap();
		}
	}

	#[tokio::test]
	async fn reviewer_update_rejects_a_foreign_history_guard_before_dispatch() {
		let (local, remote) = tokio::io::duplex(4096);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let (other, _peer) = tokio::io::duplex(4096);
		let (r, w) = tokio::io::split(other);
		let (other, _other_events) = AppServerClient::from_io(r, w);
		let result = client
			.update_live_reviewer(
				"thread",
				"turn",
				LiveReviewer::User,
				other.history_guard(0).unwrap(),
			)
			.await;
		assert!(matches!(result, Err(ClientError::StaleHistory)));
		drop(client);
		assert!(BufReader::new(remote).lines().next_line().await.unwrap().is_none());
	}
}
