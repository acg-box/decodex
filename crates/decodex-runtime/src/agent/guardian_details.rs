//! Recover optional reviewer output through supported read-only native history.
use super::{AgentCoordinator, AgentError, Value};
use decodex_core::guardian::ReviewDetails;

impl AgentCoordinator {
	pub(super) async fn observe_guardian_details(&self, method: &str, parent: &str) {
		if method == "item/autoApprovalReview/completed" {
			// Optional history recovery never authorizes or retries the action.
			let _ = tokio::time::timeout(
				std::time::Duration::from_secs(8),
				self.recover_guardian_details(parent),
			)
			.await;
		}
	}

	pub(super) async fn recover_guardian_details(&self, parent: &str) -> Result<(), AgentError> {
		if !self.store.guardian_settings().await?.review_details {
			return Ok(());
		}
		self.request_owner(parent).await?;
		let read = self
			.client
			.thread_read(serde_json::json!({"threadId":parent,"includeTurns":false}))
			.await?;
		let Some(cwd) = read["thread"]["cwd"].as_str().filter(|_| read["thread"]["id"] == parent)
		else {
			return Ok(());
		};
		// Native resume has no raw-event subscription. The list index also omits
		// Guardian parents, so verify each candidate through its native metadata.
		let page = self
			.client
			.request(
				"thread/list",
				serde_json::json!({
					"sourceKinds":["subAgentOther"],"cwd":cwd,"modelProviders":[],
					"sortKey":"updated_at","sortDirection":"desc","limit":32
				}),
			)
			.await?;
		let Some(candidates) = page["data"].as_array() else { return Ok(()) };
		for candidate in candidates.iter().take(32) {
			if candidate.pointer("/source/subAgent/other").and_then(Value::as_str)
				!= Some("guardian")
			{
				continue;
			}
			let Some(thread) = candidate["id"].as_str() else { continue };
			let Ok(metadata) = self
				.client
				.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false}))
				.await
			else {
				continue;
			};
			if metadata["thread"]["id"] != thread || metadata["thread"]["parentThreadId"] != parent
			{
				continue;
			}
			let Ok(history) = self.client.thread_history_page(thread, None, 5).await else {
				continue;
			};
			if history["thread"]["id"] != thread
				|| history["thread"]["parentThreadId"] != parent
				|| history.pointer("/thread/source/subAgent/other").and_then(Value::as_str)
					!= Some("guardian")
			{
				continue;
			}
			for turn in history["turns"].as_array().into_iter().flatten() {
				let Some(turn_id) = turn["id"].as_str() else { continue };
				for item in turn["items"].as_array().into_iter().flatten() {
					if item["type"] != "agentMessage" {
						continue;
					}
					let (Some(id), Some(text)) = (item["id"].as_str(), item["text"].as_str())
					else {
						continue;
					};
					// Store only exposed assistant output. Neither private reasoning nor
					// timing can establish an association with an action Review ID.
					let record = serde_json::json!({"observation":"native_history","threadId":thread,"turnId":turn_id,"item":{"id":id,"type":"agentMessage","text":text}}).to_string();
					self.store
						.record_review_details(
							Some(parent.into()),
							ReviewDetails {
								parent_thread_id: Some(parent.into()),
								truncated: false,
								reviewer_thread_id: thread.into(),
								reviewer_turn_id: turn_id.into(),
								item_id: id.into(),
								review_id: None,
								record,
							},
						)
						.await?;
				}
			}
		}
		Ok(())
	}
}
