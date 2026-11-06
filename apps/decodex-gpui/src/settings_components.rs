//! Optional components use their native lifecycle, separate from application updates.
use gpui::{
	AnyElement, Context, Div, FontWeight, KeyDownEvent, Role, SharedString,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	component_plugins::{self, PluginOperations},
	component_research::{self, ResearchOperations},
	component_runtime::{self, RuntimePreparation},
	settings_surface::{SettingsSurface, switch},
	shell::{agent_surface::RagflowComponentAction, workspace_symbols},
	ui_feedback::{self, Feedback, Tone},
	ui_motion::SmoothControl as _,
	ui_theme::{self, BLUE, SETTINGS_LABEL_SIZE, TEXT, TEXT_MUTED},
};
use decodex_components::{InstallProgress, PluginAction, RuntimeLock};

#[derive(Clone, Copy)]
enum Action {
	Prepare,
	ReinstallRuntime,
	Plugin(PluginAction),
	Research(component_research::Action),
	Ragflow(RagflowComponentAction),
}

pub(super) struct Components {
	details: [bool; 4],
	preparation: RuntimePreparation,
	shown: component_runtime::Snapshot,
	optional: PluginOperations,
	plugin: component_plugins::Snapshot,
	research: ResearchOperations,
	research_shown: component_research::Snapshot,
}
impl Components {
	pub(super) fn new(preparation: RuntimePreparation) -> Self {
		let shown = preparation.snapshot();

		Self {
			details: [false; 4],
			preparation,
			shown,
			optional: PluginOperations::new(decodex_components::egm_component(), None),
			plugin: component_plugins::Snapshot::default(),
			research: ResearchOperations::default(),
			research_shown: component_research::Snapshot::default(),
		}
	}

	fn prepare_runtime(&self) {
		self.preparation.prepare();
	}

	pub(super) fn poll(&mut self) -> bool {
		let next = self.preparation.snapshot();
		let plugin = self.optional.snapshot();
		let research = self.research.snapshot();
		let changed =
			next != self.shown || plugin != self.plugin || research != self.research_shown;

		self.shown = next;
		self.plugin = plugin;
		self.research_shown = research;

		changed
	}

	#[cfg(not(test))]
	pub(super) fn refresh_optional(&mut self) {
		self.research.inspect();

		if matches!(
			self.preparation.snapshot().state,
			component_runtime::PreparationState::Unchecked
				| component_runtime::PreparationState::Ready
		) {
			// Keep the last readback visible during the next entry check.
			if self.optional.snapshot().inspected {
				self.optional.start(PluginAction::Refresh);
			} else {
				self.optional.inspect();
			}
			self.poll();
		}
	}
}

impl SettingsSurface {
	pub(crate) fn poll_components(&mut self, cx: &mut Context<Self>) {
		#[cfg(not(test))]
		if self.category == super::SettingsCategory::Components {
			self.components.research.inspect();
		}

		#[cfg(not(test))]
		let preparing = matches!(
			self.components.shown.state,
			component_runtime::PreparationState::Preparing(_)
		);

		if self.components.poll() {
			#[cfg(not(test))]
			if preparing
				&& self.components.shown.state == component_runtime::PreparationState::Ready
			{
				self.components.optional.invalidate();

				if self.category == super::SettingsCategory::Components {
					self.components.refresh_optional();
				}
			}

			cx.notify();
		}
	}

	fn component_action(&mut self, action: Action, cx: &mut Context<Self>) {
		match action {
			Action::Prepare => self.components.prepare_runtime(),
			Action::ReinstallRuntime => self.components.preparation.reinstall(),
			Action::Plugin(action) => self.components.optional.start(action),
			Action::Research(action) => self.components.research.start(action),
			Action::Ragflow(action) => {
				match action {
					RagflowComponentAction::CancelModels => self.components.details[3] = false,
					RagflowComponentAction::ToggleImport => {},
					_ => self.components.details[3] = true,
				}
				if let Some(agent) = &self.native_agent {
					agent.update(cx, |s, cx| s.ragflow_component_action(action, cx));
				}
			},
		}

		self.components.poll();
		cx.notify();
	}

