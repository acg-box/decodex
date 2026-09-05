//! Product-level settings for the sole Decodex desktop application.
//!
//! Persistent settings remain daemon-owned. This presentation controls the restored native
//! Swift menu-bar panel only after it applies an authoritative protocol readback.

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
	const fn label(self) -> &'static str {
		match self {
			Self::Visible => "VISIBLE",
			Self::Hidden => "OFF",
			Self::Waiting => "SYNCING",
			Self::Unavailable => "UNAVAILABLE",
		}
	}

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
	snapshot: DesktopSettingsSnapshot,
	runtime: MenuBarRuntimeState,
	detail: SharedString,
	controller: DesktopSettingsController,
	menu_bar: NativeMenuBarHost,
	launch_at_login: LaunchAtLoginState,
	launch_at_login_detail: SharedString,
}

impl SettingsSurface {
	pub(crate) fn new(controller: DesktopSettingsController, _: &mut Context<Self>) -> Self {
		let snapshot = controller.snapshot();
		let mut menu_bar = NativeMenuBarHost::new();
		let launch_at_login =
			menu_bar.launch_at_login_state().unwrap_or(LaunchAtLoginState::OperationFailed);
		let mut surface = Self {
			snapshot,
			runtime: MenuBarRuntimeState::Waiting,
			detail: "Waiting for daemon-owned desktop settings.".into(),
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
					"Decodex.app owns the original native Swift menu-bar panel in this process."
				} else {
					"The Decodex menu-bar item is disabled."
				}
				.into();
			},
			Err(failure) => {
				self.runtime = MenuBarRuntimeState::Unavailable;
				self.detail = failure.detail().into();
			},
		}
	}

	fn toggle_menubar(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
		let Some(settings) = self.snapshot.settings else {
			return;
		};
		match self.controller.set_show_in_menu_bar(!settings.show_in_menu_bar) {
			Ok(()) => {
				self.runtime = MenuBarRuntimeState::Waiting;
				self.detail = "Saving the menu-bar preference through the Decodex service.".into();
			},
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
			.w(px(52.0))
			.h(px(28.0))
			.p(px(3.0))
			.flex()
			.items_center()
			.rounded_full()
			.border_1()
			.border_color(rgb(if enabled { BLUE } else { LINE }))
			.bg(rgb(if enabled { 0x17314c } else { 0x151b20 }))
			.opacity(if interactive { 1.0 } else { 0.58 })
			.when(interactive, |toggle| {
				toggle
					.cursor_pointer()
					.hover(|element| element.border_color(rgb(TEXT_MUTED)))
					.active(|element| element.opacity(0.78))
					.focus_visible(|element| element.border_color(rgb(BLUE)))
					.on_click(cx.listener(Self::toggle_menubar))
			})
			.child(
				div()
					.size(px(20.0))
					.rounded_full()
					.bg(rgb(if enabled { BLUE } else { TEXT_MUTED }))
					.when(enabled, |knob| knob.ml_auto()),
			)
	}

	fn launch_at_login_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let enabled = self.launch_at_login.is_requested();
		let interactive = !matches!(
			self.launch_at_login,
			LaunchAtLoginState::NotFound | LaunchAtLoginState::OperationFailed
		);
		div()
			.id("launch-at-login-toggle")
			.role(Role::Switch)
			.aria_label("Launch Decodex at login")
			.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
			.w(px(52.0))
			.h(px(28.0))
			.p(px(3.0))
			.flex()
			.items_center()
			.rounded_full()
			.border_1()
			.border_color(rgb(if enabled { BLUE } else { LINE }))
			.bg(rgb(if enabled { 0x17314c } else { 0x151b20 }))
			.opacity(if interactive { 1.0 } else { 0.58 })
			.when(interactive, |toggle| {
				toggle
					.cursor_pointer()
					.hover(|element| element.border_color(rgb(TEXT_MUTED)))
					.active(|element| element.opacity(0.78))
					.focus_visible(|element| element.border_color(rgb(BLUE)))
					.on_click(cx.listener(Self::toggle_launch_at_login))
			})
			.child(
				div()
					.size(px(20.0))
					.rounded_full()
					.bg(rgb(if enabled { BLUE } else { TEXT_MUTED }))
					.when(enabled, |knob| knob.ml_auto()),
			)
	}

	fn launch_at_login_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let state_color = launch_at_login_color(self.launch_at_login);
		div()
			.px_5()
			.py_5()
			.flex()
			.items_center()
			.gap_6()
			.border_1()
			.border_color(rgba(0xffffff12))
			.rounded(px(14.0))
			.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
			.child(
				div()
					.flex_1()
					.min_w_0()
					.flex()
					.flex_col()
					.gap_3()
					.child(
						div()
							.flex()
							.items_center()
							.gap_3()
							.child(div().text_size(px(15.0)).child("Launch at login"))
							.child(
								div()
									.px_2()
									.py_1()
									.border_1()
									.border_color(rgb(state_color))
									.rounded_full()
									.font_family("SF Mono")
									.text_size(px(8.0))
									.text_color(rgb(state_color))
									.child(launch_at_login_label(self.launch_at_login)),
							),
					)
					.child(
						div()
							.text_size(px(11.0))
							.line_height(px(16.0))
							.text_color(rgb(TEXT_MUTED))
							.child(
								"Start Decodex quietly after you sign in. Closing the window keeps Decodex and its app-owned daemon running; Quit Decodex stops both.",
							),
					)
					.child(
						div()
							.id("launch-at-login-status")
							.role(Role::Status)
							.aria_label(self.launch_at_login_detail.clone())
							.font_family("SF Mono")
							.text_size(px(8.5))
							.text_color(rgb(state_color))
							.child(self.launch_at_login_detail.clone()),
					)
					.when(
						matches!(
							self.launch_at_login,
							LaunchAtLoginState::RequiresApproval
								| LaunchAtLoginState::NotFound
								| LaunchAtLoginState::OperationFailed
						),
						|content| {
							content.child(
								div()
									.id("open-login-items-settings")
									.role(Role::Button)
									.aria_label("Open Login Items settings")
									.px_3()
									.py_2()
									.rounded(px(7.0))
									.border_1()
									.border_color(rgb(LINE))
									.text_size(px(9.5))
									.cursor_pointer()
									.hover(|element| element.border_color(rgb(BLUE)))
									.on_click(cx.listener(Self::open_login_items_settings))
									.child("Open Login Items…"),
							)
						},
					),
			)
			.child(self.launch_at_login_toggle(cx))
	}
}

