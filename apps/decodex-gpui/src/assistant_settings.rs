//! Assistant is built in. RAGFlow uses the same install lifecycle as other components.
use crate::{
	composer_input::ComposerEvent,
	shell::agent_surface::{AgentActionDto, AgentSurface, ComposerInput, EntityId},
	ui_feedback::Feedback,
	ui_theme,
};
use decodex_components::{
	ComponentStore,
	ragflow::{Action, Installation, Models},
};
use decodex_protocol::{AssistantCommand, ProfileKind, RagflowConnection};
use gpui::{
	AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement as _,
	ParentElement as _, Styled as _, Task, prelude::FluentBuilder as _,
};
use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
};

#[derive(Clone, Copy)]
pub(crate) enum ComponentAction {
	Install,
	Update,
	Uninstall,
	ToggleImport,
	ConfigureModels,
	SaveModels,
	CancelModels,
}
pub(crate) struct ComponentState {
	pub installed: bool,
	pub version: Option<String>,
	pub feedback: Option<Feedback>,
	pub memory_ready: bool,
	pub editing: bool,
	pub import: bool,
	pub busy: bool,
}
#[derive(Default)]
pub(super) struct Panel {
	setup: Option<Task<()>>,
	fields: BTreeMap<&'static str, Entity<ComposerInput>>,
	installation: Option<Installation>,
	inspected: bool,
	progress: Arc<Mutex<String>>,
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
		let first_setup = self.workspace.assistant_open.is_none();
		let id = self
			.workspace
			.assistant_open
			.get_or_insert_with(|| format!("assistant-{}", super::unique_command()))
			.clone();
		if first_setup {
			let model = self
				.composer_model_value(cx)
				.and_then(|value| decodex_protocol::ConversationModel::new(value).ok());
			let effort = self
				.composer_effort_value(cx)
				.and_then(|value| decodex_protocol::ConversationReasoningEffort::new(value).ok());
			let permissions = crate::session_settings::Defaults::load().permissions;
			let speed = crate::session_settings::default_speed().ok();
			self.draft_profiles.execution.change(id.clone(), |choice| {
				choice.model = model;
				choice.reasoning_effort = effort;
				choice.permission_mode = Some(permissions);
				choice.service_tier = speed;
			});
		}
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

	pub(crate) fn ragflow_component_state(&self) -> ComponentState {
		let progress = self.assistant_settings.progress.lock().expect("component progress").clone();
		let feedback = if !self.assistant_settings.error.is_empty() {
			Some(Feedback::error(self.assistant_settings.error.clone()))
		} else if self.assistant_settings.setup.is_some() && !progress.is_empty() {
			Some(Feedback::loading(progress))
		} else if self
			.assistant_settings
			.installation
			.as_ref()
			.is_some_and(|i| i.installed && i.memory.is_none())
		{
			if self.assistant_settings.installation.as_ref().is_some_and(|i| i.models.is_some()) {
				Some(Feedback::warning(
					"Memory setup did not finish. Verify the model settings to retry.",
				))
			} else {
				None
			}
		} else {
			self.snapshot
				.as_ref()
				.and_then(|s| s.assistant.memory_status.as_ref())
				.map(Feedback::neutral)
		};
		ComponentState {
			editing: !self.assistant_settings.fields.is_empty(),
			feedback,
			memory_ready: self
				.snapshot
				.as_ref()
				.is_some_and(|s| s.assistant.settings.ragflow.is_some()),
			installed: self.assistant_settings.installation.as_ref().is_some_and(|i| i.installed),
			version: self
				.assistant_settings
				.installation
				.as_ref()
				.filter(|i| i.installed)
				.map(|i| i.version.clone()),
			import: self
				.snapshot
				.as_ref()
				.and_then(|s| s.assistant.settings.ragflow.as_ref())
				.is_some_and(|c| c.ingest_chats),
			busy: self.assistant_settings.setup.is_some() || self.sending || self.uncertain,
		}
	}