	fn component_button(
		&self,
		id: &'static str,
		label: &'static str,
		action: Action,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut disabled_reason = "Operation in progress";
		let enabled = match action {
			Action::Prepare | Action::ReinstallRuntime => !matches!(
				self.components.shown.state,
				component_runtime::PreparationState::Preparing(_)
			),
			Action::Plugin(_) => self.components.plugin.busy.is_none(),
			Action::Research(_) => self.components.research_shown.busy.is_none(),
			Action::Ragflow(action) => {
				if self.native_agent.is_none() {
					disabled_reason = "Open the workspace to manage this component.";
				}
				self.native_agent.as_ref().is_some_and(|s| {
					let agent = s.read(cx);
					if agent.ragflow_component_state().busy {
						return false;
					}
					if matches!(action, RagflowComponentAction::SaveModels)
						&& !agent.can_submit_memory_models(cx)
					{
						disabled_reason = "Enter a Base URL and model ID for each model.";
						return false;
					}
					true
				})
			},
		};
		let button = if enabled {
			if matches!(
				action,
				Action::Prepare
					| Action::Plugin(PluginAction::Install)
					| Action::Research(
						component_research::Action::Install
							| component_research::Action::EnableTools
					)
					| Action::Ragflow(
						RagflowComponentAction::Install | RagflowComponentAction::SaveModels
					)
			) || matches!(action, Action::Research(component_research::Action::Connect))
				&& self.components.research_shown.connection.is_none()
			{
				ui_theme::settings_primary_button(id, label)
			} else if matches!(
				action,
				Action::Plugin(PluginAction::Remove)
					| Action::Research(component_research::Action::Remove)
					| Action::Ragflow(RagflowComponentAction::Uninstall)
			) {
				ui_theme::settings_destructive_button(id, label)
			} else {
				ui_theme::settings_button(id, label)
			}
		} else {
			ui_theme::settings_button_base(id, label).cursor_default().opacity(0.5)
		};

		button
			.tab_index(if enabled { 0 } else { -1 })
			.when(!enabled, |button| button.aria_description(disabled_reason))
			.on_click(cx.listener(move |this, _, _, cx| {
				if enabled {
					this.component_action(action, cx);
				}
			}))
			.on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
				if enabled
					&& !event.is_held
					&& ["enter", "space"].contains(&event.keystroke.key.as_str())
				{
					this.component_action(action, cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn component_lifecycle_buttons(
		&self,
		installed: Option<bool>,
		actions: [(&'static str, Action); 3],
		reinstall: Option<(&'static str, Action)>,
		cx: &mut Context<Self>,
	) -> Div {
		let primary = usize::from(installed == Some(true));
		let label = match installed {
			None => "Install",
			Some(false) => "Enable",
			Some(true) => "Update",
		};
		let mut controls = gpui::div().flex().items_center().gap_2().child(self.component_button(
			actions[primary].0,
			label,
			actions[primary].1,
			cx,
		));
		if installed.is_some() {
			if let Some((id, action)) = reinstall {
				controls = controls.child(self.component_button(id, "Reinstall", action, cx));
			}
			controls =
				controls.child(self.component_button(actions[2].0, "Uninstall", actions[2].1, cx));
		}
		controls
	}

	fn component_settings(&self, cx: &mut Context<Self>) -> Div {
		let lock = RuntimeLock::embedded().ok();
		let version = lock.as_ref().map(|lock| lock.version.as_str());
		let mut component = gpui::div()
			.debug_selector(|| "components-codex".into())
			.w_full()
			.flex()
			.flex_col()
			.child(
				ui_theme::settings_row()
					.child(self.component_label(
						0,
						"components-codex-details",
						"Codex",
						version,
						cx,
					))
					.when_some(self.components.shown.action, |row, action| {
						let (id, label, action) = match action {
							component_runtime::Action::Install =>
								("components-install-runtime", "Install", Action::Prepare),
							component_runtime::Action::Reinstall => (
								"components-reinstall-runtime",
								"Reinstall",
								Action::ReinstallRuntime,
							),
						};
						row.child(self.component_button(id, label, action, cx))
					}),
			);

		match &self.components.shown.state {
			component_runtime::PreparationState::Unchecked => {},
			component_runtime::PreparationState::Ready => {},
			component_runtime::PreparationState::Failed(error) => {
				let detail = if self.components.details[0] {
					error.as_str()
				} else if self.components.shown.action.is_none() {
					"The required runtime is unavailable for this build. Install a complete build of Decodex."
				} else {
					"The operation did not complete. Expand Codex for details."
				};
				component = component.children(ui_feedback::inline_feedback(
					"components-component-error",
					"Setup failed",
					&Feedback::error(detail),
				));
			},
			component_runtime::PreparationState::Preparing(progress) => {
				component = component.children(ui_feedback::inline_feedback(
					"components-component-status",
					"Preparing Codex",
					&Feedback::info(progress_text(progress)),
				));

				if let InstallProgress::Downloading { received, total: Some(total) } = progress
					&& *total > 0
				{
					component = component.child(
						gpui::div()
							.h(gpui::px(4.))
							.rounded_full()
							.bg(ui_theme::rgba(0xffffff0c))
							.child(
								gpui::div()
									.h_full()
									.w(gpui::relative(
										(*received as f32 / *total as f32).clamp(0., 1.),
									))
									.rounded_full()
									.bg(ui_theme::rgb(BLUE)),
							),
					);
				}
			},
		}

		let details = gpui::div().flex().flex_col().gap_2().child(ui_theme::settings_description(
			"Codex is prepared at startup and updates with Decodex.",
		));
		component.child(self.component_body(0, "components-codex-body", details))
	}

	fn toggle_component_details(&mut self, index: usize, cx: &mut Context<Self>) {
		self.components.details[index] = !self.components.details[index];
		if index == 3
			&& self.components.details[index]
			&& self.native_agent.as_ref().is_some_and(|agent| {
				let state = agent.read(cx).ragflow_component_state();
				state.installed && !state.editing
			}) {
			self.component_action(Action::Ragflow(RagflowComponentAction::ConfigureModels), cx);
		}
		cx.notify();
	}

	fn component_label(
		&self,
		index: usize,
		id: &'static str,
		title: &'static str,
		version: Option<&str>,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let expanded = self.components.details[index];
		let metadata = version.map(|version| SharedString::from(format!("Version {version}")));
		let (tone, status) = self.component_status(index, cx);
		gpui::div()
			.id(id)
			.debug_selector(move || id.into())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(title)
			.aria_description(match &metadata {
				Some(version) => format!("{status}. {version}"),
				None => status.to_owned(),
			})
			.aria_expanded(expanded)
			.flex_1()
			.min_w_0()
			.flex()
			.items_center()
			.min_h(gpui::px(24.))
			.gap(gpui::px(12.))
			.rounded(gpui::px(4.))
			.cursor_pointer()
			.text_size(gpui::px(SETTINGS_LABEL_SIZE))
			.line_height(gpui::px(18.))
			.text_color(ui_theme::rgb(TEXT))
			.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.active(|s| s.opacity(0.75))
			.on_click(cx.listener(move |this, _, _, cx| {
				this.toggle_component_details(index, cx);
			}))
			.on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					this.toggle_component_details(index, cx);
					cx.stop_propagation();
				}
			}))
			.child(
				gpui::div()
					.flex_none()
					.flex()
					.items_center()
					.gap_1()
					.font_weight(FontWeight::MEDIUM)
					.child(ui_feedback::status_indicator(format!("{id}-status"), status, tone))
					.child(title)
					.child(gpui::div().text_color(ui_theme::rgb(TEXT_MUTED)).child(
						workspace_symbols::process_chevron(
							SharedString::from(format!("{id}-chevron")),
							expanded,
						),
					)),
			)
			.when_some(metadata, |label, version| {
				label.child(ui_theme::settings_description(version))
			})
			.smooth()
			.into_any_element()
	}

	fn component_status(&self, index: usize, cx: &gpui::App) -> (Tone, &'static str) {
		match index {
			0 => match self.components.shown.state {
				component_runtime::PreparationState::Unchecked => (Tone::Neutral, "Not checked"),
				component_runtime::PreparationState::Preparing(_) => (Tone::Info, "Preparing"),
				component_runtime::PreparationState::Ready => (Tone::Success, "Ready"),
				component_runtime::PreparationState::Failed(_) => (Tone::Error, "Setup failed"),
			},
			1 => {
				let state = &self.components.plugin;
				if state.busy.is_some_and(|a| a != PluginAction::Refresh) {
					(Tone::Info, "Operation in progress")
				} else if state.error.is_some() {
					(Tone::Error, "Check failed")
				} else if !state.inspected {
					(Tone::Neutral, "Not checked")
				} else {
					match &state.installed {
						Some(plugin) if plugin.enabled => (Tone::Success, "Enabled"),
						Some(_) => (Tone::Warning, "Disabled"),
						None => (Tone::Neutral, "Not installed"),
					}
				}
			},
			2 => {
				let state = &self.components.research_shown;
				if state.busy.is_some() {
					(Tone::Info, "Operation in progress")
				} else if state.error.is_some() {
					(Tone::Error, "Check failed")
				} else if !state.inspected {
					(Tone::Neutral, "Not checked")
				} else if state.installed.is_none() {
					(Tone::Neutral, "Not installed")
				} else if state.tools.as_ref().is_some_and(|p| p.enabled)
					&& state
						.connection
						.as_ref()
						.is_some_and(|c| c.state == decodex_pro_bridge::ConnectionState::Connected)
				{
					(Tone::Success, "Connected and tools enabled")
				} else {
					(Tone::Warning, "Setup required")
				}
			},
			_ => {
				let Some(agent) = &self.native_agent else {
					return (Tone::Neutral, "Open the workspace to check this component");
				};
				let state = agent.read(cx).ragflow_component_state();
				if state.working {
					(Tone::Info, "Operation in progress")
				} else if state.feedback.is_some_and(|f| f.tone == Tone::Error) {
					(Tone::Error, "Operation failed")
				} else if !state.inspected {
					(Tone::Neutral, "Not checked")
				} else if !state.installed {
					(Tone::Neutral, "Not installed")
				} else if state.running == Some(false) {
					(Tone::Error, "Service unavailable")
				} else if !state.memory_ready {
					(Tone::Warning, "Memory setup required")
				} else if state.running == Some(true) {
					(Tone::Success, "Service running and Memory configured")
				} else {
					(Tone::Neutral, "Service not checked")
				}
			},
		}
	}

	fn component_body(
		&self,
		index: usize,
		id: &'static str,
		content: impl gpui::IntoElement,
	) -> AnyElement {
		crate::ui_motion::settings_disclosure(id, self.components.details[index], || {
			ui_theme::settings_detail(content).into_any_element()
		})
		.into_any_element()
	}

	pub(super) fn components_content(&self, cx: &mut Context<Self>) -> AnyElement {
		ui_theme::settings_panel(ui_theme::settings_divided_rows(vec![
			self.component_settings(cx).into_any_element(),
			self.egm_settings(cx).into_any_element(),
			self.research_settings(cx).into_any_element(),
			self.ragflow_settings(cx),
		]))
		.into_any_element()
	}

	#[cfg(any(test, feature = "visual-capture"))]
	pub(crate) fn visual_managed_components(&mut self) {
		self.components.preparation =
			RuntimePreparation::fixture(component_runtime::PreparationState::Ready);
		self.components.shown = self.components.preparation.snapshot();
		self.components.plugin = component_plugins::Snapshot {
			inspected: true,
			installed: Some(decodex_components::PluginInstallation {
				version: "0.1.0".into(),
				enabled: true,
			}),
			..Default::default()
		};
		self.components.research_shown = component_research::Snapshot {
			inspected: true,
			installed: Some(decodex_pro_bridge::install::Installation {
				version: "0.1.0".into(),
				extension_id: "fixture",
				extension_directory: Default::default(),
				plugin_directory: Default::default(),
				marketplace_directory: Default::default(),
				registration: Default::default(),
			}),
			..Default::default()
		};
	}

	#[cfg(any(test, feature = "visual-capture"))]
	pub(crate) fn visual_ragflow_settings(&mut self) {
		self.components.details[3] = true;
	}

	fn ragflow_settings(&self, cx: &mut Context<Self>) -> AnyElement {
		let state = self.native_agent.as_ref().map(|s| s.read(cx).ragflow_component_state());
		let installed = state.as_ref().is_some_and(|s| s.installed);
		let mut row = ui_theme::settings_row().child(self.component_label(
			3,
			"components-ragflow-details",
			"RAGFlow",
			state.as_ref().and_then(|s| s.version.as_deref()),
			cx,
		));
		row = row.child(self.component_lifecycle_buttons(
			installed.then_some(true),
			[
				("components-ragflow-install", Action::Ragflow(RagflowComponentAction::Install)),
				("components-ragflow-update", Action::Ragflow(RagflowComponentAction::Update)),
				(
					"components-ragflow-uninstall",
					Action::Ragflow(RagflowComponentAction::Uninstall),
				),
			],
			None,
			cx,
		));
		let mut body =
			ui_theme::settings_rows().gap(gpui::px(12.)).child(self.ragflow_component(cx));
		if let Some(state) = state.as_ref().filter(|s| s.memory_ready) {
			body = body.child(
				ui_theme::settings_row()
					.min_h(gpui::px(32.))
					.child(ui_theme::settings_label(
						"Import chats",
						"Add completed chat turns to Assistant Memory.",
					))
					.child(
						switch::settings_switch(
							"components-ragflow-import",
							"components-ragflow-import-knob",
							"Import chats into Assistant Memory",
							state.import,
							!state.busy,
						)
						.on_change(cx.listener(|s, _, _, cx| {
							s.component_action(
								Action::Ragflow(RagflowComponentAction::ToggleImport),
								cx,
							);
						})),
					),
			);
		}
		if state.as_ref().is_some_and(|s| s.editing) {
			let mut actions = gpui::div().w_full().flex().gap_2().justify_end().pt_1();
			for (id, label, action) in [
				(
					"components-ragflow-models-cancel",
					"Cancel",
					RagflowComponentAction::CancelModels,
				),
				("components-ragflow-models-save", "Save", RagflowComponentAction::SaveModels),
			] {
				actions =
					actions.child(self.component_button(id, label, Action::Ragflow(action), cx));
			}
			body = body.child(actions);
		}
		gpui::div()
			.child(row)
			.child(self.component_body(3, "components-ragflow-body", body))
			.child(
				ui_feedback::notice(
					"components-ragflow-feedback",
					"ragflow",
					state.and_then(|s| s.feedback),
				)
				.inline(),
			)
			.into_any_element()
	}

	fn research_controls(&self, cx: &mut Context<Self>) -> Div {
		let state = &self.components.research_shown;
		let mut controls = gpui::div().flex().items_center().gap_2();
		if state.inspected && state.installed.is_none() && state.error.is_some() {
			controls = controls.child(self.component_button(
				"components-reinstall-research",
				"Reinstall",
				Action::Research(component_research::Action::Reinstall),
				cx,
			));
		} else if state.inspected {
			if state.installed.is_some() {
				controls = controls.child(self.component_button(
					"components-connect-research",
					if state.connection.is_some() { "Open ChatGPT" } else { "Connect" },
					Action::Research(component_research::Action::Connect),
					cx,
				));
			}
			controls = controls.child(self.component_lifecycle_buttons(
				state.installed.as_ref().map(|_| true),
				[
					(
						"components-install-research",
						Action::Research(component_research::Action::Install),
					),
					(
						"components-update-research",
						Action::Research(component_research::Action::Update),
					),
					(
						"components-remove-research",
						Action::Research(component_research::Action::Remove),
					),
				],
				Some((
					"components-reinstall-research",
					Action::Research(component_research::Action::Reinstall),
				)),
				cx,
			));
		}

		controls
	}

	fn research_settings(&self, cx: &mut Context<Self>) -> Div {
		let state = &self.components.research_shown;
		let controls = self.research_controls(cx);

		let explanation = self.research_explanation();
		let mut details = gpui::div().flex().items_center().gap_2();

		if state.installed.is_some() {
			for (id, label, action) in [
				(
					"components-research-extension",
					"Chrome extension",
					component_research::Action::OpenExtension,
				),
				(
					"components-research-folder",
					"Show folder",
					component_research::Action::RevealExtension,
				),
			] {
				details =
					details.child(self.component_button(id, label, Action::Research(action), cx));
			}

			if state.connection.is_some() {
				details = details.child(self.component_button(
					"components-disconnect-research",
					"Close setup",
					Action::Research(component_research::Action::Disconnect),
					cx,
				));
			}
			if !state.tools.as_ref().is_some_and(|p| p.enabled) {
				details = details.child(self.component_button(
					"components-research-tools",
					"Enable Agent tools",
					Action::Research(component_research::Action::EnableTools),
					cx,
				));
			}
		}

		let feedback = state.error.as_ref().map(|e| Feedback::error(e.clone())).or_else(|| {
			state.busy.and_then(|a| match a {
				component_research::Action::Inspect => None,
				component_research::Action::Install
				| component_research::Action::Update
				| component_research::Action::Reinstall =>
					Some(Feedback::loading("Preparing research component…")),
				component_research::Action::EnableTools =>
					Some(Feedback::loading("Enabling native research tools…")),
				component_research::Action::Remove =>
					Some(Feedback::loading("Uninstalling research; keeping results…")),
				component_research::Action::Connect =>
					Some(Feedback::loading("Opening ChatGPT setup…")),
				_ => None,
			})
		});

		gpui::div()
			.debug_selector(|| "components-pro-research".into())
			.w_full()
			.flex()
			.flex_col()
			.child(
				ui_theme::settings_row()
					.child(self.component_label(
						2,
						"components-research-details",
						"Decodex Pro Research",
						state.installed.as_ref().map(|package| package.version.as_str()),
						cx,
					))
					.child(controls),
			)
			.child(
				self.component_body(
					2,
					"components-research-body",
					gpui::div()
						.flex()
						.flex_col()
						.gap_2()
						.child(ui_theme::settings_description(format!(
							"Experimental. {explanation}"
						)))
						.when(state.installed.is_some(), |body| body.child(details)),
				),
			)
			.child(
				ui_feedback::notice("components-research-feedback", "research", feedback).inline(),
			)
	}

	fn research_explanation(&self) -> &'static str {
		let state = &self.components.research_shown;
		if state.installed.is_none() {
			"Use your ChatGPT Pro subscription. Installs a local helper and Chrome extension files; no API key."
		} else if let Some(connection) = state
			.connection
			.as_ref()
			.filter(|connection| connection.state != decodex_pro_bridge::ConnectionState::Connected)
		{
			match connection.state {
				decodex_pro_bridge::ConnectionState::SignInRequired =>
					"Sign in to ChatGPT in the research tab, then connect again.",
				decodex_pro_bridge::ConnectionState::VerificationRequired =>
					"Complete ChatGPT security verification in the research tab, then connect again.",
				_ => "Open the research tab to complete setup, then connect again.",
			}
		} else if state.connection.is_none() {
			"Connect to Chrome. If the extension is not loaded, enable Developer mode, choose Load unpacked, and select the extension folder."
		} else if !state.tools.as_ref().is_some_and(|p| p.enabled) {
			"Chrome is connected. Enable native research tools for Agents. Installation sends no messages."
		} else {
			"Research tools are enabled. Conversations stay in their ChatGPT Project. Every answer verifies its actual Pro model."
		}
	}

	fn egm_settings(&self, cx: &mut Context<Self>) -> Div {
		let state = &self.components.plugin;
		let mut controls = gpui::div().flex().items_center().gap_2();

		if state.inspected {
			controls = self.component_lifecycle_buttons(
				state.installed.as_ref().map(|p| p.enabled),
				[
					("components-install-egm", Action::Plugin(PluginAction::Install)),
					("components-install-egm", Action::Plugin(PluginAction::Update)),
					("components-remove-egm", Action::Plugin(PluginAction::Remove)),
				],
				Some(("components-reinstall-egm", Action::Plugin(PluginAction::Reinstall))),
				cx,
			);
		} else {
			controls = controls.child(self.component_button(
				"components-refresh-egm",
				"Check status",
				Action::Plugin(PluginAction::Refresh),
				cx,
			));
		}

		let feedback =
			state.error.as_ref().map(|error| Feedback::error(error.clone())).or_else(|| {
				state.busy.and_then(|action| {
					Some(Feedback::loading(match action {
						PluginAction::Refresh => return None,
						PluginAction::Install => "Installing EGM…",
						PluginAction::Update => "Updating EGM…",
						PluginAction::Reinstall => "Reinstalling EGM…",
						PluginAction::Remove => "Uninstalling EGM…",
					}))
				})
			});
		gpui::div().debug_selector(|| "components-egm".into()).w_full().flex().flex_col().child(ui_theme::settings_row()
			.child(self.component_label(1, "components-egm-details", "Elephant–Goldfish (EGM)", state.installed.as_ref().map(|plugin| plugin.version.as_str()), cx))
			.child(controls))
			.child(self.component_body(1,"components-egm-body",ui_theme::settings_description("Ask an Agent to use EGM to draft or review a design. Start a new conversation after installation or update.")))
			.child(ui_feedback::notice("components-egm-feedback", "egm", feedback).inline())
	}
}

fn progress_text(progress: &InstallProgress) -> String {
	match progress {
		InstallProgress::Waiting => "Checking…".into(),
		InstallProgress::Downloading { received, total } => match total {
			Some(total) if *total > 0 => format!("Downloading… {}%", received * 100 / total),
			_ => format!("Downloading… {} MB", received / 1_000_000),
		},
		InstallProgress::Verifying => "Verifying…".into(),
		InstallProgress::Ready => "Ready".into(),
	}
}

#[cfg(test)]
mod tests {
	use gpui::AppContext as _;
	use std::thread;

