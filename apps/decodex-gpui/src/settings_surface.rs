//! Product-level settings for the sole Decodex desktop application.
//!
//! Persistent settings remain daemon-owned. This presentation controls the restored native
//! Swift menu-bar panel only after it applies an authoritative protocol readback.

use crate::ui_motion::{SmoothControl, switch_knob};
use gpui::{
	Context, Render, Role, SharedString, Window, accesskit::Toggled, div, prelude::*, px, rgb, rgba,
};

use crate::{
	desktop_settings::{
		DesktopSettingsCommandState, DesktopSettingsController, DesktopSettingsInputError,
		DesktopSettingsLoadState, DesktopSettingsSnapshot,
	},
	native_menu_bar::{LaunchAtLoginState, NativeMenuBarHost},
	ui_theme,
};

const LINE: u32 = ui_theme::LINE_STRONG;
const TEXT: u32 = ui_theme::TEXT;
const TEXT_MUTED: u32 = ui_theme::TEXT_MUTED;
const BLUE: u32 = ui_theme::BLUE;
const GREEN: u32 = ui_theme::GREEN;
const AMBER: u32 = ui_theme::AMBER;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuBarRuntimeState {
	Visible,
	Hidden,
	Waiting,
	Unavailable,
}

impl MenuBarRuntimeState {
	const fn color(self) -> u32 {
		match self {
			Self::Visible => GREEN,
			Self::Hidden => TEXT_MUTED,
			Self::Waiting => AMBER,
			Self::Unavailable => 0xb56a6a,
		}
	}
}

pub(crate) struct SettingsSurface {
	advanced_preferences: Option<gpui::AnyView>,
	snapshot: DesktopSettingsSnapshot,
	runtime: MenuBarRuntimeState,
	detail: SharedString,
	controller: DesktopSettingsController,
	menu_bar: NativeMenuBarHost,
	launch_at_login: LaunchAtLoginState,
	launch_at_login_detail: SharedString,
}

impl SettingsSurface {
	pub(crate) fn with_advanced_preferences(mut self, view: gpui::AnyView) -> Self {
		self.advanced_preferences = Some(view);
		self
	}

	pub(crate) fn new(controller: DesktopSettingsController, _: &mut Context<Self>) -> Self {
		let snapshot = controller.snapshot();
		let mut menu_bar = NativeMenuBarHost::new();
		let launch_at_login =
			menu_bar.launch_at_login_state().unwrap_or(LaunchAtLoginState::OperationFailed);
		let mut surface = Self {
			advanced_preferences: None,
			snapshot,
			runtime: MenuBarRuntimeState::Waiting,
			detail: "Loading preferences…".into(),
			controller,
			menu_bar,
			launch_at_login,
			launch_at_login_detail: launch_at_login_detail(launch_at_login).into(),
		};
		surface.apply_snapshot(snapshot);
		surface
	}

	pub(crate) fn bind_controller(
		&mut self,
		controller: DesktopSettingsController,
		cx: &mut Context<Self>,
	) {
		self.controller = controller;
		self.synchronize(cx);
		self.refresh_launch_at_login();
		cx.notify();
	}

	pub(crate) fn was_launched_as_login_item(&self) -> bool {
		self.menu_bar.was_launched_as_login_item()
	}

	fn refresh_launch_at_login(&mut self) {
		match self.menu_bar.launch_at_login_state() {
			Ok(state) => {
				self.launch_at_login = state;
				self.launch_at_login_detail = launch_at_login_detail(state).into();
			},
			Err(failure) => {
				self.launch_at_login = LaunchAtLoginState::OperationFailed;
				self.launch_at_login_detail = failure.detail().into();
			},
		}
	}

	pub(crate) fn synchronize(&mut self, cx: &mut Context<Self>) {
		let snapshot = self.controller.snapshot();
		if snapshot != self.snapshot {
			self.snapshot = snapshot;
			self.apply_snapshot(snapshot);
			cx.notify();
		}
	}

	pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
		self.synchronize(cx);
	}

	fn apply_snapshot(&mut self, snapshot: DesktopSettingsSnapshot) {
		let Some(settings) = snapshot.settings else {
			self.runtime = if matches!(
				snapshot.load,
				DesktopSettingsLoadState::Unavailable | DesktopSettingsLoadState::Refused
			) {
				MenuBarRuntimeState::Unavailable
			} else {
				MenuBarRuntimeState::Waiting
			};
			self.detail = settings_detail(snapshot).into();
			return;
		};
		if snapshot.load != DesktopSettingsLoadState::Ready {
			self.runtime = MenuBarRuntimeState::Waiting;
			self.detail = settings_detail(snapshot).into();
			return;
		}

		match self.menu_bar.apply(settings.show_in_menu_bar) {
			Ok(visible) => {
				self.runtime = if visible {
					MenuBarRuntimeState::Visible
				} else {
					MenuBarRuntimeState::Hidden
				};
				self.detail = if visible {
					"Menu bar enabled."
				} else {
					"The Decodex menu-bar item is disabled."
				}
				.into();
				if matches!(
					snapshot.command,
					DesktopSettingsCommandState::Refused
						| DesktopSettingsCommandState::OutcomeUnknown
				) {
					self.detail = settings_detail(snapshot).into();
				}
			},
			Err(failure) => {
				self.runtime = MenuBarRuntimeState::Unavailable;
				self.detail = failure.detail().into();
			},
		}
	}

	fn menubar_needs_attention(&self) -> bool {
		if matches!(
			self.snapshot.load,
			DesktopSettingsLoadState::NeverRequested | DesktopSettingsLoadState::Loading
		) || matches!(
			self.snapshot.command,
			DesktopSettingsCommandState::Sending | DesktopSettingsCommandState::AwaitingResult
		) {
			return false;
		}
		!matches!(self.runtime, MenuBarRuntimeState::Visible | MenuBarRuntimeState::Hidden)
			|| !matches!(
				self.detail.as_ref(),
				"Menu bar enabled." | "The Decodex menu-bar item is disabled."
			)
	}

	fn toggle_menubar(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
		let Some(settings) = self.snapshot.settings else {
			return;
		};
		match self.controller.set_show_in_menu_bar(!settings.show_in_menu_bar) {
			// The switch already disables itself while the command is pending.
			// Keep the current presentation until authoritative readback arrives.
			Ok(()) => {},
			Err(error) => {
				self.detail = input_error_detail(error).into();
			},
		}
		self.snapshot = self.controller.snapshot();
		cx.notify();
	}

	fn toggle_launch_at_login(
		&mut self,
		_: &gpui::ClickEvent,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		let enabled = !self.launch_at_login.is_requested();
		match self.menu_bar.set_launch_at_login(enabled) {
			Ok(state) => {
				self.launch_at_login = state;
				self.launch_at_login_detail = launch_at_login_detail(state).into();
			},
			Err(failure) => {
				self.launch_at_login_detail = failure.detail().into();
				if let Ok(state) = self.menu_bar.launch_at_login_state() {
					self.launch_at_login = state;
				}
			},
		}
		cx.notify();
	}

	fn open_login_items_settings(
		&mut self,
		_: &gpui::ClickEvent,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		if let Err(failure) = self.menu_bar.open_login_items_settings() {
			self.launch_at_login_detail = failure.detail().into();
		}
		cx.notify();
	}

	fn toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let enabled = self.snapshot.settings.is_some_and(|settings| settings.show_in_menu_bar);
		let interactive = self.snapshot.can_toggle;
		div()
			.id("menubar-surface-toggle")
			.role(Role::Switch)
			.aria_label("Show Decodex in the menu bar")
			.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
			.w(px(36.0))
			.h(px(20.0))
			.p(px(2.0))
			.flex()
			.items_center()
			.rounded_full()
			.border_1()
			.border_color(rgb(if enabled { BLUE } else { LINE }))
			.bg(if enabled { rgba(0x8baaf730) } else { rgba(0xffffff0c) })
			.opacity(if interactive { 1.0 } else { 0.58 })
			.when(interactive, |toggle| {
				toggle
					.cursor_pointer()
					.hover(|element| element.border_color(rgb(TEXT_MUTED)))
					.active(|element| element.opacity(0.78))
					.focus_visible(|element| element.border_color(rgb(BLUE)))
					.on_click(cx.listener(Self::toggle_menubar))
			})
			.child(switch_knob(
				"settings-knob",
				enabled,
				div().size(px(14.0)).rounded_full().bg(rgb(if enabled {
					BLUE
				} else {
					TEXT_MUTED
				})),
			))
			.smooth()
			.enabled(interactive)
	}

	fn launch_at_login_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let enabled = self.launch_at_login.is_requested();
		let interactive = !matches!(
			self.launch_at_login,
			LaunchAtLoginState::NotFound | LaunchAtLoginState::OperationFailed
		);
		div()
			.id("launch-at-login-toggle")
			.debug_selector(|| "launch-at-login-toggle".into())
			.role(Role::Switch)
			.aria_label("Launch Decodex at login")
			.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
			.w(px(36.0))
			.h(px(20.0))
			.p(px(2.0))
			.flex()
			.items_center()
			.rounded_full()
			.border_1()
			.border_color(rgb(if enabled { BLUE } else { LINE }))
			.bg(if enabled { rgba(0x8baaf730) } else { rgba(0xffffff0c) })
			.opacity(if interactive { 1.0 } else { 0.58 })
			.when(interactive, |toggle| {
				toggle
					.cursor_pointer()
					.hover(|element| element.border_color(rgb(TEXT_MUTED)))
					.active(|element| element.opacity(0.78))
					.focus_visible(|element| element.border_color(rgb(BLUE)))
					.on_click(cx.listener(Self::toggle_launch_at_login))
			})
			.child(switch_knob(
				"login-knob",
				enabled,
				div().size(px(14.0)).rounded_full().bg(rgb(if enabled {
					BLUE
				} else {
					TEXT_MUTED
				})),
			))
			.smooth()
			.enabled(interactive)
	}

	fn launch_at_login_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let needs_attention = !matches!(
			self.launch_at_login,
			LaunchAtLoginState::Enabled | LaunchAtLoginState::NotRegistered
		) || self.launch_at_login_detail.as_ref()
			!= launch_at_login_detail(self.launch_at_login);
		ui_theme::settings_row()
			.min_h(px(40.0))
			.px(px(0.0))
			.child(
				div()
					.flex_1()
					.min_w_0()
					.flex()
					.flex_col()
					.gap(px(3.0))
					.child("Launch at login")
					.when(needs_attention, |d| {
						d.child(
							div()
								.id("launch-at-login-status")
								.role(Role::Status)
								.text_size(px(ui_theme::CAPTION_SIZE))
								.text_color(rgb(launch_at_login_color(self.launch_at_login)))
								.child(self.launch_at_login_detail.clone()),
						)
					}),
			)
			.when(needs_attention, |d| {
				d.child(
					div()
						.id("open-login-items-settings")
						.role(Role::Button)
						.aria_label("Open Login Items settings")
						.h(px(28.0))
						.px_2()
						.flex()
						.items_center()
						.rounded(px(6.0))
						.text_size(px(11.0))
						.text_color(rgb(BLUE))
						.cursor_pointer()
						.on_click(cx.listener(Self::open_login_items_settings))
						.child("Open settings")
						.smooth(),
				)
			})
			.child(self.launch_at_login_toggle(cx))
	}
}