	pub(crate) fn ragflow_component_action(
		&mut self,
		action: ComponentAction,
		cx: &mut Context<Self>,
	) {
		if matches!(action, ComponentAction::ConfigureModels) {
			self.edit_memory_models(cx);
			return;
		}
		if self.ragflow_component_state().busy {
			return;
		}
		match action {
			ComponentAction::CancelModels => {
				self.assistant_settings.fields.clear();
				self.assistant_settings.error.clear();
				cx.notify();
				return;
			},
			ComponentAction::SaveModels => {
				self.save_memory_models(cx);
				return;
			},
			_ => {},
		}
		if matches!(action, ComponentAction::ToggleImport) {
			if let Some(mut connection) =
				self.snapshot.as_ref().and_then(|s| s.assistant.settings.ragflow.clone())
			{
				connection.ingest_chats = !connection.ingest_chats;
				self.configure_memory(Some(connection), cx);
			}
			return;
		}
		if self.profile.as_ref().is_none_or(|p| p.kind() != ProfileKind::Local) {
			self.assistant_settings.error =
				"Manage this component in the App on the service host.".into();
			cx.notify();
			return;
		}
		self.workspace.assistant_memory_action = Some(action);
		if self.assistant_id().is_none() {
			self.open_assistant(cx);
		} else {
			self.continue_memory_setup(cx);
		}
	}

