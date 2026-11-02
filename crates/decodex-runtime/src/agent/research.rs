//! Research completion uses the ordinary durable inbox and native conversation owner.
use std::{
	future,
	path::{Path, PathBuf},
	time::Duration,
};

use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::time;

use crate::agent::{AgentCoordinator, AgentError};
use decodex_codex::app_server_client::{ClientError, NativeThreadGoalStatus};
use decodex_database::{EnqueueAgentEvent, SqliteStore};
use decodex_pro_bridge::{Config, Phase, ProBridge, events::Completions};

/// The socket carries hints only. Every reconnect reads saved IDs after subscribing.
pub(crate) struct Events {
	root: Option<PathBuf>,
	stream: Option<Completions>,
}
impl Events {
	pub(crate) fn new(root: Option<PathBuf>) -> Self {
		Self { root, stream: None }
	}

	pub(crate) async fn next(&mut self) -> Option<String> {
		loop {
			let Some(root) = &self.root else { return future::pending().await };

			if let Some(stream) = &mut self.stream {
				if let Ok(Some(id)) = stream.recv().await {
					return Some(id);
				}

				self.stream = None;
			} else {
				if let Ok(Ok(stream)) =
					time::timeout(Duration::from_secs(5), Completions::connect(root)).await
				{
					self.stream = Some(stream);

					return None;
				}

				time::sleep(Duration::from_secs(1)).await;
			}
		}
	}
}

impl AgentCoordinator {
	pub(super) async fn research_goal_held(
		&self,
		thread: Option<&str>,
	) -> Result<bool, AgentError> {
		let Some(thread) = thread else { return Ok(true) };

		match self.client.thread_goal(thread).await {
			Ok(goal) => Ok(goal.is_some_and(|g| g.status != NativeThreadGoalStatus::Active)),
			Err(ClientError::Remote(e))
				if e.code == -32_601
					|| (e.code == -32_600 && e.message == "goals feature is disabled") =>
				Ok(false),
			Err(e) => Err(e.into()),
		}
	}

	pub(super) async fn observe_research_call(&self, params: &Value) -> Result<(), AgentError> {
		let Some(directory) = &self.research_directory else { return Ok(()) };
		let item = &params["item"];

		if item["type"] != "mcpToolCall"
			|| item["server"] != "decodex_research"
			|| !matches!(item["tool"].as_str(), Some("pro_research" | "pro_resume" | "pro_result"))
		{
			return Ok(());
		}

		let Some(id) = item.pointer("/arguments/id").and_then(Value::as_str).filter(|id| {
			!id.is_empty()
				&& id.len() <= 64
				&& id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
		}) else {
			return Ok(());
		};
		let owner = self.request_owner(&super::exact(params, "/threadId")?).await?;
		let Some(thread) = owner.codex_thread_id else { return Ok(()) };
		let root = directory.to_string_lossy().into_owned();

		if item["tool"] == "pro_result" {
			let returned = &item["result"]["structuredContent"];

			if item["status"] == "completed" && returned["receipt"]["id"] == id {
				let bridge =
					ProBridge::new(Config::default(), directory).map_err(AgentError::Invalid)?;

				if let Ok(answer) = bridge.read(id)
					&& returned["answer"]
						== serde_json::to_value(answer)
							.map_err(|e| AgentError::Invalid(e.to_string()))?
				{
					reconcile(&self.store, directory, Some(id.into()), false).await?;

					self.store.acknowledge_agent_research(root, id.into(), owner.id).await?;
				}
			}

			return Ok(());
		}

		self.store
			.record_agent_observation(EnqueueAgentEvent {
				source_event_id: serde_json::json!(["research_requested", root, id]).to_string(),
				work_item_id: owner.id,
				event_kind: "research_requested".into(),
				payload: serde_json::json!({"directory":root,"requestId":id,"threadId":thread})
					.to_string(),
			})
			.await?;

		reconcile(&self.store, directory, Some(id.into()), false).await
	}
}

pub(crate) fn directory() -> Option<PathBuf> {
	#[cfg(target_os = "macos")]
	return decodex_components::ComponentStore::for_user().ok().map(|s| s.research_directory());

	#[cfg(not(target_os = "macos"))]
	std::env::var_os("HOME")
		.map(|h| PathBuf::from(h).join(".local/share/decodex/components/pro-research"))
}

/// Recover exact saved IDs. Collection can observe submitted work but cannot send.
pub(crate) async fn reconcile(
	store: &SqliteStore,
	directory: &Path,
	id: Option<String>,
	recover: bool,
) -> Result<(), AgentError> {
	let root = directory.to_string_lossy().into_owned();
	let pending = store.pending_agent_research_requests(root.clone(), id).await?;

	if pending.is_empty() {
		return Ok(());
	}

	let bridge = ProBridge::new(Config::default(), directory).map_err(AgentError::Invalid)?;

	for event in pending {
		let binding: Value =
			serde_json::from_str(&event.payload).map_err(|e| AgentError::Invalid(e.to_string()))?;
		let id = super::exact(&binding, "/requestId")?;
		let Ok(receipt) = bridge.status(&id) else { continue };

		if receipt.phase != Phase::Complete {
			if receipt.last_error.is_some() {
				let key: String = Sha256::digest(
					serde_json::to_vec(&receipt).map_err(|e| AgentError::Invalid(e.to_string()))?,
				)
				.iter()
				.map(|b| format!("{b:02x}"))
				.collect();

				store.enqueue_agent_event(EnqueueAgentEvent {
                    source_event_id:serde_json::json!(["research_attention",root,id,key]).to_string(),
                    work_item_id:event.work_item_id.clone(),event_kind:"research_attention".into(),
                    payload:serde_json::json!({"directory":root,"requestId":id,"threadId":binding["threadId"],"receipt":receipt,"next":"Research needs attention. Report the saved blocker. Do not poll or resend an uncertain question."}).to_string(),
                }).await?;
			}
			if recover && matches!(receipt.phase, Phase::Sending | Phase::Submitted) {
				let (bridge, id) = (bridge.clone(), id.clone());

				tokio::spawn(async move {
					let _ = bridge.collect(&id).await;
				});
			}

			continue;
		}

		store.enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: serde_json::json!(["research_completed",root,id]).to_string(),
			work_item_id:event.work_item_id,
			event_kind:"research_completed".into(),
			payload:serde_json::json!({"requestId":id,"threadId":binding["threadId"],"receipt":receipt,"next":"Read pro_result with this ID. Assess the result before continuing or asking a follow-up."}).to_string(),
		}).await?;
	}

	Ok(())
}
