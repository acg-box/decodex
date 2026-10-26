//! Adopt native Project identity without adding another execution owner.
use crate::agent::{AgentCoordinator, AgentError, AgentWorkItem, Value};
impl AgentCoordinator {
	pub(super) async fn sync_native_project(&self, item: &AgentWorkItem) -> Result<(), AgentError> {
		let Some(thread) = &item.codex_thread_id else { return Ok(()) };
		let Some(project) = self.native_project(item).await? else { return Ok(()) };
		let read = self
			.client
			.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false}))
			.await?;

		if read["thread"]["id"] != *thread {
			return Err(AgentError::ProjectPending(
				"Native Project thread identity changed".into(),
			));
		}

		match read["thread"].get("projectId") {
			Some(Value::String(id)) if id == &project => return Ok(()),
			Some(Value::Null) => {},
			_ => {
				return Err(AgentError::ProjectPending(
					"Native Project membership differs or is unavailable; inspect it before continuing".into(),
				));
			},
		}

		let applied = self.client.assign_thread_project(thread, &project).await?;

		if applied["thread"]["id"] != *thread || applied["thread"]["projectId"] != project {
			return Err(AgentError::ProjectPending(
				"Native Project assignment was not confirmed".into(),
			));
		}

		Ok(())
	}

	pub(super) async fn native_project(
		&self,
		item: &AgentWorkItem,
	) -> Result<Option<String>, AgentError> {
		let Some(project) =
			self.store.projects().await?.into_iter().find(|p| p.agent_ids.contains(&item.id))
		else {
			return Ok(None);
		};
		let native = if let Some(id) = &project.native_project_id {
			self.client.read_project(id).await?
		} else if let Some(existing) = self.client.project_for_directory(&project.directory).await?
		{
			existing
		} else {
			self.client
				.create_project(
					&project.name,
					&project.directory,
					&format!("decodex-project:{}", project.id),
				)
				.await?
		};
		let entry = &native["project"];
		let id = super::exact(entry, "/id")?;
		let name = super::exact(entry, "/name")?;

		if !entry["roots"]
			.as_array()
			.is_some_and(|roots| roots.iter().any(|root| root["path"] == project.directory))
		{
			return Err(AgentError::ProjectPending("Native Project roots changed; inspect membership before creating an execution context".into()));
		}

		self.store.bind_native_project(project.id, id.clone(), name).await?;

		Ok(Some(id))
	}
}
