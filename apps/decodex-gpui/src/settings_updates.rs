//! App updates use Sparkle; component preparation always uses this build's lock.
use crate::{native_updater::NativeUpdater, settings_surface::SettingsSurface, ui_theme};
use decodex_components::{ComponentStore, InstallProgress, RuntimeLock};
use gpui::{AnyElement, Context, KeyDownEvent, Role, prelude::*};
use std::sync::{Arc, Mutex};

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
	app_message: String,
}

impl Updates {
	pub(super) fn new() -> Self {
		// The app lifecycle prepares the runtime. Settings checks it only on request.
		let shown = ComponentState::Unchecked;
		Self {
			shared: Arc::new(Mutex::new(shown.clone())),
			shown,
			native: NativeUpdater::new(),
			app_message: "Check for a new version of Decodex.".into(),
		}
	}

	fn spawn_preparation(&self) {
		let shared = self.shared.clone();
		std::thread::spawn(move || {
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

impl SettingsSurface {
	pub(crate) fn poll_updates(&mut self, cx: &mut Context<Self>) {
		if self.updates.poll() {
			cx.notify();
		}
	}

	fn update_action(&mut self, action: Action, cx: &mut Context<Self>) {
		match action {
			Action::Check => {
				self.updates.app_message =
					self.updates.native.check().err().unwrap_or_else(|| {
						"The update window will show available versions.".into()
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
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					this.update_action(action, cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn component_card(&self, cx: &mut Context<Self>) -> gpui::Div {
		let version = RuntimeLock::embedded()
			.map(|lock| {
				if lock.source.is_some() {
					format!("Development · {}", &lock.upstream_commit[..12])
				} else {
					lock.version
				}
			})
			.unwrap_or_else(|_| "Version unavailable".into());
		let (status, color) = match &self.updates.shown {
			ComponentState::Unchecked => ("", ui_theme::TEXT_MUTED),
			ComponentState::Ready => ("Ready", ui_theme::GREEN),
			ComponentState::Preparing(_) => ("Preparing", ui_theme::BLUE),
			ComponentState::Failed(_) => ("Needs attention", ui_theme::AMBER),
		};
		let mut component = ui_theme::settings_card().p_4().gap_3()
            .child(gpui::div().flex().items_center().justify_between().gap_4()
                .child(gpui::div().flex().flex_col().gap_1()
                    .child(gpui::div().text_size(gpui::px(13.)).font_weight(gpui::FontWeight::SEMIBOLD).child("Codex"))
                    .child(gpui::div().text_size(gpui::px(11.5)).text_color(ui_theme::rgb(ui_theme::TEXT_MUTED)).child(version)))
                .when(!status.is_empty(), |header| header.child(gpui::div().flex().items_center().gap_2().text_size(gpui::px(11.5))
                    .child(gpui::div().size(gpui::px(6.)).rounded_full().bg(ui_theme::rgb(color))).child(status))))
            .child(gpui::div().text_size(gpui::px(12.)).line_height(gpui::px(18.)).text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
                .child("The component that runs your conversations. Decodex prepares the required version automatically."));
		match &self.updates.shown {
			ComponentState::Unchecked | ComponentState::Ready => {
				component = component.child(gpui::div().flex().child(self.update_button(
					"updates-prepare-runtime",
					"Check component",
					Action::Prepare,
					cx,
				)));
			},
			ComponentState::Failed(error) => {
				component = component
					.child(
						gpui::div()
							.text_size(gpui::px(12.))
							.text_color(ui_theme::rgb(ui_theme::AMBER))
							.child(error.clone()),
					)
					.child(gpui::div().flex().child(self.update_button(
						"updates-retry-runtime",
						"Retry download",
						Action::Prepare,
						cx,
					)));
			},
			ComponentState::Preparing(progress) => {
				component = component.child(
					gpui::div()
						.id("updates-component-status")
						.role(Role::Status)
						.text_size(gpui::px(12.))
						.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
						.child(progress_text(progress)),
				);
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
									.bg(ui_theme::rgb(ui_theme::BLUE)),
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
											.font_weight(gpui::FontWeight::SEMIBOLD)
											.child("Decodex"),
									)
									.child(
										gpui::div()
											.text_size(gpui::px(11.5))
											.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
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
							.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
							.child(self.updates.app_message.clone()),
					),
			)
			.child(self.component_card(cx))
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
		visual.update(|window, cx| window.draw(cx).clear(cx));
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
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("updates-check").is_some());
		assert!(visual.debug_bounds("updates-retry-runtime").is_some());
		settings.update(visual, |settings, cx| {
			*settings.updates.shared.lock().expect("state") = ComponentState::Ready;
			settings.poll_updates(cx);
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("updates-retry-runtime").is_none());
		assert!(visual.debug_bounds("updates-prepare-runtime").is_some());
	}
}
