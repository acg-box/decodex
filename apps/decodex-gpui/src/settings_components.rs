//! Optional components use their native lifecycle, separate from application updates.
use gpui::{
	AnyElement, Context, Div, KeyDownEvent, Role, SharedString,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	component_plugins::{self, PluginOperations},
	component_research::{self, ResearchOperations},
	component_runtime::{self, PreparationState, RuntimePreparation},
	component_updates::{self, Schedule},
	settings_surface::{SettingsSurface, switch},
	shell::{
		agent_surface::{RagflowComponentAction, RagflowInspection},
		workspace_symbols,
	},
	ui_feedback::{self, Feedback, Tone},
	ui_motion::SmoothControl as _,
	ui_theme::{self, BLUE, SETTINGS_LABEL_SIZE, TEXT, TEXT_MUTED},
};
use decodex_components::{
	ComponentStore, InstallProgress, PluginAction, RuntimeLock, ragflow::Update,
};
use decodex_protocol::MemoryImportStatus;

#[derive(Clone, Copy)]
enum Action {
	Prepare,
	ReinstallRuntime,
	CheckStack,
	CheckNightly,
	OpenReview,
	Plugin(PluginAction),
	Research(component_research::Action),
	Ragflow(RagflowComponentAction),
}

// Details and the component name share this leading disclosure column.
const DISCLOSURE_WIDTH: f32 = 12.;
const DISCLOSURE_GAP: f32 = 8.;