impl SettingsSurface {
	fn cursor_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		use crate::composer_input::cursor::{Preference, Shape};
		let current = Preference::configured();
		let choices = [
			(
				"Cursor style",
				vec![
					("Bar", Preference { shape: Shape::Bar, ..current }),
					("Block", Preference { shape: Shape::Block, ..current }),
					("Underline", Preference { shape: Shape::Underline, ..current }),
				],
			),
			(
				"Cursor blinking",
				vec![
					("On", Preference { blinking: true, ..current }),
					("Off", Preference { blinking: false, ..current }),
				],
			),
		];
		div().flex().flex_col().children(choices.into_iter().map(|(title, values)| {
			ui_theme::settings_row().px_0().child(div().flex_1().child(title)).child(
				div().flex().gap_1().p_1().rounded(px(9.)).bg(rgba(0xffffff08)).children(
					values.into_iter().map(|(label, value)| {
						div()
							.id(gpui::SharedString::from(format!("{title}-{label}")))
							.role(Role::Button)
							.aria_label(format!("{title}: {label}"))
							.aria_toggled(if value == current {
								Toggled::True
							} else {
								Toggled::False
							})
							.tab_index(0)
							.px_3()
							.h(px(26.))
							.flex()
							.items_center()
							.rounded(px(6.))
							.text_size(px(11.))
							.cursor_pointer()
							.when(value == current, |d| d.bg(rgba(0xffffff16)))
							.hover(|d| d.bg(rgba(0xffffff12)))
							.on_click(cx.listener(move |_, _, _, cx| {
								value.select(cx);
								cx.notify();
							}))
							.on_key_down(cx.listener(
								move |_, event: &gpui::KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										value.select(cx);
										cx.notify();
										cx.stop_propagation();
									}
								},
							))
							.child(label)
							.smooth()
					}),
				),
			)
		}))
	}

	fn glass_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		use ui_theme::window_material::GlassStyle;
		let style = GlassStyle::configured();
		ui_theme::settings_row().px_0().child(div().flex_1().child("Glass appearance")).child(
			div().flex().gap_1().p_1().rounded(px(9.)).bg(rgba(0xffffff08)).children(
				[(GlassStyle::Regular, "Regular"), (GlassStyle::Clear, "Clear")].into_iter().map(
					|(value, label)| {
						div()
							.id(label)
							.debug_selector(move || label.to_string())
							.role(Role::Button)
							.aria_label(format!("Glass appearance: {label}"))
							.aria_toggled(if value == style {
								Toggled::True
							} else {
								Toggled::False
							})
							.tab_index(0)
							.px_3()
							.h(px(26.))
							.flex()
							.items_center()
							.rounded(px(6.))
							.text_size(px(11.))
							.cursor_pointer()
							.when(value == style, |d| d.bg(rgba(0xffffff16)))
							.hover(|d| d.bg(rgba(0xffffff12)))
							.on_click(cx.listener(move |_, _, _, cx| {
								value.select(cx);
								cx.notify();
							}))
							.on_key_down(cx.listener(
								move |_, event: &gpui::KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										value.select(cx);
										cx.notify();
										cx.stop_propagation();
									}
								},
							))
							.child(label)
							.smooth()
					},
				),
			),
		)
	}
}

