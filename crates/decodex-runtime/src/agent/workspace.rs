//! Resolve the directory for the individual conversation before its first thread/start.
use std::path::{Path, PathBuf};

use decodex_codex::app_server_client::AppServerClient;
use decodex_core::BlobHash;
use decodex_database::AgentDispatchState;
use decodex_protocol::MissingChatFolder;
use serde_json::json;
use time::{OffsetDateTime, UtcOffset};
use tokio::fs;

use super::{AgentConfig, AgentCoordinator, AgentError, AgentWorkItem, exact};

/// Only a positively missing folder can be replaced. Transient access errors are distinct.
async fn folder_is_missing(cwd: &str) -> bool {
	match fs::metadata(cwd).await {
		Ok(metadata) => !metadata.is_dir(),
		Err(error) =>
			matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory),
	}
}

pub(crate) async fn missing_folder(
	client: &AppServerClient,
	thread: &str,
	inspect_unloaded: bool,
) -> Option<String> {
	// Only an open history view needs to read a dormant chat. Dispatch already hydrated it.
	match client.configured_task_permissions(thread) {
		Some((settings, _)) if !folder_is_missing(&settings.cwd).await => return None,
		None if !inspect_unloaded => return None,
		_ => {},
	}
	// A cached missing path can be stale after native repair. Confirm it against the thread.
	let read = client.thread_read(json!({"threadId":thread,"includeTurns":false})).await.ok()?;
	if read["thread"]["id"] != thread {
		return None;
	}
	let cwd = read["thread"]["cwd"].as_str()?;
	folder_is_missing(cwd).await.then(|| cwd.to_owned())
}

impl AgentCoordinator {
	/// Refuse new native execution when the retained working folder has disappeared.
	pub(super) async fn ensure_loaded_chat_folder(&self, thread: &str) -> Result<(), AgentError> {
		if missing_folder(&self.client, thread, false).await.is_some() {
			return Err(AgentError::MissingChatFolder);
		}
		Ok(())
	}