type ComponentCommand = (&'static str, Action);

#[derive(Default)]
struct InstallationActions {
	update: Option<ComponentCommand>,
	reinstall: Option<ComponentCommand>,
	uninstall: Option<ComponentCommand>,
}

#[derive(Default)]
struct ComponentActions {
	entry: Option<(&'static str, &'static str, Action)>,
	installation: InstallationActions,
}

impl ComponentActions {
	fn for_installation(
		enabled: Option<bool>,
		install: ComponentCommand,
		mut installation: InstallationActions,
	) -> Self {
		let entry = match enabled {
			None => {
				installation = InstallationActions::default();
				Some((install.0, "Install", install.1))
			},
			Some(false) => {
				installation.update = None;
				Some((install.0, "Enable", install.1))
			},
			Some(true) => None,
		};
		Self { entry, installation }
	}
}

#[derive(Clone, Copy)]
enum Policy {
	Stack,
	Egm,
	Research,
	Ragflow,
}

pub(super) struct Components {
	egm_updates: Schedule,
	research_updates: Schedule,
	ragflow_reviews: Schedule,
	stack_updates: Schedule,
	stack_check: Option<gpui::Task<()>>,
	review_check: Option<gpui::Task<()>>,
	review: Option<Result<Option<Update>, String>>,
	stack_error: Option<String>,
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
			egm_updates: Schedule::new("DecodexAutomaticEgmUpdates"),
			research_updates: Schedule::new("DecodexAutomaticResearchUpdates"),
			stack_updates: Schedule::new("DecodexAutomaticRagflowStackUpdates"),
			stack_check: None,
			ragflow_reviews: Schedule::new("DecodexAutomaticRagflowReviews"),
			review_check: None,
			review: None,
			stack_error: None,
			preparation,
			shown,
			optional: PluginOperations::new(decodex_components::egm_component(), None),
			plugin: component_plugins::Snapshot::default(),
			research: ResearchOperations::default(),
			research_shown: component_research::Snapshot::default(),
		}
	}

	fn review_target(&self) -> Option<&Update> {
		self.review.as_ref()?.as_ref().ok()?.as_ref()
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
		let state = self.preparation.snapshot().state;
		if state == PreparationState::Unchecked {
			self.preparation.prepare();
		}
		self.research.inspect();

		if matches!(state, PreparationState::Unchecked | PreparationState::Ready) {
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
	fn update_policy(&self, policy: Policy, cx: &mut Context<Self>) -> AnyElement {
		const AUTOMATIC_UPDATES: &str = "Automatic updates";
		let (id, knob, label, description, enabled) = match policy {
			Policy::Egm => (
				"components-egm-auto",
				"components-egm-auto-knob",
				AUTOMATIC_UPDATES,
				"Install plugin updates from its Codex marketplace.",
				self.components.egm_updates.enabled,
			),
			Policy::Research => (
				"components-research-auto",
				"components-research-auto-knob",
				AUTOMATIC_UPDATES,
				"Update the helper and extension from Decodex when research is idle.",
				self.components.research_updates.enabled,
			),
			Policy::Stack => (
				"components-stack-auto",
				"components-stack-auto-knob",
				AUTOMATIC_UPDATES,
				"Use versions tested with Decodex. Keep one rollback backup.",
				self.components.stack_updates.enabled,
			),
			Policy::Ragflow => (
				"components-ragflow-auto",
				"components-ragflow-auto-knob",
				"Review updates with Assistant",
				"Assistant reviews new nightly images. You choose when to apply one.",
				self.components.ragflow_reviews.enabled,
			),
		};
		ui_theme::settings_row()
			.child(ui_theme::settings_label(label, description))
			.child(
				switch::settings_switch(id, knob, label, enabled, true)
					.aria_description(description)
					.on_change(cx.listener(move |s, enabled: &bool, _, cx| {
						match policy {
							Policy::Egm => s.components.egm_updates.set(*enabled),
							Policy::Research => s.components.research_updates.set(*enabled),
							Policy::Stack => {
								s.components.stack_updates.set(*enabled);
								s.components.stack_error = None;
								s.components.stack_check = None;
							},
							Policy::Ragflow => {
								s.components.ragflow_reviews.set(*enabled);
								s.components.review_check = None;
								s.components.review = None;
							},
						}
						s.poll_components(cx);
						cx.notify();
					})),
			)
			.into_any_element()
	}

	fn check_component_updates(&mut self, cx: &mut Context<Self>) {
		if self.components.egm_updates.due() && self.components.optional.snapshot().busy.is_none() {
			match self.components.preparation.snapshot().state {
				PreparationState::Unchecked => self.components.prepare_runtime(),
				PreparationState::Ready => {
					self.components.egm_updates.checked();
					self.components.optional.start(PluginAction::UpdateIfAvailable);
				},
				PreparationState::Preparing(_) | PreparationState::Failed(_) => {},
			}
		}
		if self.components.research_updates.due()
			&& self.components.research.snapshot().busy.is_none()
		{
			self.components.research_updates.checked();
			self.components.research.start(component_research::Action::SyncBundled);
		}
		self.check_stack_update(cx);
		self.check_nightly_update(cx);
	}

	fn check_stack_update(&mut self, cx: &mut Context<Self>) {
		if self.components.stack_updates.due()
			&& self.components.stack_check.is_none()
			&& let Some(agent) =
				self.native_agent.as_ref().filter(|a| a.read(cx).can_update_stack())
		{
			let host = agent.read(cx).component_review_host();
			self.components.stack_updates.checked();
			self.components.stack_error = None;
			cx.notify();
			let check = cx
				.background_executor()
				.spawn(async { ComponentStore::for_user()?.ragflow_bundle_pending() });
			self.components.stack_check = Some(cx.spawn(async move |surface, cx| {
				let pending = check.await;
				let _ = surface.update(cx, |s, cx| {
					s.components.stack_check = None;
					match pending {
						Ok(true) => {
							if let Some(agent) = s.native_agent.as_ref().filter(|a| {
								a.read(cx).can_update_stack()
									&& a.read(cx).component_review_host() == host
							}) {
								agent.update(cx, |a, cx| a.update_bundled_ragflow(cx));
							} else {
								s.components.stack_updates.check_now();
							}
						},
						Err(e) => {
							s.components.stack_error = Some(e);
							cx.notify();
						},
						Ok(false) => {},
					}
				});
			}));
		}
	}

	fn check_nightly_update(&mut self, cx: &mut Context<Self>) {
		if !self.components.ragflow_reviews.due() || self.components.review_check.is_some() {
			return;
		}
		let Some(host) = self.native_agent.as_ref().and_then(|agent| {
			let agent = agent.read(cx);
			(!agent.ragflow_component_state().working)
				.then(|| agent.component_review_host())
				.flatten()
		}) else {
			return;
		};
		self.components.ragflow_reviews.checked();
		self.components.review = None;
		cx.notify();
		let check =
			cx.background_executor().spawn(async { ComponentStore::for_user()?.ragflow_update() });
		self.components.review_check = Some(cx.spawn(async move |surface, cx| {
			let candidate = check.await;
			let current_host = surface
				.read_with(cx, |s, cx| {
					s.native_agent.as_ref().and_then(|a| a.read(cx).component_review_host())
				})
				.ok()
				.flatten();
			let result = match candidate {
				Ok(Some(target)) if current_host.as_ref() == Some(&host) => {
					let (profile, owner) = host.clone();
					cx.background_executor()
						.spawn(async move {
							component_updates::assign_review(profile, &owner, &target)?;
							Ok(Some(target))
						})
						.await
				},
				Ok(_) => Ok(None),
				Err(error) => Err(error),
			};
			let _ = surface.update(cx, |s, cx| {
				s.components.review_check = None;
				let same_host = s
					.native_agent
					.as_ref()
					.and_then(|a| a.read(cx).component_review_host())
					.as_ref()
					== Some(&host);
				s.components.review = same_host.then_some(result);
				cx.notify();
			});
		}));
	}

	pub(crate) fn poll_components(&mut self, cx: &mut Context<Self>) {
		self.check_component_updates(cx);
		#[cfg(not(test))]
		if self.category == super::SettingsCategory::Components {
			self.components.research.inspect();
		}

		#[cfg(not(test))]
		let preparing = matches!(self.components.shown.state, PreparationState::Preparing(_));

		if self.components.poll() {
			#[cfg(not(test))]
			if preparing && self.components.shown.state == PreparationState::Ready {
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
			Action::CheckStack => {
				self.components.stack_updates.check_now();
				self.check_stack_update(cx);
			},
			Action::CheckNightly => {
				self.components.ragflow_reviews.check_now();
				self.check_nightly_update(cx);
			},
			Action::OpenReview =>
				if let Some(target) = self.components.review_target() {
					cx.emit(super::OpenReview(component_updates::work_id(target)));
				},
			Action::Plugin(action) => self.components.optional.start(action),
			Action::Research(action) => self.components.research.start(action),
			Action::Ragflow(action) => {
				match action {
					RagflowComponentAction::CheckInstallation
					| RagflowComponentAction::CancelModels
					| RagflowComponentAction::CancelReset
					| RagflowComponentAction::ToggleImport => {},
					_ => self.components.details[3] = true,
				}
				if let Some(agent) = &self.native_agent {
					if action == RagflowComponentAction::Update
						&& let Some(review) = self.components.review_target()
					{
						agent.update(cx, |s, cx| s.update_reviewed_ragflow(review.clone(), cx));
					} else {
						agent.update(cx, |s, cx| s.ragflow_component_action(action, cx));
					}
				}
				if matches!(
					action,
					RagflowComponentAction::Install
						| RagflowComponentAction::Update
						| RagflowComponentAction::Uninstall
				) {
					self.components.review_check = None;
					self.components.review = None;
					if action == RagflowComponentAction::Uninstall {
						self.components.stack_error = None;
					}
					self.components.ragflow_reviews.check_now();
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
		let label = if matches!(action, Action::Ragflow(RagflowComponentAction::Update))
			&& self.components.review_target().is_some()
		{
			"Update to proposed image"
		} else {
			label
		};
		let mut disabled_reason = "Operation in progress";
		let enabled = match action {
			Action::Prepare | Action::ReinstallRuntime =>
				!matches!(self.components.shown.state, PreparationState::Preparing(_)),
			Action::CheckStack => {
				disabled_reason = "Available when the workspace is idle.";
				self.components.stack_updates.enabled
					&& self.components.stack_check.is_none()
					&& self.native_agent.as_ref().is_some_and(|a| a.read(cx).can_update_stack())
			},
			Action::CheckNightly => {
				disabled_reason = "Available when RAGFlow is idle in the local workspace.";
				self.components.ragflow_reviews.enabled
					&& self.components.review_check.is_none()
					&& self.native_agent.as_ref().is_some_and(|a| {
						let agent = a.read(cx);
						!agent.ragflow_component_state().working
							&& agent.component_review_host().is_some()
					})
			},
			Action::OpenReview => self.components.review_target().is_some(),
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
						disabled_reason = "Enter the Base URL and both model IDs.";
						return false;
					}
					if matches!(
						action,
						RagflowComponentAction::RequestReset | RagflowComponentAction::ResetMemory
					) && !agent.ragflow_component_state().memory_ready
					{
						disabled_reason = "Configure this Assistant's Memory before resetting it.";
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
					| Action::Ragflow(
						RagflowComponentAction::RequestReset | RagflowComponentAction::ResetMemory
					)
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

	fn component_settings(&self, cx: &mut Context<Self>) -> Div {
		let lock = RuntimeLock::embedded().ok();
		let version = lock.as_ref().map(|lock| lock.version.as_str());
		let controls = match self.components.shown.action {
			Some(component_runtime::Action::Install) => ComponentActions {
				entry: Some(("components-install-runtime", "Install", Action::Prepare)),
				..Default::default()
			},
			Some(component_runtime::Action::Reinstall) => ComponentActions {
				installation: InstallationActions {
					reinstall: Some(("components-reinstall-runtime", Action::ReinstallRuntime)),
					..Default::default()
				},
				..Default::default()
			},
			None => ComponentActions::default(),
		};
		let mut component = ui_theme::settings_rows()
			.debug_selector(|| "components-codex".into())
			.child(self.component_header(
				0,
				"components-codex-details",
				"Codex",
				version,
				controls.entry,
				cx,
			));

		match &self.components.shown.state {
			PreparationState::Unchecked => {},
			PreparationState::Ready => {},
			PreparationState::Failed(error) => {
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
			PreparationState::Preparing(progress) => {
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
		component.child(self.component_body(
			0,
			"components-codex-body",
			details,
			&[],
			controls.installation,
			cx,
		))
	}

	fn toggle_component_details(&mut self, index: usize, cx: &mut Context<Self>) {
		self.components.details[index] = !self.components.details[index];
		cx.notify();
	}

	fn component_header(
		&self,
		index: usize,
		id: &'static str,
		title: &'static str,
		version: Option<&str>,
		entry: Option<(&'static str, &'static str, Action)>,
		cx: &mut Context<Self>,
	) -> Div {
		let expanded = self.components.details[index];
		let metadata = version.map(|version| SharedString::from(format!("Version {version}")));
		let (tone, status) = self.component_status(index, cx);
		let disclosure = gpui::div()
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
			.gap(gpui::px(DISCLOSURE_GAP))
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
					.w(gpui::px(DISCLOSURE_WIDTH))
					.flex_none()
					.flex()
					.justify_center()
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(workspace_symbols::process_chevron(
						SharedString::from(format!("{id}-chevron")),
						expanded,
					)),
			)
			.child(
				ui_theme::settings_rows()
					.flex_1()
					.child(
						gpui::div()
							.flex()
							.items_center()
							.gap_1()
							.child(
								gpui::div()
									.min_w_0()
									.truncate()
									.font_weight(gpui::FontWeight::MEDIUM)
									.child(title),
							)
							.child(ui_feedback::status_indicator(
								format!("{id}-status"),
								status,
								tone,
							)),
					)
					.child(
						ui_theme::settings_description(metadata.unwrap_or_else(|| status.into()))
							.truncate(),
					),
			)
			.smooth();
		ui_theme::settings_row()
			.py(gpui::px(8.))
			.child(disclosure)
			.children(entry.map(|(id, label, action)| self.component_button(id, label, action, cx)))
	}

	fn component_status(&self, index: usize, cx: &gpui::App) -> (Tone, &'static str) {
		match index {
			0 => match self.components.shown.state {
				PreparationState::Unchecked => (Tone::Neutral, "Not checked"),
				PreparationState::Preparing(_) => (Tone::Info, "Preparing"),
				PreparationState::Ready => (Tone::Success, "Ready"),
				PreparationState::Failed(_) => (Tone::Error, "Setup failed"),
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
				} else if state.inspection == RagflowInspection::Failed {
					(Tone::Error, "Installation check failed")
				} else if state.feedback.is_some_and(|f| f.tone == Tone::Error) {
					(Tone::Error, "Operation failed")
				} else if state.inspection == RagflowInspection::Unchecked {
					(Tone::Neutral, "Not checked")
				} else if !state.installed {
					(Tone::Neutral, "Not installed")
				} else if state.running == Some(false) {
					(Tone::Error, "Service unavailable")
				} else if !state.memory_ready {
					(Tone::Warning, "Memory setup required")
				} else if state.import_status == Some(MemoryImportStatus::Unavailable) {
					(Tone::Warning, "Memory import needs attention")
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
		policies: &[Policy],
		installation: InstallationActions,
		cx: &mut Context<Self>,
	) -> AnyElement {
		crate::ui_motion::settings_disclosure(id, self.components.details[index], || {
			let buttons = [
				("Update", installation.update),
				("Reinstall", installation.reinstall),
				("Uninstall", installation.uninstall),
			]
			.into_iter()
			.filter_map(|(label, command)| {
				let (id, action) = command?;
				Some(self.component_button(id, label, action, cx))
			})
			.collect::<Vec<_>>();

			ui_theme::settings_rows()
				.pl(gpui::px(DISCLOSURE_WIDTH + DISCLOSURE_GAP))
				.pt_1()
				.pb_3()
				.child(content)
				.when(!policies.is_empty(), |body| {
					body.child(
						ui_theme::settings_rows().pt_2().children(
							policies.iter().map(|&policy| self.update_policy(policy, cx)),
						),
					)
				})
				.when(!buttons.is_empty(), |body| {
					body.child(
						ui_theme::settings_row()
							.min_h_0()
							.py_0()
							.mt_2()
							.pt_2()
							.border_t_1()
							.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER))
							.flex_wrap()
							.justify_between()
							.gap_2()
							.child(ui_theme::settings_description("Installation"))
							.child(
								gpui::div()
									.min_w_0()
									.flex()
									.flex_wrap()
									.items_center()
									.gap_2()
									.ml_auto()
									.justify_end()
									.children(buttons),
							),
					)
				})
				.into_any_element()
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
		self.components.preparation = RuntimePreparation::fixture(PreparationState::Ready);
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
		let actions = match state.as_ref().map(|s| s.inspection) {
			Some(RagflowInspection::Failed) => ComponentActions {
				entry: Some((
					"components-ragflow-inspection-retry",
					"Retry",
					Action::Ragflow(RagflowComponentAction::CheckInstallation),
				)),
				..Default::default()
			},
			Some(RagflowInspection::Confirmed) => ComponentActions::for_installation(
				installed.then_some(true),
				("components-ragflow-install", Action::Ragflow(RagflowComponentAction::Install)),
				InstallationActions {
					update: Some((
						"components-ragflow-update",
						Action::Ragflow(RagflowComponentAction::Update),
					)),
					uninstall: Some((
						"components-ragflow-uninstall",
						Action::Ragflow(RagflowComponentAction::Uninstall),
					)),
					..Default::default()
				},
			),
			_ => ComponentActions::default(),
		};
		let row = self.component_header(
			3,
			"components-ragflow-details",
			"RAGFlow",
			state.as_ref().and_then(|s| s.version.as_deref()),
			actions.entry,
			cx,
		);
		let mut body = ui_theme::settings_rows();
		if let Some(state) = state.as_ref().filter(|s| s.installed) {
			body = body.child(
				ui_theme::settings_row()
					.min_h(gpui::px(36.))
					.py_0()
					.child(gpui::div().flex_1().child(ui_theme::settings_section_title("Models")))
					.when(!state.editing, |row| {
						row.child(self.component_button(
							"components-ragflow-models-edit",
							if state.has_models { "Edit" } else { "Set up models" },
							Action::Ragflow(RagflowComponentAction::ConfigureModels),
							cx,
						))
					}),
			);
		}
		body = body.child(self.ragflow_component(cx));
		if state.as_ref().is_some_and(|s| s.editing) {
			body = body.child(self.ragflow_model_actions(cx));
		}
		if let Some(state) = state.as_ref().filter(|s| s.memory_ready) {
			let description = match state.import_status {
				Some(MemoryImportStatus::Queued) =>
					"RAGFlow received the last exchange. New memories may take time to appear in search.",
				Some(MemoryImportStatus::Unavailable) =>
					"Decodex couldn’t confirm the last import. Check your RAGFlow connection and API key.",
				Some(MemoryImportStatus::Unknown) =>
					"Decodex couldn’t read the last memory import status.",
				None => "Save memories from completed Assistant conversations.",
			};
			body = body.child(
				ui_theme::settings_row()
					.min_h(gpui::px(32.))
					.child(ui_theme::settings_label("Automatic learning", description))
					.child(
						switch::settings_switch(
							"components-ragflow-import",
							"components-ragflow-import-knob",
							"Learn from Assistant conversations",
							state.import,
							!state.busy,
						)
						.aria_description(description)
						.on_change(cx.listener(|s, _, _, cx| {
							s.component_action(
								Action::Ragflow(RagflowComponentAction::ToggleImport),
								cx,
							);
						})),
					),
			);
			if !state.editing {
				body = body.child(self.ragflow_reset_controls(state.reset_confirming, cx));
			}
		}
		let policies: &[Policy] = if installed
			|| self.components.stack_updates.enabled
			|| self.components.ragflow_reviews.enabled
		{
			&[Policy::Stack, Policy::Ragflow]
		} else {
			&[]
		};
		ui_theme::settings_rows()
			.child(row)
			.child(
				ui_feedback::notice(
					"components-ragflow-feedback",
					"ragflow",
					state.and_then(|s| s.feedback),
				)
				.inline(),
			)
			.child(self.ragflow_check_feedback(cx))
			.child(self.component_body(
				3,
				"components-ragflow-body",
				body,
				policies,
				actions.installation,
				cx,
			))
			.into_any_element()
	}

	fn ragflow_check_feedback(&self, cx: &mut Context<Self>) -> Div {
		let mut rows = ui_theme::settings_rows();
		if let Some(error) = &self.components.stack_error {
			rows = rows.children(
				ui_feedback::inline_feedback(
					"components-stack-check-status",
					"Automatic update check failed",
					&Feedback::error(error),
				)
				.map(|row| {
					row.child(self.component_button(
						"components-stack-retry",
						"Retry",
						Action::CheckStack,
						cx,
					))
				}),
			);
		}
		let (title, feedback, button_label) = match &self.components.review {
			Some(Err(error)) => ("Nightly check failed", Feedback::error(error), "Retry"),
			Some(Ok(Some(target))) => (
				"Migration review",
				Feedback::info(format!("Image {}", &target.digest[7..19])),
				"Check now",
			),
			Some(Ok(None)) =>
				("Nightly check", Feedback::neutral("No new image found."), "Check now"),
			None if self.components.review_check.is_some() => {
				return rows.children(ui_feedback::inline_feedback(
					"components-nightly-check-status",
					"Nightly check",
					&Feedback::loading("Checking for a new image…"),
				));
			},
			None => return rows,
		};
		rows.children(
			ui_feedback::inline_feedback("components-nightly-check-status", title, &feedback).map(
				|row| {
					row.when(self.components.review_target().is_some(), |row| {
						row.child(self.component_button(
							"components-open-migration-review",
							"Open review",
							Action::OpenReview,
							cx,
						))
					})
					.child(self.component_button(
						"components-review-check",
						button_label,
						Action::CheckNightly,
						cx,
					))
				},
			),
		)
	}

	fn ragflow_model_actions(&self, cx: &mut Context<Self>) -> Div {
		let mut actions = gpui::div().w_full().flex().gap_2().justify_end().pt_1();
		for (id, label, action) in [
			("components-ragflow-models-cancel", "Cancel", RagflowComponentAction::CancelModels),
			("components-ragflow-models-save", "Save", RagflowComponentAction::SaveModels),
		] {
			actions = actions.child(self.component_button(id, label, Action::Ragflow(action), cx));
		}

		actions
	}

	fn ragflow_reset_controls(&self, confirming: bool, cx: &mut Context<Self>) -> Div {
		let mut actions = gpui::div().flex().items_center().gap_2();
		if confirming {
			actions = actions
				.child(self.component_button(
					"components-ragflow-reset-cancel",
					"Cancel",
					Action::Ragflow(RagflowComponentAction::CancelReset),
					cx,
				))
				.child(self.component_button(
					"components-ragflow-reset-confirm",
					"Reset",
					Action::Ragflow(RagflowComponentAction::ResetMemory),
					cx,
				));
		} else {
			actions = actions.child(self.component_button(
				"components-ragflow-reset-request",
				"Reset",
				Action::Ragflow(RagflowComponentAction::RequestReset),
				cx,
			));
		}

		ui_theme::settings_row()
			.border_t_1()
			.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER))
			.child(ui_theme::settings_label(
				if confirming { "Reset this Memory?" } else { "Reset Memory" },
				if confirming {
					"This deletes the saved Memory and cannot be undone. Chat history and model settings stay available. Automatic memory keeps its current setting."
				} else {
					"Chat history and model settings stay available."
				},
			))
			.child(actions)
	}

	fn research_controls(&self) -> ComponentActions {
		use component_research::Action as Research;
		let state = &self.components.research_shown;
		if !state.inspected {
			return ComponentActions::default();
		}
		let reinstall =
			Some(("components-reinstall-research", Action::Research(Research::Reinstall)));
		if state.installed.is_none() && state.error.is_some() {
			return ComponentActions {
				installation: InstallationActions { reinstall, ..Default::default() },
				..Default::default()
			};
		}
		let mut controls = ComponentActions::for_installation(
			state.installed.as_ref().map(|_| true),
			("components-install-research", Action::Research(Research::Install)),
			InstallationActions {
				update: Some(("components-update-research", Action::Research(Research::Update))),
				reinstall,
				uninstall: Some(("components-remove-research", Action::Research(Research::Remove))),
			},
		);
		if state.installed.is_some() {
			controls.entry = Some((
				"components-connect-research",
				if state.connection.is_some() { "Open ChatGPT" } else { "Connect" },
				Action::Research(Research::Connect),
			));
		}
		controls
	}

	fn research_settings(&self, cx: &mut Context<Self>) -> Div {
		let state = &self.components.research_shown;
		let controls = self.research_controls();
		let explanation = self.research_explanation();
		let mut details = gpui::div().w_full().flex().flex_wrap().items_center().gap_2();

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
				component_research::Action::SyncBundled =>
					Some(Feedback::loading("Checking bundled research update…")),
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
			.child(self.component_header(
				2,
				"components-research-details",
				"Decodex Pro Research",
				state.installed.as_ref().map(|p| p.version.as_str()),
				controls.entry,
				cx,
			))
			.child(
				ui_feedback::notice("components-research-feedback", "research", feedback).inline(),
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
					if state.installed.is_some() || self.components.research_updates.enabled {
						&[Policy::Research]
					} else {
						&[]
					},
					controls.installation,
					cx,
				),
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
		let controls = if state.inspected {
			ComponentActions::for_installation(
				state.installed.as_ref().map(|p| p.enabled),
				("components-install-egm", Action::Plugin(PluginAction::Install)),
				InstallationActions {
					update: Some(("components-update-egm", Action::Plugin(PluginAction::Update))),
					reinstall: Some((
						"components-reinstall-egm",
						Action::Plugin(PluginAction::Reinstall),
					)),
					uninstall: Some((
						"components-remove-egm",
						Action::Plugin(PluginAction::Remove),
					)),
				},
			)
		} else {
			ComponentActions {
				entry: Some((
					"components-refresh-egm",
					"Check status",
					Action::Plugin(PluginAction::Refresh),
				)),
				..Default::default()
			}
		};

		let feedback =
			state.error.as_ref().map(|error| Feedback::error(error.clone())).or_else(|| {
				state.busy.and_then(|action| {
					Some(Feedback::loading(match action {
						PluginAction::Refresh => return None,
						PluginAction::UpdateIfAvailable => "Checking for an EGM update…",
						PluginAction::Install => "Installing EGM…",
						PluginAction::Update => "Updating EGM…",
						PluginAction::Reinstall => "Reinstalling EGM…",
						PluginAction::Remove => "Uninstalling EGM…",
					}))
				})
			});

		let details = ui_theme::settings_rows().gap_2().child(ui_theme::settings_description(
			"Start a new conversation after installing or updating EGM to draft or review designs.",
		));
		ui_theme::settings_rows()
			.debug_selector(|| "components-egm".into())
			.child(self.component_header(
				1,
				"components-egm-details",
				"Elephant–Goldfish (EGM)",
				state.installed.as_ref().map(|p| p.version.as_str()),
				controls.entry,
				cx,
			))
			.child(ui_feedback::notice("components-egm-feedback", "egm", feedback).inline())
			.child(self.component_body(
				1,
				"components-egm-body",
				details,
				if state.installed.as_ref().is_some_and(|p| p.enabled)
					|| self.components.egm_updates.enabled
				{
					&[Policy::Egm]
				} else {
					&[]
				},
				controls.installation,
				cx,
			))
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
		component_runtime::PreparationState,
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
		let url = visual.debug_bounds("ragflow-field-url").unwrap();
		let key = visual.debug_bounds("ragflow-field-key").unwrap();
		assert_eq!(url.top(), key.top());
		assert_eq!(extraction.top(), embedding.top());
		assert_eq!(extraction.left(), url.left());
		assert_eq!(embedding.right(), key.right());
		assert!(url.right() < key.left() && extraction.right() < embedding.left());
		assert!(extraction.top() > url.bottom());
		assert!(visual.debug_bounds("ragflow-field-embedding_url").is_none());
		assert!(visual.debug_bounds("ragflow-field-embedding_key").is_none());
		let save = visual.debug_bounds("components-ragflow-models-save").unwrap();
		assert_eq!(save.right(), embedding.right());
		assert!(save.top() > embedding.bottom());
	}

	#[gpui::test]
	fn saved_ragflow_models_remain_editable_after_cancel_and_reopening(
		cx: &mut gpui::TestAppContext,
	) {
		let agent = cx.new(crate::shell::agent_surface::AgentSurface::new);
		agent.update(cx, |agent, cx| agent.visual_ragflow_saved(cx));
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.bind_native_agent(agent.clone(), cx);
			settings.category = crate::settings_surface::SettingsCategory::Components;
			settings.visual_ragflow_settings();
			settings
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("ragflow-models-summary").is_some());
		let edit = visual.debug_bounds("components-ragflow-models-edit").unwrap();
		visual.simulate_click(edit.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("ragflow-field-url").is_some());
		assert!(visual.debug_bounds("ragflow-field-embedding_key").is_none());
		let cancel = visual.debug_bounds("components-ragflow-models-cancel").unwrap();
		visual.simulate_click(cancel.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("ragflow-models-summary").is_some());
		assert!(visual.debug_bounds("components-ragflow-models-edit").is_some());

		settings.update(visual, |s, cx| {
			s.toggle_component_details(3, cx);
			s.toggle_component_details(3, cx);
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("ragflow-models-summary").is_some());
		assert!(!agent.read_with(visual, |s, _| s.ragflow_component_state().editing));
		let reset = visual.debug_bounds("components-ragflow-reset-request").unwrap();
		visual.simulate_click(reset.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("components-ragflow-reset-confirm").is_some());
		assert!(agent.read_with(visual, |s, _| s.ragflow_component_state().reset_confirming));
		let cancel = visual.debug_bounds("components-ragflow-reset-cancel").unwrap();
		visual.simulate_click(cancel.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("components-ragflow-reset-request").is_some());
		assert!(!agent.read_with(visual, |s, _| s.ragflow_component_state().reset_confirming));
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

		assert!(visual.debug_bounds("components-remove-egm").is_none());
		assert!(visual.debug_bounds("components-reinstall-egm").is_none());
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

		settings.update(visual, |s, cx| s.toggle_component_details(1, cx));
		visual.update(crate::ui_frame::draw_current_frame);
		visual.executor().advance_clock(std::time::Duration::from_millis(250));
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("components-reinstall-egm").is_some());
		let update = visual.debug_bounds("components-update-egm").unwrap();
		let remove = visual.debug_bounds("components-remove-egm").unwrap();

		settings.update(visual, |settings, cx| {
			settings.components.plugin.busy = Some(PluginAction::Refresh);

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert_eq!(visual.debug_bounds("components-update-egm").unwrap(), update);
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

		assert_eq!(visual.debug_bounds("components-update-egm").unwrap(), update);
		assert_eq!(visual.debug_bounds("components-remove-egm").unwrap(), remove);

		visual.simulate_click(update.center(), Default::default());

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
		let status = visual.debug_bounds("conversation-activity-status").unwrap();
		assert!(status.top() >= visual.debug_bounds("components-egm-details").unwrap().bottom());
		assert!(status.bottom() <= visual.debug_bounds("components-egm-auto").unwrap().top());
		assert_eq!(visual.debug_bounds("components-update-egm").unwrap().right(), update.right());
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
	fn installed_components_share_one_ordered_maintenance_footer(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let agent = cx.new(crate::shell::agent_surface::AgentSurface::new);
			agent.update(cx, |agent, cx| agent.visual_ragflow_saved(cx));
			let mut s = SettingsSurface::new(DesktopSettingsController::production(), cx);
			s.bind_native_agent(agent, cx);
			s.category = crate::settings_surface::SettingsCategory::Components;
			s.visual_managed_components();
			s
		});
		visual.simulate_resize(gpui::size(gpui::px(640.), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_current_frame);
		let footers: [&[&str]; 4] = [
			&["components-reinstall-runtime"],
			&["components-update-egm", "components-reinstall-egm", "components-remove-egm"],
			&[
				"components-update-research",
				"components-reinstall-research",
				"components-remove-research",
			],
			&["components-ragflow-update", "components-ragflow-uninstall"],
		];
		for id in footers.iter().flat_map(|row| row.iter()) {
			assert!(visual.debug_bounds(id).is_none(), "{id} appeared in the collapsed list");
		}
		let connect = visual.debug_bounds("components-connect-research").unwrap();
		let disclosure = visual.debug_bounds("components-research-details").unwrap();
		assert!(disclosure.right() < connect.left());
		for (index, ids) in footers.iter().enumerate() {
			settings.update(visual, |s, cx| {
				s.components.details = [false; 4];
				s.toggle_component_details(index, cx);
			});
			visual.update(crate::ui_frame::draw_current_frame);
			visual.executor().advance_clock(std::time::Duration::from_millis(250));
			visual.update(crate::ui_frame::draw_frame);
			let buttons: Vec<_> = ids.iter().map(|id| visual.debug_bounds(id).unwrap()).collect();
			assert_eq!(buttons.last().unwrap().right(), connect.right(), "{ids:?}");
			for pair in buttons.windows(2) {
				assert_eq!(pair[0].top(), pair[1].top());
				assert!(pair[0].right() < pair[1].left(), "{ids:?}");
			}
			if index == 1 {
				assert!(
					visual.debug_bounds("components-egm-auto").unwrap().bottom() < buttons[0].top()
				);
			}
			if index == 3 {
				let model = visual.debug_bounds("ragflow-models-summary").unwrap();
				let stack = visual.debug_bounds("components-stack-auto").unwrap();
				let nightly = visual.debug_bounds("components-ragflow-auto").unwrap();
				assert!(
					model.bottom() < stack.top()
						&& stack.bottom() < nightly.top()
						&& nightly.bottom() < buttons[0].top()
				);
			}
			if index == 2 {
				let header = visual.debug_bounds("components-connect-research").unwrap();
				let extension = visual.debug_bounds("components-research-extension").unwrap();
				let switch = visual.debug_bounds("components-research-auto").unwrap();
				assert!(extension.top() > header.bottom());
				assert!(switch.top() > extension.bottom() && buttons[0].top() > switch.bottom());
			}
		}
	}

	#[gpui::test]
	fn ragflow_check_failures_keep_separate_retry_actions(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut s = SettingsSurface::new(DesktopSettingsController::production(), cx);
			s.category = crate::settings_surface::SettingsCategory::Components;
			s.components.stack_updates.enabled = true;
			s.components.ragflow_reviews.enabled = true;
			s.components.stack_updates.checked();
			s.components.ragflow_reviews.checked();
			s.components.stack_error = Some("Bundle unavailable".into());
			s.components.review = Some(Err("Nightly unavailable".into()));
			s
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let header = visual.debug_bounds("components-ragflow-details").unwrap();
		let stack = visual.debug_bounds("components-stack-retry").unwrap();
		let nightly = visual.debug_bounds("components-review-check").unwrap();
		assert!(header.bottom() < stack.top() && stack.bottom() < nightly.top());
		assert!(visual.debug_bounds("components-stack-auto").is_none());
		settings.update(visual, |s, cx| {
			s.component_action(super::Action::CheckStack, cx);
			assert!(s.components.stack_updates.due() && !s.components.ragflow_reviews.due());
			s.components.stack_updates.checked();
			s.component_action(super::Action::CheckNightly, cx);
			assert!(!s.components.stack_updates.due() && s.components.ragflow_reviews.due());
			s.components.review = None;
			s.components.ragflow_reviews.enabled = false;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("components-stack-retry").is_some());
		assert!(visual.debug_bounds("components-review-check").is_none());
	}

	#[gpui::test]
	fn scheduled_egm_update_prepares_codex_with_settings_closed(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});
		settings.update(visual, |s, cx| {
			s.components.preparation = RuntimePreparation::fixture(PreparationState::Unchecked);
			s.components.egm_updates.enabled = true;
			s.poll_components(cx);
			assert_ne!(s.components.preparation.snapshot().state, PreparationState::Unchecked);
			assert!(s.components.egm_updates.due());
		});
	}

	#[gpui::test]
	fn opening_settings_does_not_start_another_component_check(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});

		settings.update(visual, |s, cx| {
			s.category = crate::settings_surface::SettingsCategory::Components;

			s.refresh(cx);

			assert_eq!(s.components.preparation.snapshot().state, PreparationState::Unchecked);
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
			settings.components.preparation =
				RuntimePreparation::fixture(PreparationState::Preparing(InstallProgress::Waiting));

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

		assert!(visual.debug_bounds("components-reinstall-runtime").is_none());
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
			settings.components.details[0] = true;
			settings.components.preparation = RuntimePreparation::fixture(PreparationState::Ready);

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
			s.components.shown.state = PreparationState::Preparing(InstallProgress::Waiting);

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
