//! Observe native reviewer details without overriding conversation permissions.
use std::time::Duration;

use sha2::{Digest as _, Sha256};
use tokio::time;

use crate::agent::{AgentCoordinator, AgentError, Value, native_subagents};
use decodex_core::guardian::ReviewDetails;

impl AgentCoordinator {
	pub(super) async fn global_guardian_notification(
		&self,
		method: &str,
		params: &Value,
	) -> Result<bool, AgentError> {
		if !matches!(method, "rawResponseItem/completed" | "turn/completed") {
			return Ok(false);
		}

		let Some(thread) = params["threadId"].as_str() else { return Ok(false) };
		let settings = self.store.guardian_settings().await?;

		if !settings.review_details {
			return Ok(false);
		}

		let owned = self.store.list_agent_work_items().await?;

		if owned.iter().any(|a| a.codex_thread_id.as_deref() == Some(thread)) {
			return Ok(false);
		}

		let Ok(Ok(native)) = time::timeout(
			Duration::from_secs(8),
			self.client.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false})),
		)
		.await
		else {
			return Ok(false);
		};

		if native["thread"]["id"] != thread
			|| native.pointer("/thread/source/subAgent/other").and_then(Value::as_str)
				!= Some("guardian")
		{
			return Ok(false);
		}

		let Some(parent) = native["thread"]["parentThreadId"].as_str() else { return Ok(false) };

		if native_subagents::request_owner(&self.store, &self.client, parent).await.is_err() {
			return Ok(false);
		}

		let turn = if method == "turn/completed" {
			params["turn"]["id"].as_str()
		} else {
			params["turnId"].as_str()
		};
		let Some(turn) = turn else { return Ok(true) };

		if method == "rawResponseItem/completed"
			&& (params["item"]["type"] != "message" || params["item"]["role"] != "assistant")
		{
			return Ok(true);
		}

		// A terminal notification can include canonical items. Keep only lifecycle
		// facts here; optional observation must not retain private reasoning items.
		let raw = if method == "turn/completed" {
			serde_json::json!({"threadId":thread,"turn":{"id":turn,"status":params["turn"]["status"],"error":params["turn"]["error"]}}).to_string()
		} else {
			params.to_string()
		};
		let digest: String =
			Sha256::digest(raw.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
		let item = if method == "turn/completed" {
			format!("turn-completed:{turn}")
		} else {
			params["item"]["id"]
				.as_str()
				.map(str::to_owned)
				.unwrap_or_else(|| format!("record-sha256:{digest}"))
		};
		let details = ReviewDetails {
			parent_thread_id: Some(parent.into()),
			truncated: false,
			reviewer_thread_id: thread.into(),
			reviewer_turn_id: turn.into(),
			item_id: item,
			review_id: None,
			record: raw,
		};
		// The protocol proves the private reviewer and parent, but supplies no action Review ID.
		// Keep the record unassociated rather than guessing from timing or the latest Review.
		let _ = self.store.record_review_details(Some(parent.into()), details).await;

		Ok(true)
	}
}
