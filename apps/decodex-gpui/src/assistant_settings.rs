//! Assistant is built in. RAGFlow uses the same install lifecycle as other components.
use crate::{
	composer_input::ComposerEvent,
	shell::agent_surface::{AgentActionDto, AgentSurface, ComposerInput, EntityId},
	ui_feedback::Feedback,
	ui_theme,
};
use decodex_components::{
	ComponentStore,
	ragflow::{Action, Installation, MemorySetup, ModelEndpoint, Models},
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

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ComponentAction {
	Install,
	Update,
	Uninstall,
	ToggleImport,
	ConfigureModels,
	SaveModels,
	CancelModels,
	RequestReset,
	CancelReset,
	ResetMemory,
}
pub(crate) struct ComponentState {
	pub inspected: bool,
	pub running: Option<bool>,
	pub working: bool,
	pub installed: bool,
	pub version: Option<String>,
	pub feedback: Option<Feedback>,
	pub memory_ready: bool,
	pub has_models: bool,
	pub reset_confirming: bool,
	pub editing: bool,
	pub import: bool,
	pub busy: bool,
}
#[derive(Default)]
pub(super) struct Panel {
	setup: Option<Task<()>>,
	status_check: Option<Task<()>>,
	running: Option<bool>,
	fields: BTreeMap<&'static str, Entity<ComposerInput>>,
	installation: Option<Installation>,
	inspected: bool,
	progress: Arc<Mutex<String>>,
	error: String,
	completion: Option<String>,
	reset_target: Option<MemorySetup>,
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

	pub(super) fn assistant_id(&self) -> Option<String> {
		self.snapshot.as_ref()?.agents.iter().find(|a| a.assistant).map(|a| a.id.clone())
	}

	pub(super) fn is_assistant_conversation(&self, id: &str) -> bool {
		self.snapshot.as_ref().is_some_and(|s| {
			s.assistant.agent_id.as_deref() == Some(id)
				|| s.assistant.topic_ids.iter().any(|topic| topic == id)
		})
	}

	pub(super) fn new_assistant_conversation(&mut self, cx: &mut Context<Self>) {
		let Some(assistant) = self.assistant_id() else {
			self.open_assistant(cx);
			return;
		};
		if self.sending || self.uncertain {
			return;
		}
		let id = self
			.workspace
			.assistant_open
			.get_or_insert_with(|| format!("assistant-topic-{}", super::unique_command()))
			.clone();
		self.execute(
			AgentActionDto::CreateAssistantTopic {
				assistant_id: EntityId::new(assistant).expect("saved Assistant identity"),
				topic_id: EntityId::new(id).expect("generated topic identity"),
			},
			None,
			cx,
		);
	}

	pub(crate) fn ragflow_component_state(&self) -> ComponentState {
		let progress = self.assistant_settings.progress.lock().expect("component progress").clone();
		let memory = self.assistant_settings.installation.as_ref().and_then(|i| i.memory.as_ref());
		let connection = self.snapshot.as_ref().and_then(|s| s.assistant.settings.ragflow.as_ref());
		let memory_ready = memory.zip(connection).is_some_and(|(memory, connection)| {
			connection.url == memory.url
				&& connection.credential_id == memory.credential_id
				&& connection.user_id == memory.user_id
				&& connection.memory_id.as_deref() == Some(memory.memory_id.as_str())
		});
		let feedback = if !self.assistant_settings.error.is_empty() {
			Some(Feedback::error(self.assistant_settings.error.clone()))
		} else if self.assistant_settings.setup.is_some() && !progress.is_empty() {
			Some(Feedback::loading(progress))
		} else if let Some(message) = &self.assistant_settings.completion {
			Some(Feedback::success(message.clone()))
		} else if self
			.assistant_settings
			.installation
			.as_ref()
			.is_some_and(|i| i.installed && i.memory.is_none())
		{
			if self.assistant_settings.installation.as_ref().is_some_and(|i| i.models.is_some()) {
				Some(Feedback::warning(
					"Memory setup did not finish. Save the model settings to retry.",
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
			inspected: self.assistant_settings.inspected,
			running: self.assistant_settings.running,
			working: self.assistant_settings.setup.is_some(),
			editing: !self.assistant_settings.fields.is_empty(),
			feedback,
			memory_ready,
			has_models: self
				.assistant_settings
				.installation
				.as_ref()
				.is_some_and(|i| i.models.is_some()),
			reset_confirming: memory_ready
				&& self
					.assistant_settings
					.reset_target
					.as_ref()
					.zip(memory)
					.is_some_and(|(a, b)| a == b),
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
			ComponentAction::RequestReset => {
				if self.ragflow_component_state().memory_ready
					&& self.assistant_settings.fields.is_empty()
				{
					self.assistant_settings.reset_target = self
						.assistant_settings
						.installation
						.as_ref()
						.and_then(|i| i.memory.clone());
				}
				cx.notify();
				return;
			},
			ComponentAction::CancelReset => {
				self.assistant_settings.reset_target = None;
				cx.notify();
				return;
			},
			ComponentAction::ResetMemory => {
				self.reset_memory(cx);
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
		if self.assistant_settings.running == Some(false) {
			body = body.child(ui_theme::settings_description(
				"Start the container environment, then reopen Components to check again.",
			));
		}
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
					"Both models use the same OpenAI-compatible API connection.",
				))
				.child(fields([("url", "Base URL"), ("key", "API key (optional)")]))
				.child(fields([("chat", "Extraction model"), ("embedding", "Embedding model")]));
			if self.assistant_settings.installation.as_ref().is_some_and(|i| i.models.is_some()) {
				body = body.child(ui_theme::settings_description(
					"Leave a key blank to keep it while its Base URL is unchanged.",
				));
			}
			body = body.child(ui_theme::settings_description(
				"Extraction saves facts. Embedding makes memories searchable.",
			));
		} else if let Some(models) =
			self.assistant_settings.installation.as_ref().and_then(|i| i.models.as_ref())
		{
			body = body.child(
				ui_theme::settings_rows()
					.debug_selector(|| "ragflow-models-summary".into())
					.children(
						[
							("Base URL", models.extraction.base_url.as_str()),
							("Extraction model", models.extraction.model.as_str()),
							("Embedding model", models.embedding.model.as_str()),
							(
								"API key",
								if models.extraction.api_key.is_empty() {
									"Not set"
								} else {
									"Saved"
								},
							),
						]
						.into_iter()
						.map(|(label, value)| {
							ui_theme::settings_row()
								.min_h(gpui::px(26.))
								.py(gpui::px(3.))
								.child(
									ui_theme::settings_description(label)
										.w(gpui::px(120.))
										.flex_none(),
								)
								.child(
									gpui::div()
										.flex_1()
										.min_w_0()
										.text_ellipsis()
										.font_weight(gpui::FontWeight::NORMAL)
										.child(value.to_owned()),
								)
						}),
					),
			);
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
						"Assistant Memory automatically learns from completed Assistant conversations and recalls relevant personal memories. Codex memory is disabled only for this Assistant."
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
				"url":"http://127.0.0.1:9380", "version":"fixture", "image":"fixture",
				"memory":null, "models":null, "project":"fixture", "password":"fixture", "installed":true
			}))
			.expect("RAGFlow fixture"),
		);
		self.assistant_settings.inspected = true;
		self.assistant_settings.running = Some(true);
		self.edit_memory_models(cx);
	}

	#[cfg(any(test, feature = "visual-capture"))]
	pub(crate) fn visual_ragflow_saved(&mut self, cx: &mut Context<Self>) {
		self.visual_ragflow_settings(cx);
		let memory = MemorySetup {
			url: "http://127.0.0.1:9380/".into(),
			credential_id: "a".repeat(64),
			user_id: "decodex-fixture".into(),
			memory_id: "memory-fixture".into(),
		};
		let endpoint = ModelEndpoint {
			base_url: "http://127.0.0.1:4000/v1".into(),
			api_key: "private-fixture".into(),
			model: "gpt-6.1-terra".into(),
		};
		let installed =
			self.assistant_settings.installation.as_mut().expect("fixture installation");
		installed.version = "v1.0.0-rc3-51-g543f57ea9".into();
		installed.models = Some(Models {
			extraction: endpoint.clone(),
			embedding: ModelEndpoint { model: "Qwen3-Embedding-8B".into(), ..endpoint },
		});
		installed.memory = Some(memory.clone());
		self.snapshot.as_mut().expect("fixture state").assistant.settings.ragflow =
			Some(memory_connection(memory));
		self.assistant_settings.fields.clear();
	}

	fn edit_memory_models(&mut self, cx: &mut Context<Self>) {
		self.assistant_settings.reset_target = None;
		self.assistant_settings.completion = None;
		let models = self
			.assistant_settings
			.installation
			.as_ref()
			.and_then(|i| i.models.clone())
			.unwrap_or_default();
		for (key, value, label, placeholder) in [
			("chat", models.extraction.model, "Extraction model", "Model ID"),
			("url", models.extraction.base_url, "Base URL", "https://api.example.com/v1"),
			("key", String::new(), "API key (optional)", ""),
			("embedding", models.embedding.model, "Embedding model", "Model ID"),
		] {
			let input = cx.new(|cx| ComposerInput::with_placeholder(40, placeholder, label, cx));
			input.update(cx, |i, cx| {
				if key.ends_with("key") {
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
		["chat", "url", "embedding"].into_iter().all(|key| {
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
		let mut extraction =
			ModelEndpoint { model: read("chat"), base_url: read("url"), api_key: read("key") };
		if extraction.api_key.is_empty()
			&& let Some(saved) = self
				.assistant_settings
				.installation
				.as_ref()
				.and_then(|i| i.models.as_ref())
				.map(|m| &m.extraction)
				.filter(|saved| saved.base_url == extraction.base_url)
		{
			extraction.api_key.clone_from(&saved.api_key);
		}
		let embedding = ModelEndpoint { model: read("embedding"), ..extraction.clone() };
		Models { extraction, embedding }
	}

	#[cfg(not(test))]
	pub(crate) fn refresh_ragflow_status(&mut self, cx: &mut Context<Self>) {
		if self.assistant_settings.setup.is_some()
			|| self.assistant_settings.status_check.is_some()
			|| self.profile.as_ref().is_none_or(|p| p.kind() != ProfileKind::Local)
		{
			return;
		}
		let Some(installation) =
			self.assistant_settings.installation.clone().filter(|i| i.installed)
		else {
			return;
		};
		let profile = self.profile.clone();
		let request = cx.background_executor().spawn(async move { installation.is_running() });
		self.assistant_settings.status_check = Some(cx.spawn(async move |surface, cx| {
			let running = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.profile == profile {
					s.assistant_settings.status_check = None;
					s.assistant_settings.running = Some(running);
					cx.notify();
				}
			});
		}));
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
		self.finish_memory_component(request, profile, owner, ComponentAction::SaveModels, cx);
	}

	fn reset_memory(&mut self, cx: &mut Context<Self>) {
		let Some(target) = self.assistant_settings.reset_target.take() else {
			return;
		};
		if !self.ragflow_component_state().memory_ready
			|| self.assistant_settings.installation.as_ref().and_then(|i| i.memory.as_ref())
				!= Some(&target)
		{
			self.assistant_settings.error =
				"The selected Memory changed. Review its settings before resetting it.".into();
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
			"Resetting Assistant Memory…".into();
		let request = cx.background_executor().spawn(async move {
			ComponentStore::for_user()?.reset_ragflow_memory(&target, &namespace).map(Some)
		});

		self.finish_memory_component(request, profile, owner, ComponentAction::ResetMemory, cx);
	}

	pub(super) fn continue_memory_setup(&mut self, cx: &mut Context<Self>) {
		if self.assistant_settings.setup.is_some()
			|| self.profile.as_ref().is_none_or(|p| p.kind() != ProfileKind::Local)
		{
			return;
		}
		if !self.assistant_settings.inspected {
			let profile = self.profile.clone();
			let request = cx.background_executor().spawn(async {
				let installation = ComponentStore::for_user()?.ragflow_installation()?;
				let running =
					installation.as_ref().filter(|i| i.installed).map(Installation::is_running);
				Ok::<_, String>((installation, running))
			});
			self.assistant_settings.setup = Some(cx.spawn(async move |surface, cx| {
				let result = request.await;
				let _ = surface.update(cx, |s, cx| {
					if s.profile != profile {
						return;
					}
					s.assistant_settings.setup = None;
					s.assistant_settings.inspected = true;
					match result {
						Ok((installation, running)) => {
							s.assistant_settings.installation = installation;
							s.assistant_settings.running = running;
						},
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
			| ComponentAction::CancelModels
			| ComponentAction::RequestReset
			| ComponentAction::CancelReset
			| ComponentAction::ResetMemory => return,
		};
		let progress = self.assistant_settings.progress.clone();
		*progress.lock().expect("component progress") = "Preparing RAGFlow…".into();
		let request = cx.background_executor().spawn(async move {
			ComponentStore::for_user()?.apply_ragflow(operation, &namespace, |message| {
				*progress.lock().expect("component progress") = message.into()
			})
		});
		self.finish_memory_component(request, profile, owner, action, cx);
	}

	fn finish_memory_component(
		&mut self,
		request: Task<decodex_components::Result<Option<Installation>>>,
		profile: decodex_protocol::ClientProfile,
		owner: String,
		action: ComponentAction,
		cx: &mut Context<Self>,
	) {
		self.assistant_settings.error.clear();
		self.assistant_settings.completion = None;
		self.assistant_settings.status_check = None;
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
						if let Some(connection) = connection.as_mut()
							&& let Some(previous) = s
								.snapshot
								.as_ref()
								.and_then(|s| s.assistant.settings.ragflow.as_ref())
								.filter(|previous| {
									previous.url == connection.url
										&& previous.credential_id == connection.credential_id
								}) {
							connection.dataset_ids.clone_from(&previous.dataset_ids);
							connection.ingest_chats = previous.ingest_chats;
						}
						if action == ComponentAction::ResetMemory {
							s.assistant_settings.completion = Some(
								"Memory reset. Automatic memory keeps its current setting.".into(),
							);
						}
						s.assistant_settings.running = installation.as_ref().map(|i| i.installed);
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
		credential_id: memory.credential_id,
		user_id: memory.user_id,
		dataset_ids: Vec::new(),
		memory_id: Some(memory.memory_id),
		ingest_chats: true,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn ragflow_preserves_the_complete_reported_version(cx: &mut gpui::TestAppContext) {
		let agent = cx.new(AgentSurface::new);
		agent.update(cx, |s, cx| {
			s.visual_ragflow_settings(cx);
			for (stored, reported) in [
				("v1.0.0-rc3-51-g543f57ea9", "v1.0.0-rc3-51-g543f57ea9"),
				("1.0.0+vendor.build", "1.0.0+vendor.build"),
			] {
				s.assistant_settings.installation.as_mut().unwrap().version = stored.into();
				assert_eq!(s.ragflow_component_state().version.as_deref(), Some(reported));
				assert_eq!(s.assistant_settings.installation.as_ref().unwrap().version, stored);
			}
		});
	}

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
			("url", "https://chat.example.test/v1"),
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
			assert_eq!(s.assistant_settings.error, "Enter a valid extraction model Base URL.");
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
		surface.update(cx, |s, cx| {
			s.visual_ragflow_settings(cx);
			s.assistant_settings.installation.as_mut().unwrap().models = Some(Models {
				extraction: ModelEndpoint {
					base_url: "http://localhost:1234/v1".into(),
					api_key: "private-fixture".into(),
					model: "custom-chat".into(),
				},
				embedding: ModelEndpoint {
					base_url: "http://localhost:1234/v1".into(),
					api_key: "private-fixture".into(),
					model: "custom-embedding".into(),
				},
			});
			s.composer.update(cx, |i, cx| i.set_content("Keep the chat draft", cx));
			s.sending = true;
			s.ragflow_component_action(ComponentAction::ConfigureModels, cx);
			s.sending = false;
			let fields = s.assistant_settings.fields.clone();
			assert_eq!(fields.len(), 4);
			assert_eq!(fields["url"].read(cx).content(), "http://localhost:1234/v1");
			assert!(fields["key"].read(cx).content().is_empty());
			assert_eq!(fields["chat"].read(cx).content(), "custom-chat");
			assert_eq!(fields["embedding"].read(cx).content(), "custom-embedding");
			assert_eq!(s.memory_models(cx).extraction.api_key, "private-fixture");
			assert_eq!(s.memory_models(cx).embedding.api_key, "private-fixture");
			fields["url"].update(cx, |i, cx| i.set_content("https://models.example.test/v1", cx));
			assert!(s.memory_models(cx).extraction.api_key.is_empty());
			assert!(
				s.memory_models(cx).embedding.api_key.is_empty(),
				"a different shared endpoint cannot inherit the saved key"
			);
			fields["key"].update(cx, |i, cx| i.set_content("shared-fixture", cx));
			let models = s.memory_models(cx);
			assert_eq!(models.extraction.base_url, "https://models.example.test/v1");
			assert_eq!(models.extraction.base_url, models.embedding.base_url);
			assert_eq!(models.extraction.api_key, "shared-fixture");
			assert_eq!(models.extraction.api_key, models.embedding.api_key);
			let mut next = s.snapshot.clone().unwrap();
			next.runtime_source = Some(EntityId::new("another-process").unwrap());
			s.invalidate_snapshot_panels(&next);
			assert!(s.assistant_settings.fields.is_empty());
			assert_eq!(s.composer.read(cx).content(), "Keep the chat draft");
		});
	}

	#[gpui::test]
	fn existing_model_settings_use_one_connection_when_edited(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_ragflow_settings(cx);
			for embedding_url in
				["https://models.example.test/v1", "https://embedding.example.test/v1"]
			{
				let saved = Models {
					extraction: ModelEndpoint {
						base_url: "https://models.example.test/v1".into(),
						api_key: "shared-fixture".into(),
						model: "chat-model".into(),
					},
					embedding: ModelEndpoint {
						base_url: embedding_url.into(),
						api_key: "old-embedding-fixture".into(),
						model: "embedding-model".into(),
					},
				};
				s.assistant_settings.installation.as_mut().unwrap().models = Some(saved.clone());
				s.edit_memory_models(cx);
				assert_eq!(s.assistant_settings.fields.len(), 4);
				let models = s.memory_models(cx);
				assert_eq!(models.extraction.base_url, models.embedding.base_url);
				assert_eq!(models.extraction.api_key, models.embedding.api_key);
				assert_eq!(models.embedding.api_key, "shared-fixture");
				assert_eq!(models.extraction.model, saved.extraction.model);
				assert_eq!(models.embedding.model, saved.embedding.model);
				assert!(
					s.assistant_settings.installation.as_ref().unwrap().models.as_ref()
						== Some(&saved),
					"opening the editor must not change saved settings"
				);
			}
		});
	}

	#[gpui::test]
	fn reset_confirmation_is_bound_to_the_memory_that_was_reviewed(cx: &mut gpui::TestAppContext) {
		let agent = cx.new(AgentSurface::new);
		agent.update(cx, |s, cx| {
			s.visual_ragflow_saved(cx);
			let before = s.snapshot.as_ref().unwrap().assistant.settings.clone();
			s.ragflow_component_action(ComponentAction::RequestReset, cx);
			assert!(s.ragflow_component_state().reset_confirming);
			assert_eq!(s.snapshot.as_ref().unwrap().assistant.settings, before);
			s.assistant_settings
				.installation
				.as_mut()
				.unwrap()
				.memory
				.as_mut()
				.unwrap()
				.memory_id = "replacement-memory".into();
			s.snapshot.as_mut().unwrap().assistant.settings.ragflow.as_mut().unwrap().memory_id =
				Some("replacement-memory".into());
			s.ragflow_component_action(ComponentAction::ResetMemory, cx);
			assert!(
				s.assistant_settings.setup.is_none(),
				"a stale confirmation must not start a reset"
			);
			assert!(s.assistant_settings.error.contains("Memory changed"));
			assert_eq!(
				s.assistant_settings
					.installation
					.as_ref()
					.unwrap()
					.memory
					.as_ref()
					.unwrap()
					.memory_id,
				"replacement-memory"
			);
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