impl Render for SettingsSurface {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let needs_attention = self.menubar_needs_attention();
		div()
			.id("settings-surface")
			.role(Role::Main)
			.aria_label("Decodex settings")
			.size_full()
			.min_w_0()
			.min_h_0()
			.text_size(px(ui_theme::BODY_SIZE))
			.line_height(px(ui_theme::BODY_LINE_HEIGHT))
			.text_color(rgb(TEXT))
			.child(
				div()
					.id("settings-scroll-viewport")
					.size_full()
					.overflow_y_scroll()
					.px(px(28.0))
					.py(px(18.0))
					.flex()
					.justify_center()
					.child(
						div()
							.w_full()
							.max_w(px(600.0))
							.flex()
							.flex_col()
							.gap(px(24.0))
							.child(ui_theme::settings_title("General"))
							.child(self.glass_controls(cx))
							.child(self.cursor_controls(cx))
							.child(
								div().flex().flex_col().gap(px(8.0)).child(
									div()
										.w_full()
										.py(px(4.0))
										.flex()
										.flex_col()
										.child(
											ui_theme::settings_row()
												.min_h(px(40.0))
												.px(px(0.0))
												.child(
													div()
														.flex_1()
														.min_w_0()
														.flex()
														.flex_col()
														.gap(px(3.0))
														.child("Show in menu bar")
														.when(needs_attention, |d| {
															d.child(
																div()
																	.id("menubar-runtime-status")
																	.debug_selector(|| {
																		"menubar-runtime-status"
																			.into()
																	})
																	.role(Role::Status)
																	.text_size(px(
																		ui_theme::CAPTION_SIZE,
																	))
																	.text_color(rgb(self
																		.runtime
																		.color()))
																	.child(self.detail.clone()),
															)
														}),
												)
												.child(self.toggle(cx)),
										)
										.child(div().h(px(2.0)))
										.child(self.launch_at_login_card(cx)),
								),
							)
							.children(self.advanced_preferences.clone())
							.child(quote_attribution()),
					),
			)
	}
}

const fn launch_at_login_color(state: LaunchAtLoginState) -> u32 {
	match state {
		LaunchAtLoginState::Enabled => GREEN,
		LaunchAtLoginState::RequiresApproval => AMBER,
		LaunchAtLoginState::NotRegistered => TEXT_MUTED,
		LaunchAtLoginState::NotFound | LaunchAtLoginState::OperationFailed => 0xb56a6a,
	}
}

const fn launch_at_login_detail(state: LaunchAtLoginState) -> &'static str {
	match state {
		LaunchAtLoginState::NotRegistered => "Decodex does not start automatically at login.",
		LaunchAtLoginState::Enabled => "macOS will start Decodex quietly when you sign in.",
		LaunchAtLoginState::RequiresApproval =>
			"macOS requires approval in System Settings > General > Login Items.",
		LaunchAtLoginState::NotFound =>
			"Install Decodex.app in Applications before enabling launch at login.",
		LaunchAtLoginState::OperationFailed => "The macOS login-item state is unavailable.",
	}
}

const fn settings_detail(snapshot: DesktopSettingsSnapshot) -> &'static str {
	match snapshot.load {
		DesktopSettingsLoadState::NeverRequested => "Waiting for the Decodex settings query.",
		DesktopSettingsLoadState::Loading => "Loading your menu-bar preference.",
		DesktopSettingsLoadState::Ready => match snapshot.command {
			DesktopSettingsCommandState::Sending | DesktopSettingsCommandState::AwaitingResult =>
				"Saving preference…",
			DesktopSettingsCommandState::OutcomeUnknown =>
				"Reading back the menu-bar preference after an uncertain response.",
			DesktopSettingsCommandState::Refused =>
				"The Decodex service refused the menu-bar preference change.",
			DesktopSettingsCommandState::Idle | DesktopSettingsCommandState::Accepted =>
				"Your menu-bar preference is saved.",
		},
		DesktopSettingsLoadState::Offline =>
			"Connect to the Decodex service to read desktop settings.",
		DesktopSettingsLoadState::Unavailable => "Daemon-owned desktop settings are unavailable.",
		DesktopSettingsLoadState::Refused => "The desktop settings response was invalid.",
	}
}

