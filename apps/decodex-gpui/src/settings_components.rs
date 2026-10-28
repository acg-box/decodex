//! Optional components use their native lifecycle, separate from application updates.
use gpui::{
	AnyElement, Context, Div, KeyDownEvent,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	component_runtime::{PreparationState as ComponentState, RuntimePreparation},
	settings_surface::SettingsSurface,
	ui_feedback::{self, Feedback},
	ui_theme::{self, BLUE},
};
use decodex_components::{InstallProgress, RuntimeLock};

#[derive(Clone, Copy)]
enum Action {
	Prepare,
}

pub(super) struct Components {
	preparation: RuntimePreparation,
	shown: ComponentState,
}
impl Components {
	pub(super) fn new(preparation: RuntimePreparation) -> Self {
		let shown = preparation.snapshot();

		Self { preparation, shown }
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
	pub(crate) fn poll_components(&mut self, cx: &mut Context<Self>) {
		if self.components.poll() {
			cx.notify();
		}
	}

	fn component_action(&mut self, action: Action, cx: &mut Context<Self>) {
		match action {
			Action::Prepare => self.components.prepare_runtime(),
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
		ui_theme::settings_group("This Mac", vec![self.component_settings(cx).into_any_element()])
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
	use crate::settings_surface::components::{
		ComponentState, RuntimePreparation, SettingsSurface,
	};

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