impl Render for SettingsSurface {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let runtime_color = self.runtime.color();
		div()
			.id("settings-surface")
			.role(Role::Main)
			.aria_label("Decodex settings")
			.size_full()
			.min_w_0()
			.min_h_0()
			.flex()
			.flex_col()
			.bg(rgba(0x00000000))
			.text_color(rgb(TEXT))
			.child(
				div()
					.id("settings-scroll-viewport")
					.flex_1()
					.min_h_0()
					.overflow_y_scroll()
					.px_7()
					.py_6()
					.flex()
					.justify_center()
					.child(
						div()
							.w_full()
							.max_w(px(940.0))
							.flex()
							.flex_col()
							.gap_6()
							.child(settings_header())
							.child(
								div()
									.px_5()
									.py_5()
									.flex()
									.items_center()
									.gap_6()
									.border_1()
									.border_color(rgba(0xffffff12))
									.rounded(px(14.0))
									.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
									.child(menu_bar_description(
										self.runtime,
										self.detail.clone(),
										runtime_color,
									))
									.child(self.toggle(cx)),
							)
							.child(self.launch_at_login_card(cx))
							.child(authority_boundary()),
					),
			)
	}
}

const fn launch_at_login_label(state: LaunchAtLoginState) -> &'static str {
	match state {
		LaunchAtLoginState::NotRegistered => "OFF",
		LaunchAtLoginState::Enabled => "ON",
		LaunchAtLoginState::RequiresApproval => "APPROVAL",
		LaunchAtLoginState::NotFound | LaunchAtLoginState::OperationFailed => "UNAVAILABLE",
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

fn settings_header() -> impl IntoElement {
	div()
		.flex()
		.flex_col()
		.gap_2()
		.child(
			div()
				.font_family("SF Mono")
				.text_size(px(8.0))
				.text_color(rgb(BLUE))
				.child("SETTINGS / DESKTOP SURFACES"),
		)
		.child(div().text_size(px(22.0)).child("One application, one service."))
		.child(
			div()
				.max_w(px(700.0))
				.text_size(px(11.5))
				.line_height(px(17.0))
				.text_color(rgb(TEXT_MUTED))
				.child(
					"Decodex.app owns the main window and the optional menu-bar item. The Decodex service owns product behavior and persistent settings.",
				),
		)
}

fn menu_bar_description(
	runtime: MenuBarRuntimeState,
	detail: SharedString,
	runtime_color: u32,
) -> impl IntoElement {
	div()
		.flex_1()
		.min_w_0()
		.flex()
		.flex_col()
		.gap_3()
		.child(
			div()
				.flex()
				.items_center()
				.gap_3()
				.child(div().text_size(px(15.0)).child("Menu bar"))
				.child(
					div()
						.px_2()
						.py_1()
						.border_1()
						.border_color(rgb(runtime_color))
						.rounded_full()
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(runtime_color))
						.child(runtime.label()),
				),
		)
		.child(
			div()
				.text_size(px(11.0))
				.line_height(px(16.0))
				.text_color(rgb(TEXT_MUTED))
				.child(
					"Show Decodex in the menu bar. The item opens the same Decodex.app process; accounts, quota, routing, login, and recovery remain protocol-backed views in the app.",
				),
		)
		.child(
			div()
				.id("menubar-runtime-status")
				.role(Role::Status)
				.aria_label(detail.clone())
				.font_family("SF Mono")
				.text_size(px(8.5))
				.text_color(rgb(runtime_color))
				.child(detail),
		)
}

