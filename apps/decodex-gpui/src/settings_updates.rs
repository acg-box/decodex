//! App updates use Sparkle; component preparation always uses this build's lock.
use crate::{native_updater::NativeUpdater, settings_surface::SettingsSurface, ui_theme};
use decodex_components::{ComponentStore, InstallProgress, RuntimeLock};
use gpui::{AnyElement, Context, KeyDownEvent, Role, SharedString, prelude::*};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq)]
enum ComponentState {
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
		let shown = ComponentState::Preparing(InstallProgress::Waiting);
		let result = Self {
			shared: Arc::new(Mutex::new(shown.clone())),
			shown,
			native: NativeUpdater::new(),
			app_message: "Check for a new version of Decodex.".into(),
		};
		#[cfg(not(test))]
		result.spawn_preparation();
		result
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
		gpui::div()
			.id(id)
			.debug_selector(move || id.into())
			.role(Role::Button)
			.aria_label(label)
			.tab_index(0)
			.px_3()
			.py_2()
			.rounded_md()
			.border_1()
			.border_color(gpui::rgb(ui_theme::LINE_STRONG))
			.cursor_pointer()
			.hover(|s| s.bg(gpui::rgba(ui_theme::HOVER_FILL)))
			.focus_visible(|s| s.border_color(gpui::rgb(ui_theme::BLUE)))
			.on_click(cx.listener(move |this, _, _, cx| this.update_action(action, cx)))
			.on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					this.update_action(action, cx);
					cx.stop_propagation();
				}
			}))
			.child(label)
			.into_any_element()
	}

	pub(super) fn updates_content(&self, cx: &mut Context<Self>) -> AnyElement {
		let message = match &self.updates.shown {
			ComponentState::Preparing(progress) => progress_text(progress),
			ComponentState::Ready => "Ready".into(),
			ComponentState::Failed(error) => error.clone(),
		};
		let version = RuntimeLock::embedded()
			.map(|lock| {
				if lock.source.is_some() {
					format!("development ({})", &lock.upstream_commit[..12])
				} else {
					lock.version
				}
			})
			.unwrap_or_default();
		let mut component = gpui::div()
			.flex()
			.flex_col()
			.gap_3()
			.child(format!("Codex {version}"))
			.child(SharedString::from(message));
		match &self.updates.shown {
			ComponentState::Failed(_) => {
				component = component.child(self.update_button(
					"updates-retry-runtime",
					"Retry",
					Action::Prepare,
					cx,
				));
			},
			ComponentState::Ready => {
				component = component.child(self.update_button(
					"updates-prepare-runtime",
					"Check component",
					Action::Prepare,
					cx,
				));
			},
			ComponentState::Preparing(_) => {},
		}
		gpui::div()
			.flex()
			.flex_col()
			.gap_6()
			.child(
				gpui::div()
					.flex()
					.flex_col()
					.gap_3()
					.child(format!("Decodex {}", env!("CARGO_PKG_VERSION")))
					.child(SharedString::from(self.updates.app_message.clone()))
					.child(self.update_button(
						"updates-check",
						"Check for updates",
						Action::Check,
						cx,
					)),
			)
			.child(component)
			.child(gpui::div().text_color(gpui::rgb(ui_theme::TEXT_MUTED)).child(
				"Release components download automatically. Sessions start when the pinned component is ready.",
			))
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
		updates.prepare_runtime();
		assert_eq!(
			*updates.shared.lock().expect("state"),
			ComponentState::Preparing(InstallProgress::Waiting)
		);
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