const fn input_error_detail(error: DesktopSettingsInputError) -> &'static str {
	match error {
		DesktopSettingsInputError::Offline =>
			"Connect to the Decodex service before changing this setting.",
		DesktopSettingsInputError::Busy => "Wait for the current settings request to finish.",
		DesktopSettingsInputError::NotLoaded => "Wait for preferences to load.",
		DesktopSettingsInputError::IdentityUnavailable =>
			"Decodex could not create a bounded settings command identity.",
	}
}

fn quote_attribution() -> impl IntoElement {
	div()
		.id("quote-source")
		.role(Role::Link)
		.tab_index(0)
		.aria_label("Quotes provided by ZenQuotes. Open source website.")
		.text_size(px(11.))
		.text_color(rgb(TEXT_MUTED))
		.cursor_pointer()
		.hover(|d| d.text_color(rgb(TEXT)))
		.on_click(|_, _, cx| cx.open_url("https://zenquotes.io/"))
		.on_key_down(|event, _, cx| {
			if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				cx.open_url("https://zenquotes.io/");
				cx.stop_propagation();
			}
		})
		.child("Inspirational quotes provided by ZenQuotes API")
}

#[cfg(test)]
mod tests {
	use gpui::{TestAppContext, size};

	use super::*;

	#[gpui::test]
	fn saving_does_not_insert_a_status_row_or_move_other_controls(cx: &mut TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});
		settings.update(visual, |s, cx| {
			s.snapshot.load = DesktopSettingsLoadState::Ready;
			s.snapshot.command = DesktopSettingsCommandState::Idle;
			s.runtime = MenuBarRuntimeState::Hidden;
			s.detail = "The Decodex menu-bar item is disabled.".into();
			cx.notify();
		});
		visual.update(|window, cx| {
			window.resize(size(px(800.), px(700.)));
			window.draw(cx).clear();
		});
		let original = visual.debug_bounds("launch-at-login-toggle").expect("login toggle");
		for command in
			[DesktopSettingsCommandState::Sending, DesktopSettingsCommandState::AwaitingResult]
		{
			settings.update(visual, |s, cx| {
				s.snapshot.command = command;
				s.runtime = MenuBarRuntimeState::Waiting;
				s.detail = "Saving preference…".into();
				cx.notify();
			});
			visual.update(|window, cx| window.draw(cx).clear());
			assert!(visual.debug_bounds("menubar-runtime-status").is_none());
			assert_eq!(visual.debug_bounds("launch-at-login-toggle"), Some(original));
		}
		settings.update(visual, |s, cx| {
			s.snapshot.command = DesktopSettingsCommandState::Refused;
			s.detail = "The service refused the change.".into();
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear());
		assert!(visual.debug_bounds("menubar-runtime-status").is_some());
	}

	#[gpui::test]
	fn glass_style_buttons_update_the_active_material(cx: &mut TestAppContext) {
		use ui_theme::window_material::GlassStyle;
		let controller = DesktopSettingsController::production();
		let (_, visual) = cx.add_window_view(|_, cx| SettingsSurface::new(controller, cx));
		visual.update(|window, cx| {
			window.resize(size(px(800.), px(600.)));
			window.draw(cx).clear();
		});
		let clear = visual.debug_bounds("Clear").expect("Clear control");
		visual.simulate_click(clear.center(), Default::default());
		assert_eq!(GlassStyle::configured(), GlassStyle::Clear);
		let regular = visual.debug_bounds("Regular").expect("Regular control");
		visual.simulate_click(regular.center(), Default::default());
		assert_eq!(GlassStyle::configured(), GlassStyle::Regular);
	}

	#[gpui::test]
	fn settings_draw_at_the_selected_desktop_size(cx: &mut TestAppContext) {
		let controller = DesktopSettingsController::production();
		let (_settings, visual) = cx.add_window_view(|_, cx| SettingsSurface::new(controller, cx));
		visual.update(|window, cx| {
			window.resize(size(px(1_490.0), px(1_055.0)));
			window.draw(cx).clear();
		});
	}
}
