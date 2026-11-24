//! Resolve the directory for the individual conversation before its first thread/start.
use std::path::{Path, PathBuf};

use decodex_core::BlobHash;
use serde_json::json;
use time::{OffsetDateTime, UtcOffset};
use tokio::fs;

use super::{AgentConfig, AgentCoordinator, AgentError, AgentWorkItem, exact};

impl AgentCoordinator {
	pub(super) async fn native_chat_directory(&self, thread: &str) -> Result<String, AgentError> {
		let read = self.client.thread_read(json!({"threadId":thread,"includeTurns":false})).await?;
		if read["thread"]["id"] != thread {
			return Err(AgentError::Invalid("The conversation identity changed.".into()));
		}
		exact(&read, "/thread/cwd")
	}

	pub(super) async fn new_chat_directory(
		&self,
		item: &AgentWorkItem,
	) -> Result<PathBuf, AgentError> {
		if let Some(directory) = self.store.work_directory(item.id.clone()).await? {
			return Ok(directory.into());
		}
		if let Some(settings) = self.store.read_agent_root_settings(&item.id).await? {
			let saved: AgentConfig = serde_json::from_str(&settings)
				.map_err(|_| AgentError::Invalid("Invalid saved Agent configuration.".into()))?;
			if let Some(directory) = saved.cwd {
				return Ok(directory.into());
			}
		}

		// Codex merges user and managed desktop storage settings in config/read.
		let response = self.client.request("config/read", json!({"includeLayers":false})).await?;
		let config = response.get("config").filter(|value| value.is_object()).ok_or_else(|| {
			AgentError::Invalid("Codex chat folder settings are unavailable.".into())
		})?;
		let root = match config.pointer("/desktop/projectlessWorkspaceRoot") {
			None | Some(serde_json::Value::Null) =>
				std::env::home_dir().map(|home| home.join("Documents").join("Codex")),
			Some(value) => value.as_str().map(PathBuf::from).filter(|path| path.is_absolute()),
		}
		.ok_or_else(|| AgentError::Invalid("Codex needs an absolute chat folder.".into()))?;
		chat_folder(&root, item).await
	}
}

async fn chat_folder(root: &Path, item: &AgentWorkItem) -> Result<PathBuf, AgentError> {
	let created =
		OffsetDateTime::from_unix_timestamp_nanos(i128::from(item.created_at_micros) * 1000)
			.map_err(|_| AgentError::Invalid("Invalid chat creation time.".into()))?;
	let offset = UtcOffset::local_offset_at(created)
		.map_err(|_| AgentError::Invalid("The local date is unavailable.".into()))?;
	let day = root.join(created.to_offset(offset).date().to_string());
	// Use the immutable conversation ID so racing starts and retries resolve the same folder.
	let digest =
		BlobHash::digest(format!("{}:{}", item.created_at_micros, item.id).as_bytes()).to_hex();
	let folder = day.join(format!("chat-{}", &digest[..24]));
	for directory in [root, day.as_path(), folder.as_path()] {
		fs::create_dir_all(directory).await.map_err(|error| {
			AgentError::Invalid(format!(
				"Cannot create chat folder {} ({error}).",
				directory.display()
			))
		})?;
		let metadata = fs::symlink_metadata(directory).await.map_err(|error| {
			AgentError::Invalid(format!(
				"Cannot inspect chat folder {} ({error}).",
				directory.display()
			))
		})?;
		if !metadata.is_dir() {
			return Err(AgentError::Invalid(format!(
				"Chat folder {} must be a directory.",
				directory.display()
			)));
		}
	}
	Ok(folder)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::agent::tests::fixture_with_history;

	#[tokio::test]
	async fn new_chats_resolve_their_own_directory_on_a_shared_connection() {
		let project = tempfile::tempdir().unwrap();
		let chats = tempfile::tempdir().unwrap();
		let original = tempfile::tempdir().unwrap();
		let (agent, mut sent, _temp) = fixture_with_history(json!({
			"_chatRoot":chats.path(),
			"existing-thread":{"thread":{"id":"existing-thread","cwd":original.path(),"turns":[]}}
		}))
		.await;
		let draft = AgentCoordinator::new_conversation_work("chat-one").unwrap();
		agent.store.reserve_agent_voice_draft(draft.clone(), None, None).await.unwrap();
		let other = AgentCoordinator::new_conversation_work("chat-two").unwrap();
		agent.store.reserve_agent_voice_draft(other.clone(), None, None).await.unwrap();

		let (first, retry) =
			tokio::join!(agent.new_chat_directory(&draft), agent.new_chat_directory(&draft));
		let first = first.unwrap();
		assert_eq!(first, retry.unwrap());
		let second = agent.new_chat_directory(&other).await.unwrap();
		assert!(first.is_dir() && second.is_dir());
		assert_eq!(first.parent(), second.parent());
		assert_eq!(first.parent().and_then(Path::parent), Some(chats.path()));
		assert_ne!(first, second);
		assert!(!first.starts_with(agent.config.cwd.as_deref().unwrap()));
		while sent.try_recv().is_ok() {}

		let project_path = project.path().to_str().unwrap();
		agent
			.store
			.register_project("project".into(), "Selected".into(), project_path.into())
			.await
			.unwrap();
		let selected = AgentCoordinator::new_conversation_work("project-chat").unwrap();
		agent
			.store
			.reserve_agent_voice_draft(selected.clone(), Some("project".into()), None)
			.await
			.unwrap();
		assert_eq!(agent.new_chat_directory(&selected).await.unwrap(), project.path());
		assert!(sent.try_recv().is_err());

		let mut existing = other;
		existing.codex_thread_id = Some("existing-thread".into());
		assert_eq!(agent.work_thread_params(&draft).await.unwrap()["cwd"], json!(first));
		assert_eq!(
			agent.work_thread_params(&existing).await.unwrap()["cwd"],
			json!(original.path())
		);
	}
}