	use crate::{
		component_runtime,
		desktop_settings::DesktopSettingsController,
		settings_surface::components::{RuntimePreparation, SettingsSurface},
		ui_feedback::Tone,
	};
	use decodex_components::{InstallProgress, PluginAction, PluginInstallation};

	#[gpui::test]
	fn ragflow_groups_connection_and_models_with_aligned_actions(cx: &mut gpui::TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, cx| {
			let agent = cx.new(crate::shell::agent_surface::AgentSurface::new);
			agent.update(cx, |agent, cx| agent.visual_ragflow_settings(cx));
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.bind_native_agent(agent, cx);
			settings.category = crate::settings_surface::SettingsCategory::Components;
			settings.visual_ragflow_settings();
			settings
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let extraction = visual.debug_bounds("ragflow-field-chat").unwrap();
		let embedding = visual.debug_bounds("ragflow-field-embedding").unwrap();
		for [model, url, key] in [
			["ragflow-field-chat", "ragflow-field-chat_url", "ragflow-field-chat_key"],
			[
				"ragflow-field-embedding",
				"ragflow-field-embedding_url",
				"ragflow-field-embedding_key",
			],
		] {
			let model = visual.debug_bounds(model).unwrap();
			let url = visual.debug_bounds(url).unwrap();
			let key = visual.debug_bounds(key).unwrap();
			assert_eq!(model.top(), url.top());
			assert_eq!(url.top(), key.top());
			assert!(model.right() < url.left() && url.right() < key.left());
			assert_eq!(model.left(), extraction.left());
		}
		assert!(embedding.top() > extraction.bottom());
		let save = visual.debug_bounds("components-ragflow-models-save").unwrap();
		assert_eq!(
			save.right(),
			visual.debug_bounds("ragflow-field-embedding_key").unwrap().right()
		);
		assert!(save.top() > embedding.bottom());
	}

	#[gpui::test]
	fn component_lights_distinguish_availability_from_installation(cx: &mut gpui::TestAppContext) {
		cx.new(|cx| {
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			assert_eq!(settings.component_status(1, cx), (super::Tone::Neutral, "Not checked"));
			settings.components.plugin.inspected = true;
			assert_eq!(settings.component_status(1, cx), (super::Tone::Neutral, "Not installed"));
			settings.components.plugin.installed =
				Some(PluginInstallation { version: "fixture".into(), enabled: false });
			assert_eq!(settings.component_status(1, cx), (super::Tone::Warning, "Disabled"));
			settings.components.plugin.installed.as_mut().unwrap().enabled = true;
			assert_eq!(settings.component_status(1, cx), (super::Tone::Success, "Enabled"));
			settings.components.plugin.busy = Some(PluginAction::Refresh);
			assert_eq!(
				settings.component_status(1, cx),
				(super::Tone::Success, "Enabled"),
				"background reads do not flash the light"
			);
			settings.components.plugin.busy = Some(PluginAction::Update);
			assert_eq!(
				settings.component_status(1, cx),
				(super::Tone::Info, "Operation in progress")
			);
			settings.components.plugin.busy = None;
			settings.components.plugin.error = Some("fixture".into());
			assert_eq!(settings.component_status(1, cx), (super::Tone::Error, "Check failed"));
			settings.components.research_shown.inspected = true;
			settings.components.research_shown.installed =
				Some(decodex_pro_bridge::install::Installation {
					version: "fixture".into(),
					extension_id: "fixture",
					extension_directory: Default::default(),
					plugin_directory: Default::default(),
					marketplace_directory: Default::default(),
					registration: Default::default(),
				});
			assert_eq!(settings.component_status(2, cx), (super::Tone::Warning, "Setup required"));
			settings.components.research_shown.connection = Some(decodex_pro_bridge::Connection {
				state: decodex_pro_bridge::ConnectionState::Connected,
				selected_model: None,
			});
			settings.components.research_shown.tools =
				Some(PluginInstallation { version: "fixture".into(), enabled: true });
			assert_eq!(
				settings.component_status(2, cx),
				(super::Tone::Success, "Connected and tools enabled")
			);
			settings
		});
	}

	#[gpui::test]
	fn optional_component_controls_follow_native_readback_and_busy_state(
		cx: &mut gpui::TestAppContext,
	) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);

