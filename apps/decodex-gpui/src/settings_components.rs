//! Optional components use their native lifecycle, separate from application updates.
use gpui::{
	AnyElement, Context, Div, KeyDownEvent,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	component_plugins::{PluginOperations, Snapshot},
	component_runtime::{PreparationState as ComponentState, RuntimePreparation},
	settings_surface::SettingsSurface,
	ui_feedback::{self, Feedback},
	ui_theme::{self, BLUE},
};
use decodex_components::{InstallProgress, PluginAction, RuntimeLock, egm_component};

#[derive(Clone, Copy)]
enum Action {
	Prepare,
	Plugin(PluginAction),
}

pub(super) struct Components {
	preparation: RuntimePreparation,
	shown: ComponentState,
	optional: PluginOperations,
	plugin: Snapshot,
}
impl Components {
	pub(super) fn new(preparation: RuntimePreparation) -> Self {
		let shown = preparation.snapshot();

		Self {
			preparation,
			shown,
			optional: PluginOperations::new(egm_component(), None),
			plugin: Snapshot::default(),
		}
	}

	fn prepare_runtime(&self) {
		self.preparation.prepare();
	}

	pub(super) fn poll(&mut self) -> bool {
		let next = self.preparation.snapshot();
		let plugin = self.optional.snapshot();
		let changed = next != self.shown || plugin != self.plugin;

		self.shown = next;
		self.plugin = plugin;

		changed
	}

	#[cfg(not(test))]
	pub(super) fn refresh_optional(&self) {
		if matches!(self.preparation.snapshot(), ComponentState::Unchecked | ComponentState::Ready)
		{
			self.optional.start(PluginAction::Refresh);
		}
	}
}

impl SettingsSurface {
	pub(crate) fn poll_components(&mut self, cx: &mut Context<Self>) {
		#[cfg(not(test))]
		let preparing = matches!(self.components.shown, ComponentState::Preparing(_));

		if self.components.poll() {
			#[cfg(not(test))]
			if preparing
				&& self.components.shown == ComponentState::Ready
				&& self.category == super::SettingsCategory::Components
			{
				self.components.refresh_optional();
			}

			cx.notify();
		}
	}

