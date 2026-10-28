//! App updates use Sparkle; component preparation always uses this build's lock.
use gpui::{
	AnyElement, Context, Div, KeyDownEvent,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	component_runtime::{PreparationState as ComponentState, RuntimePreparation},
	native_updater::NativeUpdater,
	settings_surface::SettingsSurface,
	ui_feedback::{self, Feedback},
	ui_theme::{self, BLUE},
};
use decodex_components::{InstallProgress, RuntimeLock};

#[derive(Clone, Copy)]
enum Action {
	Check,
	Prepare,
}

pub(super) struct Updates {
	preparation: RuntimePreparation,
	shown: ComponentState,
	native: NativeUpdater,
	app_feedback: Option<Feedback>,
}
impl Updates {
	pub(super) fn new(preparation: RuntimePreparation) -> Self {
		let shown = preparation.snapshot();

		Self { preparation, shown, native: NativeUpdater::new(), app_feedback: None }
	}

	fn prepare_runtime(&self) {
		self.preparation.prepare();
	}

	pub(super) fn poll(&mut self) -> bool {
		let next = self.preparation.snapshot();
		let changed = next != self.shown;

		self.shown = next;

		changed
	}
}

impl SettingsSurface {
	pub(crate) fn poll_updates(&mut self, cx: &mut Context<Self>) {
		if self.updates.poll() {
			cx.notify();
		}
	}

	fn update_action(&mut self, action: Action, cx: &mut Context<Self>) {
		match action {
			Action::Check => {
				self.updates.app_feedback = self.updates.native.check().err().map(Feedback::error);
			},
			Action::Prepare => self.updates.prepare_runtime(),
		}

		self.updates.poll();
		cx.notify();
	}

	fn update_button(
		&self,
		id: &'static str,
		label: &'static str,
		action: Action,
		cx: &mut Context<Self>,
	) -> AnyElement {
		ui_theme::settings_button(id, label)
			.on_click(cx.listener(move |this, _, _, cx| this.update_action(action, cx)))
			.on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					this.update_action(action, cx);
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
		let action = match self.updates.shown {
			ComponentState::Preparing(_) => None,
			ComponentState::Failed(_) => Some(("updates-retry-runtime", "Retry setup")),
			_ => Some(("updates-prepare-runtime", "Check Codex")),
		};
		let mut component = gpui::div().w_full().flex().flex_col().child(
			ui_theme::settings_row()
				.child(ui_theme::settings_label(
					"Codex",
					format!(
						"{version}\nRuns your conversations. Prepared automatically by Decodex."
					),
				))
				.when_some(action, |row, (id, label)| {
					row.child(self.update_button(id, label, Action::Prepare, cx))
				}),
		);

		match &self.updates.shown {
			ComponentState::Unchecked => {},
			ComponentState::Ready => {
				component = component.children(ui_feedback::inline_feedback(
					"updates-component-ready",
					"",
					&Feedback::success("Ready"),
				));
			},
			ComponentState::Failed(error) => {
				component = component.children(ui_feedback::inline_feedback(
					"updates-component-error",
					"Could not prepare Codex",
					&Feedback::error(error.clone()),
				));
			},
			ComponentState::Preparing(progress) => {
				component = component.children(ui_feedback::inline_feedback(
					"updates-component-status",
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

	pub(super) fn updates_content(&self, cx: &mut Context<Self>) -> AnyElement {
		let app = gpui::div()
			.w_full()
			.flex()
			.flex_col()
			.child(
				ui_theme::settings_row()
					.child(ui_theme::settings_label(
						"Decodex",
						format!("Version {}", env!("CARGO_PKG_VERSION")),
					))
					.child(self.update_button(
						"updates-check",
						"Check for updates",
						Action::Check,
						cx,
					)),
			)
			.children(self.updates.app_feedback.as_ref().and_then(|feedback| {
				ui_feedback::inline_feedback("updates-app-feedback", "App update", feedback)
			}));

		ui_theme::settings_group(
			"Installed software",
			vec![app.into_any_element(), self.component_settings(cx).into_any_element()],
		)
		.into_any_element()
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
	use crate::settings_surface::updates::{ComponentState, RuntimePreparation, SettingsSurface};

	#[gpui::test]
	fn opening_settings_does_not_start_another_component_check(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(
				crate::desktop_settings::DesktopSettingsController::production(),
				cx,
			)
		});

		settings.update(visual, |s, cx| {
			s.category = crate::settings_surface::SettingsCategory::Updates;

			s.refresh(cx);

			assert_eq!(s.updates.preparation.snapshot(), ComponentState::Unchecked);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("updates-prepare-runtime").is_some());
		assert!(visual.debug_bounds("updates-component-status").is_none());
	}

	#[gpui::test]
	fn failed_components_offer_retry_without_blocking_app_updates(cx: &mut gpui::TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(
				crate::desktop_settings::DesktopSettingsController::production(),
				cx,
			);

			settings.category = crate::settings_surface::SettingsCategory::Updates;

			settings
		});

		settings.update(visual, |settings, cx| {
			settings.updates.preparation =
				RuntimePreparation::fixture(ComponentState::Failed("Download failed".into()));

			settings.poll_updates(cx);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("updates-check").is_some());
		assert!(visual.debug_bounds("updates-retry-runtime").is_some());

		settings.update(visual, |settings, cx| {
			settings.updates.preparation.finish_fixture(Ok(()));

			settings.poll_updates(cx);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("updates-retry-runtime").is_none());
		assert!(visual.debug_bounds("updates-prepare-runtime").is_some());
	}
}