			settings.category = crate::settings_surface::SettingsCategory::Components;

			settings
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-refresh-egm").is_some());
		assert!(visual.debug_bounds("components-retry-egm").is_none());
		let cold_egm = visual.debug_bounds("components-egm").unwrap();
		let cold_research = visual.debug_bounds("components-pro-research").unwrap();
		assert!(
			visual.debug_bounds("components-install-research").is_none(),
			"an unchecked component is not an absent installation"
		);
		assert!(
			visual.debug_bounds("components-install-egm").is_none(),
			"inspect before writing native configuration"
		);

		settings.update(visual, |settings, cx| {
			settings.components.plugin.inspected = true;
			settings.components.research_shown.inspected = true;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-install-egm").is_some());
		assert!(visual.debug_bounds("components-install-research").is_some());
		assert!(visual.debug_bounds("components-remove-egm").is_none());

		settings.update(visual, |settings, cx| {
			settings.components.plugin.installed =
				Some(PluginInstallation { version: "0.1.0".into(), enabled: true });

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-remove-egm").is_some());
		assert!(visual.debug_bounds("components-reinstall-egm").is_some());
		assert_eq!(
			visual.debug_bounds("components-egm").unwrap(),
			cold_egm,
			"version readback must not shift the row"
		);
		assert_eq!(
			visual.debug_bounds("components-pro-research").unwrap(),
			cold_research,
			"other rows must not move while component metadata loads"
		);

		let install = visual.debug_bounds("components-install-egm").unwrap();
		let remove = visual.debug_bounds("components-remove-egm").unwrap();

		settings.update(visual, |settings, cx| {
			settings.components.plugin.busy = Some(PluginAction::Refresh);

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert_eq!(visual.debug_bounds("components-install-egm").unwrap(), install);
		assert_eq!(visual.debug_bounds("components-remove-egm").unwrap(), remove);
		assert_eq!(
			visual.debug_bounds("conversation-feedback-slot").unwrap().size.height,
			gpui::px(0.)
		);

		thread::sleep(std::time::Duration::from_millis(600));

		visual.update(crate::ui_frame::draw_current_frame);

		assert_eq!(
			visual.debug_bounds("conversation-feedback-slot").unwrap().size.height,
			gpui::px(0.),
			"normal native checks must not flash a loading row"
		);

		settings.update(visual, |settings, cx| {
			settings.components.plugin.busy = None;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(
			visual.debug_bounds("conversation-activity-status").is_none(),
			"a quick read must not flash a loading row"
		);

		settings.update(visual, |settings, cx| {
			settings.components.plugin.busy = Some(PluginAction::Update);

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert_eq!(visual.debug_bounds("components-install-egm").unwrap(), install);
		assert_eq!(visual.debug_bounds("components-remove-egm").unwrap(), remove);

		visual.simulate_click(install.center(), Default::default());

		assert_eq!(
			settings.read_with(visual, |s, _| s.components.optional.snapshot()),
			Default::default(),
			"busy controls must not dispatch a second operation"
		);

		thread::sleep(std::time::Duration::from_millis(300));

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(250));

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(
			visual.debug_bounds("conversation-activity-status").is_some(),
			"slow operations still show progress"
		);
		assert_eq!(
			visual.debug_bounds("conversation-activity-status").unwrap().left(),
			visual.debug_bounds("components-egm").unwrap().left(),
			"component feedback must align with the setting, without conversation insets"
		);
		assert_eq!(visual.debug_bounds("components-install-egm").unwrap(), install);
	}

	#[gpui::test]
	fn component_details_start_compact_and_toggle_independently(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut s = SettingsSurface::new(DesktopSettingsController::production(), cx);

			s.category = crate::settings_surface::SettingsCategory::Components;
			s.components.plugin.inspected = true;

			s
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("settings-group-This Mac").is_none());
		assert_eq!(settings.read_with(visual, |s, _| s.components.details), [false; 4]);

		let codex = visual.debug_bounds("components-codex").unwrap();
		let egm = visual.debug_bounds("components-egm").unwrap();
		let research = visual.debug_bounds("components-pro-research").unwrap();

		assert_eq!(codex.bottom() + gpui::px(1.), egm.top(), "one divider separates rows");
		assert_eq!(egm.bottom() + gpui::px(1.), research.top(), "list dividers stay uniform");

		let toggle = visual.debug_bounds("components-egm-details").unwrap();
		let action = visual.debug_bounds("components-install-egm").unwrap();

		assert!(
			toggle.top() <= action.center().y && toggle.bottom() >= action.center().y,
			"the disclosure belongs in the component header, not a separate Details row"
		);
		assert!(toggle.right() < action.left(), "component actions stay outside the disclosure");

		visual.simulate_click(toggle.center(), Default::default());

		assert_eq!(
			settings.read_with(visual, |s, _| s.components.details),
			[false, true, false, false]
		);

		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("components-egm").unwrap(), egm);
		visual.executor().advance_clock(std::time::Duration::from_millis(50));
		visual.update(crate::ui_frame::draw_frame);
		let opening = visual.debug_bounds("components-egm").unwrap();
		visual.executor().advance_clock(std::time::Duration::from_millis(150));
		visual.update(crate::ui_frame::draw_frame);
		let expanded = visual.debug_bounds("components-egm").unwrap();
		assert!(
			expanded.size.height > egm.size.height + gpui::px(24.),
			"component details retain their full natural height"
		);
		assert!(
			opening.size.height > egm.size.height && opening.size.height < expanded.size.height
		);
		for _ in 0..3 {
			visual.update(crate::ui_frame::draw_frame);
			assert_eq!(visual.debug_bounds("components-egm").unwrap(), expanded);
		}

		let toggle = visual.debug_bounds("components-egm-details").unwrap();

		visual.simulate_click(toggle.center(), Default::default());

		assert_eq!(settings.read_with(visual, |s, _| s.components.details), [false; 4]);
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("components-egm").unwrap(), expanded);
		visual.executor().advance_clock(std::time::Duration::from_millis(200));
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("components-egm").unwrap(), egm);
		assert_eq!(visual.debug_bounds("components-pro-research").unwrap(), research);
	}

	#[gpui::test]
	fn opening_settings_does_not_start_another_component_check(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});

		settings.update(visual, |s, cx| {
			s.category = crate::settings_surface::SettingsCategory::Components;

			s.refresh(cx);

			assert_eq!(
				s.components.preparation.snapshot().state,
				component_runtime::PreparationState::Unchecked
			);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-prepare-runtime").is_none());
		assert!(visual.debug_bounds("components-retry-runtime").is_none());
		assert!(visual.debug_bounds("components-component-status").is_none());
	}

	#[gpui::test]
	fn interrupted_component_install_restores_install_and_shares_startup_readback(
		cx: &mut gpui::TestAppContext,
	) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);

			settings.category = crate::settings_surface::SettingsCategory::Components;

			settings
		});

		settings.update(visual, |settings, cx| {
			settings.components.preparation = RuntimePreparation::fixture(
				component_runtime::PreparationState::Preparing(InstallProgress::Waiting),
			);

			settings.poll_components(cx);
		});
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-retry-runtime").is_none());
		assert!(visual.debug_bounds("components-component-status").is_some());

		settings.update(visual, |settings, cx| {
			settings.components.preparation.finish_fixture(Err("Download failed".into()));

			settings.poll_components(cx);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-install-runtime").is_some());
		assert!(visual.debug_bounds("components-retry-runtime").is_none());
		assert!(visual.debug_bounds("components-component-error").is_some());

		settings.update(visual, |settings, cx| {
			settings.components.preparation.finish_fixture(Ok(()));
			settings.poll_components(cx);
			assert_eq!(settings.component_status(0, cx), (Tone::Success, "Ready"));
		});
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-reinstall-runtime").is_some());
		assert!(visual.debug_bounds("components-install-runtime").is_none());
		assert!(visual.debug_bounds("components-component-error").is_none());
	}

	#[gpui::test]
	fn installed_components_keep_reinstall_available_and_disable_it_while_busy(
		cx: &mut gpui::TestAppContext,
	) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.category = crate::settings_surface::SettingsCategory::Components;
			settings.components.preparation =
				RuntimePreparation::fixture(component_runtime::PreparationState::Ready);
			settings.poll_components(cx);
			settings
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("components-reinstall-runtime").is_some());
		assert!(visual.debug_bounds("components-retry-runtime").is_none());
		settings.update(visual, |s, cx| {
			s.components.preparation.finish_fixture(Err("Installation did not complete.".into()));
			s.poll_components(cx);
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("components-reinstall-runtime").is_some());
		settings.update(visual, |s, cx| {
			s.components.shown.state =
				component_runtime::PreparationState::Preparing(InstallProgress::Waiting);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let before = settings.read_with(visual, |s, _| s.components.preparation.snapshot());
		let button = visual.debug_bounds("components-reinstall-runtime").unwrap();
		visual.simulate_click(button.center(), Default::default());
		assert_eq!(
			settings.read_with(visual, |s, _| s.components.preparation.snapshot()),
			before,
			"a disabled reinstall must not dispatch a second operation"
		);
	}
}
