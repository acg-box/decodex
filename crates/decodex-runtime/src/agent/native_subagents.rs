//! Resolve native child requests without granting local manager tool authority.

use std::{collections::HashSet, time::Duration};

use tokio::time;

use crate::agent::{
	self, AgentCoordinator, AgentError, AgentWorkItem, AppServerClient, SqliteStore,
};

impl AgentCoordinator {
	pub(super) async fn request_owner(&self, thread: &str) -> Result<AgentWorkItem, AgentError> {
		request_owner(&self.store, &self.client, thread).await
	}
}

pub(crate) async fn request_owner(
	store: &SqliteStore,
	client: &AppServerClient,
	thread: &str,
) -> Result<AgentWorkItem, AgentError> {
	let work = store.list_agent_work_items().await?;
	let resolve = async {
		let mut current = thread.to_owned();
		let mut visited = HashSet::new();

		for _ in 0..32 {
			if !visited.insert(current.clone()) {
				break;
			}

			if let Some(owner) =
				work.iter().find(|item| item.codex_thread_id.as_ref() == Some(&current))
			{
				return Ok(owner.clone());
			}

			let native = client.thread_read(serde_json::json!({"threadId":current})).await?;

			if native["thread"]["id"] != current {
				break;
			}

			let parent =
				agent::exact(&native, "/thread/source/subAgent/thread_spawn/parent_thread_id")?;
			// Forks and independently created workers do not establish native child authority.
			if native["thread"]["parentThreadId"].as_str() != Some(&parent) {
				break;
			}

			current = parent;
		}

		Err(AgentError::Invalid("request for unowned native thread".into()))
	};

	time::timeout(Duration::from_secs(10), resolve)
		.await
		.map_err(|_| AgentError::Invalid("native request ownership lookup timed out".into()))?
}
