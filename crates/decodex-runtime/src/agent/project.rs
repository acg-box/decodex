//! Adopt native Project identity without adding another execution owner.
use super::{AgentCoordinator, AgentError, AgentWorkItem};
impl AgentCoordinator {
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
			return Err(AgentError::Rejected("Native Project roots changed; inspect membership before creating an execution context".into()));
		}
		self.store.bind_native_project(project.id, id.clone(), name).await?;
		Ok(Some(id))
	}
}
