//! App updates use Sparkle; component preparation always uses this build's lock.
use std::{
	sync::{Arc, Mutex},
	thread,
};

use gpui::{
	AnyElement, Context, Div, FontWeight, KeyDownEvent,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	native_updater::NativeUpdater,
	settings_surface::SettingsSurface,
	ui_feedback::{self, Feedback},
	ui_theme::{self, BLUE, TEXT_MUTED},
};
use decodex_components::{ComponentStore, InstallProgress, RuntimeLock};

#[derive(Clone, Debug, PartialEq)]
enum ComponentState {
	Unchecked,
	Preparing(InstallProgress),
	Ready,
	Failed(String),
}

#[derive(Clone, Copy)]
enum Action {
	Check,
	Prepare,
}

pub(super) struct Updates {
	shared: Arc<Mutex<ComponentState>>,
	shown: ComponentState,
	native: NativeUpdater,
	app_feedback: Option<Feedback>,
}
impl Updates {
	pub(super) fn new() -> Self {
		// The app lifecycle prepares the runtime. Settings checks it only on request.
		let shown = ComponentState::Unchecked;

		Self {
			shared: Arc::new(Mutex::new(shown.clone())),
			shown,
			native: NativeUpdater::new(),
			app_feedback: None,
		}
	}

	fn spawn_preparation(&self) {
		let shared = self.shared.clone();

		thread::spawn(move || {
			let result = RuntimeLock::embedded().and_then(|lock| {
				ComponentStore::for_user()?.ensure(&lock, |progress| {
					*shared.lock().expect("component state") = ComponentState::Preparing(progress);
				})
			});

			*shared.lock().expect("component state") = match result {
				Ok(_) => ComponentState::Ready,
				Err(error) => ComponentState::Failed(error),
			};
		});
	}

	fn prepare_runtime(&self) {
		let mut state = self.shared.lock().expect("component state");

		// Check the worker's state, not a rendered snapshot: repeated clicks cannot
		// queue another download before the view receives its next frame.
		if matches!(*state, ComponentState::Preparing(_)) {
			return;
		}

		*state = ComponentState::Preparing(InstallProgress::Waiting);

		drop(state);

		self.spawn_preparation();
	}

	pub(super) fn poll(&mut self) -> bool {
		let next = self.shared.lock().expect("component state").clone();
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
				self.updates.app_feedback = Some(match self.updates.native.check() {
					Ok(()) => Feedback::info("The update window will show available versions."),
					Err(error) => Feedback::error(error),
				});
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

	fn component_card(&self, cx: &mut Context<Self>) -> Div {
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
			ComponentState::Failed(_) => Some(("updates-retry-runtime", "Retry download")),
			_ => Some(("updates-prepare-runtime", "Check component")),
		};
		let mut component = ui_theme::settings_card().p_4().gap_3()
            .child(gpui::div().flex().items_center().justify_between().gap_4()
                .child(gpui::div().flex_1().min_w_0().flex().flex_col().gap_1()
                    .child(gpui::div().text_size(gpui::px(13.)).font_weight(FontWeight::SEMIBOLD).child("Codex"))
                    .child(gpui::div().text_size(gpui::px(11.5)).text_color(ui_theme::rgb(TEXT_MUTED)).child(version)))
                .when_some(action, |header, (id, label)| header.child(self.update_button(id, label, Action::Prepare, cx))))
            .child(gpui::div().text_size(gpui::px(12.)).line_height(gpui::px(18.)).text_color(ui_theme::rgb(TEXT_MUTED))
                .child("The component that runs your conversations. Decodex prepares the required version automatically."));

		match &self.updates.shown {
			ComponentState::Unchecked => {},
			ComponentState::Ready => {
				component = component.children(ui_feedback::inline_feedback(
					"updates-component-ready",
					"Codex is ready",
					&Feedback::success("The required component is available."),
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
		gpui::div()
			.flex()
			.flex_col()
			.gap_5()
			.child(
				ui_theme::settings_card()
					.p_4()
					.gap_3()
					.child(
						gpui::div()
							.flex()
							.items_center()
							.justify_between()
							.gap_4()
							.child(
								gpui::div()
									.flex()
									.flex_col()
									.gap_1()
									.child(
										gpui::div()
											.text_size(gpui::px(13.))
											.font_weight(FontWeight::SEMIBOLD)
											.child("Decodex"),
									)
									.child(
										gpui::div()
											.text_size(gpui::px(11.5))
											.text_color(ui_theme::rgb(TEXT_MUTED))
											.child(format!(
												"Version {}",
												env!("CARGO_PKG_VERSION")
											)),
									),
							)
							.child(self.update_button(
								"updates-check",
								"Check for updates",
								Action::Check,
								cx,
							)),
					)
					.child(
						gpui::div()
							.text_size(gpui::px(12.))
							.line_height(gpui::px(18.))
							.text_color(ui_theme::rgb(TEXT_MUTED))
							.child("Check for a new version of Decodex."),
					)
					.children(self.updates.app_feedback.as_ref().and_then(|feedback| {
						ui_feedback::inline_feedback("updates-app-feedback", "App update", feedback)
					})),
			)
			.child(self.component_card(cx))
			.into_any_element()
	}
}

fn progress_text(progress: &InstallProgress) -> String {
	match progress {
		InstallProgress::Waiting => "Checking the component cache…".into(),
		InstallProgress::Downloading { received, total } => match total {
			Some(total) if *total > 0 => format!("Downloading… {}%", received * 100 / total),
			_ => format!("Downloading… {} MB", received / 1_000_000),
		},
		InstallProgress::Verifying => "Verifying the component…".into(),
		InstallProgress::Ready => "Ready".into(),
	}
}

#[cfg(test)]
mod tests {
	use crate::settings_surface::updates::{
		ComponentState, InstallProgress, SettingsSurface, Updates,
	};

	#[test]
	fn repeated_clicks_do_not_queue_downloads_while_the_worker_is_busy() {
		let mut updates = Updates::new();

		updates.shown = ComponentState::Failed("Previous failure".into());
		*updates.shared.lock().expect("state") =
			ComponentState::Preparing(InstallProgress::Waiting);

		updates.prepare_runtime();

		assert_eq!(
			*updates.shared.lock().expect("state"),
			ComponentState::Preparing(InstallProgress::Waiting)
		);
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
			s.category = crate::settings_surface::SettingsCategory::Updates;

			s.refresh(cx);

			assert_eq!(*s.updates.shared.lock().expect("state"), ComponentState::Unchecked);
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
			*settings.updates.shared.lock().expect("state") =
				ComponentState::Failed("Download failed".into());

			settings.poll_updates(cx);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("updates-check").is_some());
		assert!(visual.debug_bounds("updates-retry-runtime").is_some());

		settings.update(visual, |settings, cx| {
			*settings.updates.shared.lock().expect("state") = ComponentState::Ready;

			settings.poll_updates(cx);
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("updates-retry-runtime").is_none());
		assert!(visual.debug_bounds("updates-prepare-runtime").is_some());
	}
}