	fn component_action(&mut self, action: Action, cx: &mut Context<Self>) {
		match action {
			Action::Prepare => self.components.prepare_runtime(),
			Action::Plugin(action) => self.components.optional.start(action),
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
		ui_theme::settings_button(id, label)
			.on_click(cx.listener(move |this, _, _, cx| this.component_action(action, cx)))
			.on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					this.component_action(action, cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn component_settings(&self, cx: &mut Context<Self>) -> Div {
		let version = RuntimeLock::embedded()
			.map(|lock| {
				if lock.source.is_some() {
					format!("Development · {}", &lock.upstream_commit[..12])
				} else {
					lock.version
				}
			})
			.unwrap_or_else(|_| "Version unavailable".into());
		let action = match self.components.shown {
			ComponentState::Preparing(_) => None,
			ComponentState::Failed(_) => Some(("components-retry-runtime", "Retry setup")),
			_ => Some(("components-prepare-runtime", "Verify")),
		};
		let mut component = gpui::div().w_full().flex().flex_col().child(
			ui_theme::settings_row()
				.child(ui_theme::settings_label(
					"Codex",
					format!(
						"{version}\nRequired for conversations. Follows the runtime included with this Decodex build."
					),
				))
				.when_some(action, |row, (id, label)| {
					row.child(self.component_button(id, label, Action::Prepare, cx))
				}),
		);

		match &self.components.shown {
			ComponentState::Unchecked => {},
			ComponentState::Ready => {
				component = component.children(ui_feedback::inline_feedback(
					"components-component-ready",
					"",
					&Feedback::success("Ready"),
				));
			},
			ComponentState::Failed(error) => {
				component = component.children(ui_feedback::inline_feedback(
					"components-component-error",
					"Could not prepare Codex",
					&Feedback::error(error.clone()),
				));
			},
			ComponentState::Preparing(progress) => {
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

		component
	}

	pub(super) fn components_content(&self, cx: &mut Context<Self>) -> AnyElement {
		ui_theme::settings_group(
			"This Mac",
			vec![
				self.component_settings(cx).into_any_element(),
				self.egm_settings(cx).into_any_element(),
			],
		)
		.into_any_element()
	}

	fn egm_settings(&self, cx: &mut Context<Self>) -> Div {
		let state = &self.components.plugin;
		let status = state.status_text();
		let mut controls = gpui::div().flex().items_center().gap_2();

		if state.busy.is_none() {
			controls = controls.child(self.component_button(
				"components-refresh-egm",
				"Refresh",
				Action::Plugin(PluginAction::Refresh),
				cx,
			));
			if state.inspected {
				let (label, action) = match &state.installed {
					None => ("Install", PluginAction::Install),
					Some(plugin) if !plugin.enabled => ("Enable", PluginAction::Install),
					Some(_) => ("Update", PluginAction::Update),
				};

				controls = controls.child(self.component_button(
					"components-install-egm",
					label,
					Action::Plugin(action),
					cx,
				));
				controls = controls.child(self.component_button(
					"components-remove-egm",
					if state.installed.is_some() { "Uninstall" } else { "Clean up" },
					Action::Plugin(PluginAction::Remove),
					cx,
				));
			}
		}

		let feedback =
			state.error.as_ref().map(|error| Feedback::error(error.clone())).or_else(|| {
				state.busy.map(|action| {
					Feedback::info(match action {
						PluginAction::Refresh => "Reading native plugin state…",
						PluginAction::Install => "Installing EGM…",
						PluginAction::Update => "Updating EGM…",
						PluginAction::Remove => "Uninstalling EGM…",
					})
				})
			});

		gpui::div().w_full().flex().flex_col().child(ui_theme::settings_row()
			.child(ui_theme::settings_label("Elephant–Goldfish (EGM)", format!("{status}\nGoogle's complete design and review plugin. Optional; uses your Codex subscription.\nStart a new conversation after installing or updating.")))
			.child(controls))
			.children(feedback.as_ref().and_then(|feedback| ui_feedback::inline_feedback("components-egm-feedback", "", feedback)))
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
	use decodex_components::{PluginAction, PluginInstallation};

	use crate::settings_surface::components::{
		ComponentState, RuntimePreparation, SettingsSurface,
	};

	#[gpui::test]
	fn optional_component_controls_follow_native_readback_and_busy_state(
		cx: &mut gpui::TestAppContext,
	) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(
				crate::desktop_settings::DesktopSettingsController::production(),
				cx,
			);

			settings.category = crate::settings_surface::SettingsCategory::Components;
			settings
		});

		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("components-refresh-egm").is_some());
		assert!(
			visual.debug_bounds("components-install-egm").is_none(),
			"inspect before writing native configuration"
		);
		settings.update(visual, |settings, cx| {
			settings.components.plugin.inspected = true;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("components-install-egm").is_some());
		assert!(visual.debug_bounds("components-remove-egm").is_some());
		settings.update(visual, |settings, cx| {
			settings.components.plugin.installed =
				Some(PluginInstallation { version: "0.1.0".into(), enabled: true });
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("components-remove-egm").is_some());
		settings.update(visual, |settings, cx| {
			settings.components.plugin.busy = Some(PluginAction::Update);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("components-install-egm").is_none());
		assert!(visual.debug_bounds("components-remove-egm").is_none());
		assert!(visual.debug_bounds("components-egm-feedback").is_some());
	}

	#[gpui::test]
	fn opening_settings_does_not_start_another_component_check(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(
				crate::desktop_settings::DesktopSettingsController::production(),
				cx,
			)
		});

		settings.update(visual, |s, cx| {
			s.category = crate::settings_surface::SettingsCategory::Components;

			s.refresh(cx);

			assert_eq!(s.components.preparation.snapshot(), ComponentState::Unchecked);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-prepare-runtime").is_some());
		assert!(visual.debug_bounds("components-component-status").is_none());
	}

	#[gpui::test]
	fn failed_components_offer_retry_and_share_startup_readback(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(
				crate::desktop_settings::DesktopSettingsController::production(),
				cx,
			);

			settings.category = crate::settings_surface::SettingsCategory::Components;

			settings
		});

		settings.update(visual, |settings, cx| {
			settings.components.preparation =
				RuntimePreparation::fixture(ComponentState::Failed("Download failed".into()));

			settings.poll_components(cx);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-prepare-runtime").is_none());
		assert!(visual.debug_bounds("components-retry-runtime").is_some());

		settings.update(visual, |settings, cx| {
			settings.components.preparation.finish_fixture(Ok(()));

			settings.poll_components(cx);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("components-retry-runtime").is_none());
		assert!(visual.debug_bounds("components-prepare-runtime").is_some());
	}
}
