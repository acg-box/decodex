//! Assistant entry and optional memory setup use the existing service commands.
use crate::{
	shell::agent_surface::{AgentActionDto, AgentSurface, ComposerInput, EntityId},
	ui_feedback::{self, Feedback},
	ui_theme,
};
use decodex_components::ComponentStore;
use decodex_protocol::{AssistantCommand, ProfileKind, RagflowConnection};
use gpui::{
	AnyElement, AppContext as _, Context, Entity, IntoElement as _, ParentElement as _,
	Styled as _, Task,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(crate) enum ComponentAction {
	Assistant,
	Connect,
	Save,
	Cancel,
	Disconnect,
	ToggleImport,
}
pub(crate) struct ComponentState {
	pub assistant: bool,
	pub connected: bool,
	pub editing: bool,
	pub import: bool,
	pub busy: bool,
}
#[derive(Default)]
pub(super) struct Panel {
	owner: Option<String>,
	fields: BTreeMap<&'static str, Entity<ComposerInput>>,
	setup: Option<Task<()>>,
	error: String,
}

impl AgentSurface {
	pub(crate) fn open_assistant(&mut self, cx: &mut Context<Self>) {
		if let Some(id) = self.assistant_id() {
			self.open_page(&id, cx);
			return;
		}
		if self.sending || self.uncertain {
			return;
		}
		let id = self
			.workspace
			.assistant_open
			.get_or_insert_with(|| format!("assistant-{}", super::unique_command()))
			.clone();
		self.execute(
			AgentActionDto::CreateAssistant {
				agent_id: EntityId::new(id).expect("generated Agent identity"),
			},
			None,
			cx,
		);
	}

	fn assistant_id(&self) -> Option<String> {
		self.snapshot.as_ref()?.agents.iter().find(|a| a.assistant).map(|a| a.id.clone())
	}

	pub(crate) fn assistant_component_state(&self) -> ComponentState {
		ComponentState {
			assistant: self.assistant_id().is_some(),
			connected: self
				.snapshot
				.as_ref()
				.is_some_and(|s| s.assistant.settings.ragflow.is_some()),
			editing: self.assistant_settings.owner.is_some(),
			import: self
				.snapshot
				.as_ref()
				.and_then(|s| s.assistant.settings.ragflow.as_ref())
				.is_some_and(|c| c.ingest_chats),
			busy: self.assistant_settings.setup.is_some() || self.sending || self.uncertain,
		}
	}

	pub(crate) fn assistant_component_action(
		&mut self,
		action: ComponentAction,
		cx: &mut Context<Self>,
	) {
		match action {
			ComponentAction::Assistant => self.open_assistant(cx),
			ComponentAction::Connect => self.edit_memory_connection(cx),
			ComponentAction::Save => self.connect_memory(cx),
			ComponentAction::Cancel => {
				self.assistant_settings = Panel::default();
				cx.notify();
			},
			ComponentAction::Disconnect => self.configure_memory(None, cx),
			ComponentAction::ToggleImport =>
				if let Some(mut connection) =
					self.snapshot.as_ref().and_then(|s| s.assistant.settings.ragflow.clone())
				{
					connection.ingest_chats = !connection.ingest_chats;
					self.configure_memory(Some(connection), cx);
				},
		}
	}

	pub(crate) fn assistant_settings_view(&self, _cx: &mut Context<Self>) -> AnyElement {
		let mut body = gpui::div().w_full().flex().flex_col().gap_2();
		if self.assistant_settings.owner.is_some() {
			for (key, label) in [("url", "Service address"), ("key", "API key")] {
				if let Some(field) = self.assistant_settings.fields.get(key) {
					body = body.child(label).child(field.clone());
				}
			}
			body = body.child(ui_theme::settings_description(
				"Setup creates a private Memory with your RAGFlow default models and connects it to Assistant.",
			));
		} else if let Some(connection) =
			self.snapshot.as_ref().and_then(|s| s.assistant.settings.ragflow.as_ref())
		{
			body = body
				.child(ui_theme::settings_description(format!(
					"Service address: {}",
					connection.url
				)))
				.child(ui_theme::settings_description(format!(
					"Import completed chats ({})",
					if connection.ingest_chats { "on" } else { "off" }
				)));
		} else {
			body = body.child(ui_theme::settings_description(
				"Connect a RAGFlow service to add long-term memory to Assistant.",
			));
		}
		body=body.child(ui_theme::settings_description("RAGFlow Memory disables Codex memory for Assistant at the next safe context refresh. Chat import is optional."));
		if self.assistant_settings.setup.is_some() {
			body = body.child(
				ui_feedback::notice(
					"ragflow-setup-progress",
					"ragflow",
					Some(Feedback::neutral("Configuring Memory…")),
				)
				.inline(),
			);
		}
		if !self.assistant_settings.error.is_empty() {
			body = body.child(
				ui_feedback::notice(
					"ragflow-setup-error",
					"ragflow",
					Some(Feedback::warning(&self.assistant_settings.error)),
				)
				.inline(),
			);
		}
		if let Some(status) =
			self.snapshot.as_ref().and_then(|s| s.assistant.memory_status.as_ref())
		{
			body = body.child(
				ui_feedback::notice(
					"ragflow-memory-status",
					"ragflow",
					Some(Feedback::neutral(status)),
				)
				.inline(),
			);
		}
		body.into_any_element()
	}

	pub(super) fn continue_memory_setup(&mut self, cx: &mut Context<Self>) {
		if self.workspace.assistant_connect && self.assistant_id().is_some() {
			self.workspace.assistant_connect = false;
			self.edit_memory_connection(cx);
		}
	}

	fn edit_memory_connection(&mut self, cx: &mut Context<Self>) {
		let Some(owner) = self.assistant_id() else {
			self.workspace.assistant_connect = true;
			self.open_assistant(cx);
			return;
		};
		let url = self
			.snapshot
			.as_ref()
			.and_then(|s| s.assistant.settings.ragflow.as_ref())
			.map(|c| c.url.as_str())
			.unwrap_or("http://localhost:9380");
		let mut fields = BTreeMap::new();
		for (key, value, label) in
			[("url", url, "RAGFlow service address"), ("key", "", "RAGFlow API key")]
		{
			let input = cx.new(|cx| ComposerInput::with_placeholder(40, "", label, cx));
			input.update(cx, |i, cx| {
				if key == "key" {
					i.obscure();
				}
				i.set_content(value, cx);
			});
			fields.insert(key, input);
		}
		self.assistant_settings = Panel { owner: Some(owner), fields, ..Default::default() };
		cx.notify();
	}

	fn connect_memory(&mut self, cx: &mut Context<Self>) {
		if self.assistant_component_state().busy {
			return;
		}
		let Some(owner) = self
			.assistant_settings
			.owner
			.clone()
			.filter(|o| self.assistant_id().as_ref() == Some(o))
		else {
			return;
		};
		let Some(profile) = self.profile.clone() else {
			return;
		};
		if profile.kind() != ProfileKind::Local {
			self.assistant_settings.error =
				"Configure this component in the App on the service host.".into();
			cx.notify();
			return;
		}
		let read = |key| {
			self.assistant_settings
				.fields
				.get(key)
				.map(|f| f.read(cx).content().trim().to_owned())
				.unwrap_or_default()
		};
		let url = read("url");
		let mut key = read("key");
		let previous = self.snapshot.as_ref().and_then(|s| s.assistant.settings.ragflow.clone());
		let namespace = format!("{}:{owner}", profile.expected_server_id().as_str());
		let request = cx.background_executor().spawn(async move {
			let store = ComponentStore::for_user()?;
			if key.is_empty()
				&& let Some(c) = previous.filter(|c| {
					decodex_components::ragflow::origin(&c.url).ok()
						== decodex_components::ragflow::origin(&url).ok()
				})
				&& let Some(id) = c.credential_id
			{
				key = store.ragflow_key(&id, &url)?;
			}
			store.setup_ragflow(&url, &key, &namespace)
		});
		self.assistant_settings.error.clear();
		self.assistant_settings.setup = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.profile.as_ref() != Some(&profile)
					|| s.assistant_id().as_ref() != Some(&owner)
					|| s.assistant_settings.owner.as_ref() != Some(&owner)
				{
					return;
				}
				s.assistant_settings.setup = None;
				match result {
					Ok(memory) if !s.sending && !s.uncertain => s.configure_memory(
						Some(RagflowConnection {
							url: memory.url,
							api_key_env: String::new(),
							credential_id: Some(memory.credential_id),
							user_id: memory.user_id,
							dataset_ids: Vec::new(),
							memory_id: Some(memory.memory_id),
							ingest_chats: false,
						}),
						cx,
					),
					Ok(_) =>
						s.assistant_settings.error =
							"Memory is ready. Retry Connect after the current action finishes."
								.into(),
					Err(error) => s.assistant_settings.error = error,
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn configure_memory(&mut self, connection: Option<RagflowConnection>, cx: &mut Context<Self>) {
		let Some(owner) = self.assistant_id() else {
			return;
		};
		let Some(mut settings) = self.snapshot.as_ref().map(|s| s.assistant.settings.clone())
		else {
			return;
		};
		settings.ragflow = connection;
		self.execute(
			AgentActionDto::ManageAssistant {
				agent_id: EntityId::new(owner).expect("saved Agent"),
				command: AssistantCommand::Configure { settings },
			},
			None,
			cx,
		);
		self.assistant_settings = Panel::default();
		cx.notify();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn memory_setup_edit_is_scoped_and_preserves_the_chat_draft(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let snapshot = s.snapshot.as_mut().unwrap();
			snapshot.agents.push(
				serde_json::from_value(
					serde_json::json!({"id":"agent","name":"Assistant","native_thread_id":null,"assistant":true}),
				)
				.unwrap(),
			);
			snapshot.assistant.agent_id = Some("agent".into());
			s.composer.update(cx, |i, cx| i.set_content("Keep the chat draft", cx));
			s.edit_memory_connection(cx);
			assert!(s.assistant_settings.fields.contains_key("url"));
			assert!(s.assistant_settings.fields.contains_key("key"));
			assert_eq!(s.assistant_settings.fields.len(), 2);
			let mut next = s.snapshot.clone().unwrap();
			next.runtime_source = Some(EntityId::new("another-process").unwrap());
			s.invalidate_snapshot_panels(&next);
			assert!(s.assistant_settings.owner.is_none());
			assert_eq!(s.composer.read(cx).content(), "Keep the chat draft");
		});
	}
}