	pub(crate) fn ragflow_settings_view(&self, _cx: &mut Context<Self>) -> AnyElement {
		let mut body = ui_theme::settings_rows().gap(gpui::px(12.));
		if !self.assistant_settings.fields.is_empty() {
			let fields = |items: [(&str, &'static str); 2]| {
				gpui::div()
					.w_full()
					.grid()
					.grid_cols(2)
					.gap(gpui::px(12.))
					.children(items.into_iter().filter_map(|(key, label)| {
						self.assistant_settings.fields.get(key).map(|field| {
							let selector = format!("ragflow-field-{key}");
							ui_theme::settings_field(
								label,
								gpui::div()
									.w_full()
									.h(gpui::px(ui_theme::SETTINGS_CONTROL_HEIGHT))
									.child(field.clone()),
							)
							.debug_selector(move || selector.clone())
							.into_any_element()
						})
					}))
					.into_any_element()
			};
			body = body
				.child(ui_theme::settings_description(
					"Both models use the same OpenAI-compatible API.",
				))
				.child(fields([("url", "Base URL"), ("key", "API key (optional)")]));
			if self.assistant_settings.installation.as_ref().is_some_and(|i| i.models.is_some()) {
				body = body.child(ui_theme::settings_description(
					"Leave the key blank to keep it when the Base URL is unchanged.",
				));
			}
			body = body
				.child(fields([("chat", "Extraction model"), ("embedding", "Embedding model")]))
				.child(ui_theme::settings_description(
					"Extraction saves facts. Embedding makes memories searchable.",
				));
		} else if let Some(models) =
			self.assistant_settings.installation.as_ref().and_then(|i| i.models.as_ref())
		{
			let rows = [
				("Base URL", &models.base_url),
				("Extraction model", &models.chat_model),
				("Embedding model", &models.embedding_model),
			]
			.into_iter()
			.map(|(label, value)| {
				ui_theme::settings_field(label, ui_theme::settings_description(value.clone()))
					.into_any_element()
			})
			.collect();
			body = body.child(ui_theme::settings_divided_rows(rows));
		} else if self.assistant_settings.installation.as_ref().is_none_or(|i| !i.installed) {
			body = body.child(ui_theme::settings_description("RAGFlow nightly with Infinity stores Assistant memory. A running Docker-compatible container environment is required."));
		}
		body.when(
			self.assistant_settings.installation.as_ref().is_some_and(|i| i.installed),
			|body| {
				body.child(ui_theme::settings_description(
					if self.ragflow_component_state().memory_ready {
						"Enabling Assistant Memory turns off Codex memory for this Assistant."
					} else {
						"Enabling Assistant Memory turns off Codex memory for this Assistant. Chat import starts off and can be enabled after setup."
					},
				))
			},
		)
		.into_any_element()
	}

	#[cfg(any(test, feature = "visual-capture"))]
	pub(crate) fn visual_ragflow_settings(&mut self, cx: &mut Context<Self>) {
		self.visual_workspace_fixture(cx);
		self.assistant_settings.installation = Some(
			serde_json::from_value(serde_json::json!({
				"url":"http://127.0.0.1:9380", "version":"nightly (fixture)", "image":"fixture",
				"memory":null, "models":null, "project":"fixture", "password":"fixture", "installed":true
			}))
			.expect("RAGFlow fixture"),
		);
		self.edit_memory_models(cx);
	}

	fn edit_memory_models(&mut self, cx: &mut Context<Self>) {
		let models = self
			.assistant_settings
			.installation
			.as_ref()
			.and_then(|i| i.models.clone())
			.unwrap_or_default();
		for (key, value, label, placeholder) in [
			("url", models.base_url, "Base URL", "https://api.example.com/v1"),
			("key", String::new(), "API key (optional)", ""),
			("chat", models.chat_model, "Extraction model", "Model ID"),
			("embedding", models.embedding_model, "Embedding model", "Model ID"),
		] {
			let input = cx.new(|cx| ComposerInput::with_placeholder(40, placeholder, label, cx));
			input.update(cx, |i, cx| {
				if key == "key" {
					i.obscure();
				}
				i.set_content(&value, cx);
			});
			cx.subscribe(&input, |surface, _, event: &ComposerEvent, cx| {
				if matches!(event, ComposerEvent::Changed) {
					surface.assistant_settings.error.clear();
					cx.notify();
				}
			})
			.detach();
			self.assistant_settings.fields.insert(key, input);
		}
		self.assistant_settings.error.clear();
		cx.notify();
	}

	pub(crate) fn can_submit_memory_models(&self, cx: &gpui::App) -> bool {
		["url", "chat", "embedding"].into_iter().all(|key| {
			self.assistant_settings
				.fields
				.get(key)
				.is_some_and(|field| !field.read(cx).content().trim().is_empty())
		})
	}

	fn memory_models(&self, cx: &gpui::App) -> Models {
		let read = |key| {
			self.assistant_settings
				.fields
				.get(key)
				.map(|f| f.read(cx).content().trim().to_owned())
				.unwrap_or_default()
		};
		let mut models = Models {
			base_url: read("url"),
			api_key: read("key"),
			chat_model: read("chat"),
			embedding_model: read("embedding"),
		};
		if models.api_key.is_empty()
			&& let Some(saved) = self
				.assistant_settings
				.installation
				.as_ref()
				.and_then(|i| i.models.as_ref())
				.filter(|m| m.base_url == models.base_url)
		{
			models.api_key.clone_from(&saved.api_key);
		}
		models
	}

	fn save_memory_models(&mut self, cx: &mut Context<Self>) {
		let models = self.memory_models(cx);
		if let Err(error) = models.validate() {
			self.assistant_settings.error = error;
			cx.notify();
			return;
		}
		let Some(profile) = self.profile.clone().filter(|p| p.kind() == ProfileKind::Local) else {
			return;
		};
		let Some(owner) = self.assistant_id() else {
			return;
		};
		let namespace = format!("{}:assistant", profile.expected_server_id().as_str());
		*self.assistant_settings.progress.lock().expect("component progress") =
			"Configuring Assistant Memory…".into();
		let request = cx.background_executor().spawn(async move {
			ComponentStore::for_user()?.configure_ragflow_models(models, &namespace).map(Some)
		});
		self.finish_memory_component(request, profile, owner, cx);
	}

	pub(super) fn continue_memory_setup(&mut self, cx: &mut Context<Self>) {
		if self.assistant_settings.setup.is_some()
			|| self.profile.as_ref().is_none_or(|p| p.kind() != ProfileKind::Local)
		{
			return;
		}
		if !self.assistant_settings.inspected {
			let profile = self.profile.clone();
			let request = cx
				.background_executor()
				.spawn(async { ComponentStore::for_user()?.ragflow_installation() });
			self.assistant_settings.setup = Some(cx.spawn(async move |surface, cx| {
				let result = request.await;
				let _ = surface.update(cx, |s, cx| {
					if s.profile != profile {
						return;
					}
					s.assistant_settings.setup = None;
					s.assistant_settings.inspected = true;
					match result {
						Ok(installation) => s.assistant_settings.installation = installation,
						Err(error) => s.assistant_settings.error = error,
					}
					s.continue_memory_setup(cx);
					cx.notify();
				});
			}));
			return;
		}
		let memory = self
			.assistant_settings
			.installation
			.as_ref()
			.filter(|i| i.installed)
			.and_then(|s| s.memory.clone());
		let configured =
			self.snapshot.as_ref().is_some_and(|s| s.assistant.settings.ragflow.is_some());
		if self.assistant_settings.installation.as_ref().is_some_and(|i| {
			!i.installed
				&& self
					.snapshot
					.as_ref()
					.and_then(|s| s.assistant.settings.ragflow.as_ref())
					.is_some_and(|c| c.url == i.url)
		}) && !self.sending
			&& !self.uncertain
		{
			self.configure_memory(None, cx);
			return;
		}
		if self.assistant_id().is_none() {
			if memory.is_some() || self.workspace.assistant_memory_action.is_some() {
				self.open_assistant(cx);
			}
			return;
		}
		if let Some(action) = self.workspace.assistant_memory_action.take() {
			self.run_memory_component(action, cx);
		} else if !configured
			&& !self.sending
			&& !self.uncertain
			&& let Some(memory) = memory
		{
			self.configure_memory(Some(memory_connection(memory)), cx);
		}
	}

	fn run_memory_component(&mut self, action: ComponentAction, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Some(owner) = self.assistant_id() else {
			return;
		};
		let namespace = format!("{}:assistant", profile.expected_server_id().as_str());
		let operation = match action {
			ComponentAction::Install => Action::Install,
			ComponentAction::Update => Action::Update,
			ComponentAction::Uninstall => Action::Uninstall,
			ComponentAction::ToggleImport
			| ComponentAction::ConfigureModels
			| ComponentAction::SaveModels
			| ComponentAction::CancelModels => return,
		};
		let progress = self.assistant_settings.progress.clone();
		*progress.lock().expect("component progress") = "Preparing RAGFlow…".into();
		let request = cx.background_executor().spawn(async move {
			ComponentStore::for_user()?.apply_ragflow(operation, &namespace, |message| {
				*progress.lock().expect("component progress") = message.into()
			})
		});
		self.finish_memory_component(request, profile, owner, cx);
	}

	fn finish_memory_component(
		&mut self,
		request: Task<decodex_components::Result<Option<Installation>>>,
		profile: decodex_protocol::ClientProfile,
		owner: String,
		cx: &mut Context<Self>,
	) {
		self.assistant_settings.error.clear();
		self.assistant_settings.setup = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.profile.as_ref() != Some(&profile) || s.assistant_id().as_ref() != Some(&owner)
				{
					return;
				}
				s.assistant_settings.setup = None;
				match result {
					Ok(installation) => {
						s.assistant_settings.fields.clear();
						let mut connection = installation
							.as_ref()
							.filter(|i| i.installed)
							.and_then(|i| i.memory.clone())
							.map(memory_connection);
						if let Some(connection) = connection.as_mut() {
							connection.ingest_chats = s
								.snapshot
								.as_ref()
								.and_then(|s| s.assistant.settings.ragflow.as_ref())
								.is_some_and(|c| c.ingest_chats);
						}
						s.assistant_settings.installation = installation;
						if s.assistant_settings
							.installation
							.as_ref()
							.is_some_and(|i| i.installed && i.models.is_none())
						{
							s.edit_memory_models(cx);
						}
						if !s.sending && !s.uncertain {
							s.configure_memory(connection, cx);
						}
					},
					Err(error) => {
						s.assistant_settings.error = error;
						s.assistant_settings.inspected = false;
					},
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
		cx.notify();
	}
}

fn memory_connection(memory: decodex_components::ragflow::MemorySetup) -> RagflowConnection {
	RagflowConnection {
		url: memory.url,
		api_key_env: String::new(),
		credential_id: Some(memory.credential_id),
		user_id: memory.user_id,
		dataset_ids: Vec::new(),
		memory_id: Some(memory.memory_id),
		ingest_chats: false,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn model_editor_blocks_empty_submit_and_updates_after_field_changes(
		cx: &mut gpui::TestAppContext,
	) {
		let agent = cx.new(AgentSurface::new);
		agent.update(cx, |s, cx| s.visual_ragflow_settings(cx));
		let (_, visual) = cx.add_window_view(|_, cx| {
			let mut settings = crate::settings_surface::SettingsSurface::new(
				crate::desktop_settings::DesktopSettingsController::production(),
				cx,
			);
			settings.bind_native_agent(agent.clone(), cx);
			settings.category = crate::settings_surface::SettingsCategory::Components;
			settings.visual_ragflow_settings();
			settings
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let save = visual.debug_bounds("components-ragflow-models-save").unwrap();
		visual.simulate_click(save.center(), Default::default());
		agent.read_with(visual, |s, cx| {
			assert!(!s.can_submit_memory_models(cx));
			assert!(s.assistant_settings.error.is_empty(), "empty submit must not dispatch");
		});
		let changed = std::rc::Rc::new(std::cell::Cell::new(0));
		let _observation = visual.update(|_, cx| {
			let changed = changed.clone();
			cx.observe(&agent, move |_, _| changed.set(changed.get() + 1))
		});
		let fields = agent.read_with(visual, |s, _| s.assistant_settings.fields.clone());
		for (key, value) in [
			("url", "https://models.example.test/v1"),
			("chat", "custom-extraction"),
			("embedding", "custom-embedding"),
		] {
			fields[key].update(visual, |i, cx| i.set_content(value, cx));
		}
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(changed.get() > 0, "field changes must refresh the owning form");
		assert!(agent.read_with(visual, |s, cx| s.can_submit_memory_models(cx)));
		fields["url"].update(visual, |i, cx| i.set_content("invalid-url", cx));
		visual.update(crate::ui_frame::draw_current_frame);
		let save = visual.debug_bounds("components-ragflow-models-save").unwrap();
		visual.simulate_click(save.center(), Default::default());
		agent.read_with(visual, |s, _| {
			assert_eq!(s.assistant_settings.error, "Enter a valid model Base URL.");
		});
		fields["url"].update(visual, |i, cx| i.set_content("https://models.example.test/v1", cx));
		fields["embedding"].update(visual, |i, cx| i.set_content("", cx));
		visual.update(crate::ui_frame::draw_current_frame);
		let save = visual.debug_bounds("components-ragflow-models-save").unwrap();
		visual.simulate_click(save.center(), Default::default());
		agent.read_with(visual, |s, cx| {
			assert!(!s.can_submit_memory_models(cx));
			assert!(
				s.assistant_settings.error.is_empty(),
				"clearing a required field disables submit"
			);
		});
	}

	#[gpui::test]
	fn model_configuration_masks_saved_keys_and_preserves_the_chat_draft(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s,cx| {
   s.visual_workspace_fixture(cx);
   s.assistant_settings.installation = Some(serde_json::from_value(serde_json::json!({
    "url":"http://127.0.0.1:9380","version":"nightly (fixture)","image":"fixture","memory":null,
    "models":{"base_url":"http://localhost:1234/v1","api_key":"private-fixture","chat_model":"custom-chat","embedding_model":"custom-embedding"},
    "project":"fixture","password":"private-fixture","installed":true
   })).unwrap());
   s.composer.update(cx,|i,cx|i.set_content("Keep the chat draft",cx));
   s.sending = true;
   s.ragflow_component_action(ComponentAction::ConfigureModels,cx);
   s.sending = false;
   assert_eq!(s.assistant_settings.fields.len(),4);
   assert_eq!(s.assistant_settings.fields["url"].read(cx).content(),"http://localhost:1234/v1");
   assert_eq!(s.assistant_settings.fields["key"].read(cx).content(),"");
   assert_eq!(s.assistant_settings.fields["chat"].read(cx).content(),"custom-chat");
   assert_eq!(s.memory_models(cx).api_key,"private-fixture");
   s.assistant_settings.fields["url"].clone().update(cx,|i,cx| i.set_content("https://different.example.test/v1",cx));
   assert!(s.memory_models(cx).api_key.is_empty(),"a different endpoint cannot inherit the saved key");
   let mut next = s.snapshot.clone().unwrap();
   next.runtime_source = Some(EntityId::new("another-process").unwrap());
   s.invalidate_snapshot_panels(&next);
   assert!(s.assistant_settings.fields.is_empty());
   assert_eq!(s.composer.read(cx).content(),"Keep the chat draft");
  });
	}

	#[gpui::test]
	fn first_assistant_message_uses_creation_choices_without_changing_the_previous_agent(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer_manager = Some("agent".into());
			s.selected = Some("agent".into());
			s.composer.update(cx, |i, cx| i.set_content("Keep the previous draft", cx));
			s.draft_profiles.execution.change("agent".into(), |choice| {
				choice.model =
					Some(decodex_protocol::ConversationModel::new("gpt-6.1-sol").unwrap());
				choice.reasoning_effort = Some(decodex_protocol::ConversationReasoningEffort::Max);
				choice.permission_mode = Some(decodex_protocol::PermissionMode::FullAccess);
			});
			s.open_assistant(cx);
			let id = s.workspace.assistant_open.clone().unwrap();
			let action = s.configured_send(
				EntityId::new(id).unwrap(),
				decodex_protocol::HistoryText::new("First request").unwrap(),
				Vec::new(),
			);
			let AgentActionDto::SendConfigured { execution, .. } = action else {
				panic!("configured first input")
			};
			assert_eq!(execution.model.as_ref().map(|m| m.as_str()), Some("gpt-6.1-sol"));
			assert_eq!(
				execution.reasoning_effort,
				Some(decodex_protocol::ConversationReasoningEffort::Max)
			);
			assert_eq!(
				execution.permission_mode,
				Some(crate::session_settings::Defaults::load().permissions)
			);
			assert_eq!(
				execution.selected_service_tier(),
				crate::session_settings::default_speed().ok()
			);
			assert_eq!(
				s.draft_profiles.execution.choice("agent").permission_mode,
				Some(decodex_protocol::PermissionMode::FullAccess)
			);
			assert_eq!(s.composer.read(cx).content(), "Keep the previous draft");
		});
	}
}
