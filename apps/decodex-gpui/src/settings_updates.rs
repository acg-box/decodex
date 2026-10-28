//! Application updates use Sparkle. Component operations have a separate destination.

use gpui::{
	AnyElement, Context, KeyDownEvent,
	prelude::{
		InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _,
	},
};

use crate::{
	native_updater::NativeUpdater,
	settings_surface::SettingsSurface,
	ui_feedback::{self, Feedback},
	ui_theme,
};

pub(super) struct Updates {
	native: NativeUpdater,
	feedback: Option<Feedback>,
}
impl Updates {
	pub(super) fn new() -> Self {
		Self { native: NativeUpdater::new(), feedback: None }
	}
}
impl SettingsSurface {
	fn check_app_update(&mut self, cx: &mut Context<Self>) {
		self.updates.feedback = self.updates.native.check().err().map(Feedback::error);
		cx.notify();
	}

	pub(super) fn updates_content(&self, cx: &mut Context<Self>) -> AnyElement {
		ui_theme::settings_group(
			"Application",
			vec![
				gpui::div()
					.child(
						ui_theme::settings_row()
							.child(ui_theme::settings_label(
								"Decodex",
								format!("Version {}", env!("CARGO_PKG_VERSION")),
							))
							.child(
								ui_theme::settings_button("updates-check", "Check for updates")
									.on_click(cx.listener(|s, _, _, cx| s.check_app_update(cx)))
									.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
										if !e.is_held
											&& ["enter", "space"]
												.contains(&e.keystroke.key.as_str())
										{
											s.check_app_update(cx);
											cx.stop_propagation();
										}
									})),
							),
					)
					.children(self.updates.feedback.as_ref().and_then(|f| {
						ui_feedback::inline_feedback("updates-app-feedback", "App update", f)
					}))
					.into_any_element(),
			],
		)
		.into_any_element()
	}
}
