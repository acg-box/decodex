//! Apply global intent to exact loaded native sources. Never answer pending approvals.
use super::{AgentCoordinator, AgentError, Value};
use decodex_core::guardian::GuardianSource;

impl AgentCoordinator {
	pub(crate) async fn sync_global_guardian(&self) -> Result<(), AgentError> {
		let state = self.store.guardian_state().await?;
		let desired = state.settings.reviewer.native();
		for (id, _) in self.store.persistent_agents().await? {
			let agent = self.store.get_agent_work_item(id.clone()).await?;
			let Some(thread) = agent.codex_thread_id else { continue };
			if !self.loaded_threads.contains(&thread) {
				continue;
			}
			let mut receipt = GuardianSource {
				source_id: self
					.native_generation
					.as_ref()
					.map(|g| g.as_str())
					.unwrap_or_else(|| self.client.connection_identity())
					.into(),
				agent_id: id,
				thread_id: thread.clone(),
				revision: state.settings.revision,
				outcome: "pending".into(),
				effective_reviewer: None,
				detail: String::new(),
			};
			let reserved = self.store.reserve_guardian_source(receipt.clone()).await?;
			let Some((before, guard)) = self.client.configured_task_permissions(&thread) else {
				receipt.outcome = "unsupported".into();
				receipt.detail =
					"Native effective reviewer readback is unavailable; no setting was inferred"
						.into();
				self.store.observe_guardian_source(receipt).await?;
				continue;
			};
			let previous = state.sources.iter().find(|s| {
				s.agent_id == receipt.agent_id
					&& s.thread_id == thread
					&& s.source_id == receipt.source_id
			});
			if reserved && before.approvals_reviewer != desired {
				let result = self.client.select_thread_reviewer(&thread, desired, guard).await;
				if let Err(error) = result {
					receipt.outcome = if matches!(
						error,
						super::ClientError::Remote(_)
							| super::ClientError::InvalidFrame
							| super::ClientError::StaleHistory
					) {
						"rejected"
					} else {
						"unknown"
					}
					.into();
					receipt.effective_reviewer = Some(before.approvals_reviewer);
					receipt.detail="Native publication was not confirmed. Pending approvals are unchanged; no automatic replay.".into();
					self.store.observe_guardian_source(receipt).await?;
					continue;
				}
			}
			let observed = self
				.client
				.configured_task_permissions(&thread)
				.map(|(facts, _)| facts.approvals_reviewer);
			receipt.outcome = if observed.as_deref() == Some(desired) {
				"applied"
			} else if reserved || previous.is_some_and(|p| p.outcome == "pending") {
				"pending"
			} else if previous.is_some_and(|p| p.outcome == "unknown") {
				"unknown"
			} else {
				"overridden"
			}
			.into();
			receipt.effective_reviewer = observed;
			receipt.detail=if agent.active_turn_id.is_some(){"Observed thread preference applies to subsequent turns. The active turn and already-pending human approvals keep their native owner."}else{"Observed native thread preference. Execution permissions and Full Access are separate."}.into();
			self.store.observe_guardian_source(receipt).await?;
		}
		Ok(())
	}

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
		let Ok(Ok(native)) = tokio::time::timeout(
			std::time::Duration::from_secs(8),
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
		if super::native_subagents::request_owner(&self.store, &self.client, parent).await.is_err()
		{
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
		let raw = params.to_string();
		use sha2::{Digest as _, Sha256};
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
		let details = decodex_core::guardian::ReviewDetails {
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
