//! One explicit native workspace-owner notification. The caller owns account admission.
use super::{AppServerClient, ClientError};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Native notification purpose; never substitute one purpose after a failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountNudgeCreditType {
	/// Request additional workspace credits.
	Credits,
	/// Request a higher workspace usage limit.
	UsageLimit,
}

/// Bounded native outcome for one notification attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountNudgeOutcome {
	/// Native backend confirmed sending the notification.
	Sent,
	/// Native backend declined a repeated notification during its cooldown.
	CooldownActive,
	/// This installed process does not implement the method. No fallback was sent.
	Unsupported,
	/// Delivery cannot be established. Never automatically replay this attempt.
	Uncertain,
}

impl AppServerClient {
	/// Send once on this exact authenticated connection, without automatic retry.
	/// The owner must bind the process to the selected account/revision and persist an
	/// operation claim before calling. The native method has no account selector or
	/// idempotency parameter; transport errors and unrecognized replies are uncertain.
	pub async fn send_account_nudge(
		&self,
		credit_type: AccountNudgeCreditType,
	) -> AccountNudgeOutcome {
		let result = tokio::time::timeout(
			std::time::Duration::from_secs(15),
			self.request("account/sendAddCreditsNudgeEmail", json!({"creditType": credit_type})),
		)
		.await;
		let value = match result {
			Ok(Ok(value)) => value,
			Ok(Err(ClientError::Remote(error))) if error.code == -32601 =>
				return AccountNudgeOutcome::Unsupported,
			_ => return AccountNudgeOutcome::Uncertain,
		};
		#[derive(Deserialize)]
		#[serde(rename_all = "snake_case")]
		enum Status {
			Sent,
			CooldownActive,
		}
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Response {
			status: Status,
		}
		match serde_json::from_value::<Response>(value) {
			Ok(Response { status: Status::Sent }) => AccountNudgeOutcome::Sent,
			Ok(Response { status: Status::CooldownActive }) => AccountNudgeOutcome::CooldownActive,
			Err(_) => AccountNudgeOutcome::Uncertain,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	#[tokio::test]
	async fn native_nudge_keeps_exact_purpose_and_never_retries_unknown_delivery() {
		for (credit_type, response, expected) in [
			(
				AccountNudgeCreditType::Credits,
				json!({"result":{"status":"sent"}}),
				AccountNudgeOutcome::Sent,
			),
			(
				AccountNudgeCreditType::UsageLimit,
				json!({"result":{"status":"cooldown_active"}}),
				AccountNudgeOutcome::CooldownActive,
			),
			(
				AccountNudgeCreditType::Credits,
				json!({"result":{"status":"future_status"}}),
				AccountNudgeOutcome::Uncertain,
			),
			(
				AccountNudgeCreditType::Credits,
				json!({"error":{"code":-32601,"message":"unsupported"}}),
				AccountNudgeOutcome::Unsupported,
			),
			(
				AccountNudgeCreditType::Credits,
				json!({"error":{"code":-32603,"message":"delivery unknown"}}),
				AccountNudgeOutcome::Uncertain,
			),
		] {
			let (local, remote) = tokio::io::duplex(4096);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				let request: serde_json::Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "account/sendAddCreditsNudgeEmail");
				assert_eq!(request["params"], json!({"creditType":credit_type}));
				let mut reply = response;
				reply["id"] = request["id"].clone();
				writer.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
				// Any subsequent bytes would be an unauthorized replay of this attempt.
				assert!(
					tokio::time::timeout(std::time::Duration::from_millis(25), lines.next_line())
						.await
						.is_err()
				);
			});
			assert_eq!(client.send_account_nudge(credit_type).await, expected);
			server.await.unwrap();
			client.close();
		}
	}
	#[tokio::test]
	async fn connection_loss_after_request_is_uncertain() {
		let (local, remote) = tokio::io::duplex(4096);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let mut lines = BufReader::new(remote).lines();
			assert!(lines.next_line().await.unwrap().is_some());
		});
		assert_eq!(
			client.send_account_nudge(AccountNudgeCreditType::UsageLimit).await,
			AccountNudgeOutcome::Uncertain
		);
		server.await.unwrap();
	}
}