fn authority_boundary() -> impl IntoElement {
	div()
		.p_6()
		.flex()
		.flex_col()
		.gap_5()
		.border_1()
		.border_color(rgba(0xffffff0e))
		.rounded(px(14.0))
		.bg(rgba(ui_theme::SURFACE_MATERIAL))
		.child(
			div()
				.font_family("SF Mono")
				.text_size(px(10.0))
				.text_color(rgb(TEXT_MUTED))
				.child("PROCESS AND AUTHORITY BOUNDARY"),
		)
		.child(
			div()
				.flex()
				.items_center()
				.justify_between()
				.text_size(px(11.5))
				.child(boundary_node("DECODEX.APP", "window · menu bar", BLUE))
				.child(boundary_edge("typed protocol"))
				.child(boundary_node("DECODEX SERVICE", "state · behavior · effects", GREEN)),
		)
}

fn boundary_node(title: &'static str, detail: &'static str, color: u32) -> impl IntoElement {
	div()
		.w(px(250.0))
		.px_4()
		.py_3()
		.flex()
		.flex_col()
		.gap_1()
		.border_1()
		.border_color(rgba((color << 8) | 0x66))
		.rounded(px(9.0))
		.bg(rgba(0xffffff05))
		.child(div().font_family("SF Mono").text_size(px(9.0)).text_color(rgb(color)).child(title))
		.child(div().text_size(px(9.5)).text_color(rgb(TEXT_MUTED)).child(detail))
}

fn boundary_edge(label: &'static str) -> impl IntoElement {
	div()
		.flex_1()
		.min_w(px(100.0))
		.flex()
		.flex_col()
		.items_center()
		.gap_2()
		.text_size(px(9.0))
		.text_color(rgb(TEXT_MUTED))
		.child(label)
		.child(div().w_full().h(px(1.0)).bg(rgb(LINE)))
}

const fn settings_detail(snapshot: DesktopSettingsSnapshot) -> &'static str {
	match snapshot.load {
		DesktopSettingsLoadState::NeverRequested => "Waiting for the Decodex settings query.",
		DesktopSettingsLoadState::Loading => "Loading the daemon-owned menu-bar preference.",
		DesktopSettingsLoadState::Ready => match snapshot.command {
			DesktopSettingsCommandState::Sending | DesktopSettingsCommandState::AwaitingResult =>
				"Saving the menu-bar preference through the Decodex service.",
			DesktopSettingsCommandState::OutcomeUnknown =>
				"Reading back the menu-bar preference after an uncertain response.",
			DesktopSettingsCommandState::Refused =>
				"The Decodex service refused the menu-bar preference change.",
			DesktopSettingsCommandState::Idle | DesktopSettingsCommandState::Accepted =>
				"The daemon-owned menu-bar preference is current.",
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
		DesktopSettingsInputError::NotLoaded => "Wait for daemon-owned settings to load.",
		DesktopSettingsInputError::IdentityUnavailable =>
			"Decodex could not create a bounded settings command identity.",
	}
}

#[cfg(test)]
mod tests {
	use gpui::{TestAppContext, size};

	use super::*;

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