	pub(crate) async fn restore_chat_folder(
		&mut self,
		work: &str,
		missing: &MissingChatFolder,
		destination: &str,
	) -> Result<(), AgentError> {
		let thread = missing.thread_id.as_str();
		let item = self.store.get_agent_work_item(work.into()).await?;
		let changed = || AgentError::Rejected("This conversation or its folder changed.".into());
		if item.codex_thread_id.as_deref() != Some(thread)
			|| item.dispatch_state != AgentDispatchState::Idle
			|| item.active_turn_id.is_some()
		{
			return Err(changed());
		}
		let goal = fs::canonicalize(destination).await.map_err(|_| changed())?;
		if !fs::metadata(&goal).await.is_ok_and(|m| m.is_dir()) {
			return Err(changed());
		}
		let goal = goal.to_str().ok_or_else(changed)?;
		let current =
			self.client.thread_read(json!({"threadId":thread,"includeTurns":false})).await?;
		let row = &current["thread"];
		if row["id"] != thread
			|| row["cwd"] != missing.cwd.as_str()
			|| !folder_is_missing(missing.cwd.as_str()).await
		{
			return Err(changed());
		}
		match row.pointer("/status/type").and_then(|s| s.as_str()) {
			Some("notLoaded") => {
				let mut params = self.resume_observer_params(thread, true).await?;
				params["cwd"] = json!(goal);
				let response = self.client.thread_resume(params).await?;
				if response.pointer("/thread/id") != Some(&json!(thread)) || response["cwd"] != goal
				{
					return Err(changed());
				}
				self.attach_native_thread_owner(thread).await?;
				self.loaded_threads.insert(thread.to_owned());
			},
			Some("idle") => {
				let guard = self.client.thread_settings_guard(thread).ok_or_else(changed)?;
				self.client
					.request_with_history(
						"thread/settings/update",
						json!({"threadId":thread,"cwd":goal}),
						guard,
					)
					.await?;
				// Codex acknowledges queueing before it publishes the effective settings.
				tokio::time::timeout(std::time::Duration::from_secs(5), async {
					loop {
						if self.native_chat_directory(thread).await? == goal {
							return Ok::<(), AgentError>(());
						}
						tokio::time::sleep(std::time::Duration::from_millis(100)).await;
					}
				})
				.await
				.map_err(|_| {
					AgentError::Invalid("Codex did not confirm the new working folder.".into())
				})??;
			},
			_ => return Err(changed()),
		}
		Ok(())
	}

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
		// ChatGPT Desktop uses homeDirectory/Documents/Codex when this setting is absent.
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
		// Follow the configured root, which can be a user-owned folder link.
		let metadata = if directory == root {
			fs::metadata(directory).await
		} else {
			fs::symlink_metadata(directory).await
		}
		.map_err(|error| {
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
	async fn recover_only_the_missing_folder_on_an_idle_bound_thread() {
		let folder = tempfile::tempdir().unwrap();
		let old = folder.path().join("removed");
		let old = old.to_str().unwrap();
		let selected = folder.path().canonicalize().unwrap();
		let selected = selected.to_str().unwrap();
		for (status, operation) in
			[("idle", "thread/settings/update"), ("notLoaded", "thread/resume"), ("active", "")]
		{
			let thread = "saved-thread";
			let (mut agent, mut requests, _temp) = fixture_with_history(
				json!({thread:{"thread":{"id":thread,"cwd":old,"turns":[],"status":{"type":status}}}}),
			)
			.await;
			let work = AgentCoordinator::new_conversation_work("managed-chat").unwrap();
			agent.store.reserve_agent_voice_draft(work.clone(), None, None).await.unwrap();
			agent.store.bind_agent_thread(work.id.clone(), thread.into()).await.unwrap();
			let missing = MissingChatFolder {
				thread_id: decodex_protocol::EntityId::new(thread).unwrap(),
				cwd: decodex_protocol::ConversationWorkingDirectory::new(old).unwrap(),
			};
			assert_eq!(missing_folder(&agent.client, thread, true).await.as_deref(), Some(old));
			let outcome = agent.restore_chat_folder(&work.id, &missing, selected).await;
			let writes = std::iter::from_fn(|| requests.try_recv().ok())
				.filter(|r| {
					matches!(
						r["method"].as_str(),
						Some(
							"thread/settings/update"
								| "thread/resume"
								| "turn/start"
								| "thread/start"
						)
					)
				})
				.collect::<Vec<_>>();
			if status == "active" {
				assert!(outcome.is_err() && writes.is_empty());
			} else {
				outcome.unwrap();
				assert_eq!(writes.len(), 1);
				assert_eq!(writes[0]["method"], operation);
				assert_eq!(writes[0]["params"]["cwd"], selected);
				assert!(agent.restore_chat_folder(&work.id, &missing, selected).await.is_err());
			}
		}
	}

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

		let explicit = tempfile::tempdir().unwrap();
		let explicit_path = explicit.path().to_str().unwrap();
		let voice = AgentCoordinator::new_conversation_work("explicit-voice").unwrap();
		agent
			.store
			.reserve_agent_voice_draft(
				voice.clone(),
				Some("project".into()),
				Some(explicit_path.into()),
			)
			.await
			.unwrap();
		assert_eq!(agent.new_chat_directory(&voice).await.unwrap(), explicit.path());
		let text = AgentCoordinator::new_conversation_work("explicit-text").unwrap();
		agent
			.store
			.create_agent_conversation(
				text.clone(),
				decodex_database::EnqueueAgentEvent {
					source_event_id: "initial-text".into(),
					work_item_id: text.id.clone(),
					event_kind: "user_message".into(),
					payload: json!({"text":"Hello","options":{"cwd":explicit_path}}).to_string(),
				},
				Some("project".into()),
			)
			.await
			.unwrap();
		assert_eq!(agent.new_chat_directory(&text).await.unwrap(), explicit.path());
		assert!(agent.store.projects().await.unwrap()[0].agent_ids.contains(&text.id));
		std::os::unix::fs::symlink(chats.path(), explicit.path().join("chat-root-link")).unwrap();
		assert!(
			chat_folder(&explicit.path().join("chat-root-link"), &other).await.unwrap().is_dir()
		);

		let mut existing = other;
		existing.codex_thread_id = Some("existing-thread".into());
		assert_eq!(agent.work_thread_params(&draft).await.unwrap()["cwd"], json!(first));
		assert_eq!(
			agent.work_thread_params(&existing).await.unwrap()["cwd"],
			json!(original.path())
		);
	}
}
