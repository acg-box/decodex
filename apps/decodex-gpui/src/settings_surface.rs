//! Product-level settings for the sole Decodex desktop application.
//!
//! Persistent settings remain daemon-owned. This presentation controls the restored native
//! Swift menu-bar panel only after it applies an authoritative protocol readback.

#[path = "settings_power.rs"] mod power;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuBarRuntimeState {
	Visible,
	Hidden,
	Waiting,
	Unavailable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsCategory {
	#[default]
	General,
	Appearance,
}
impl SettingsCategory {
	pub(crate) fn title(self) -> &'static str {
		match self {
			Self::General => "General",
			Self::Appearance => "Appearance",
		}
	}
}

pub(crate) struct SettingsSurface {
	pub(crate) category: SettingsCategory,
	power: power::PowerSettings,
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
			power: Default::default(),
			category: SettingsCategory::General,
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
		self.refresh_power(cx);
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

	pub(crate) fn notifications(&self) -> Vec<(&'static str, String)> {
		let mut notices = Vec::new();
		if let Some(error) = &self.power.error {
			notices.push(("System sleep", error.clone()));
		}
		if self.menubar_needs_attention() {
			notices.push(("Menu bar", self.detail.to_string()));
		}
		if !matches!(
			self.launch_at_login,
			LaunchAtLoginState::Enabled | LaunchAtLoginState::NotRegistered
		) || self.launch_at_login_detail.as_ref() != launch_at_login_detail(self.launch_at_login)
		{
			notices.push(("Launch at login", self.launch_at_login_detail.to_string()));
		}
		notices
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

	fn toggle_activation(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
		let Some(settings) = self.snapshot.settings else {
			return;
		};
		if let Err(error) = self.controller.set_auto_activate_quota(!settings.auto_activate_quota) {
			self.detail = input_error_detail(error).into();
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

	pub(crate) fn open_login_items_settings(
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

	fn toggle(&self, activation: bool, cx: &mut Context<Self>) -> impl IntoElement {
		let enabled = self.snapshot.settings.is_some_and(|settings| {
			if activation { settings.auto_activate_quota } else { settings.show_in_menu_bar }
		});
		let interactive = self.snapshot.can_toggle;
		div()
			.id(if activation { "quota-activation-toggle" } else { "menubar-surface-toggle" })
			.role(Role::Switch)
			.aria_label(if activation {
				"Automatically activate weekly quota"
			} else {
				"Show Decodex in the menu bar"
			})
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
					.on_click(cx.listener(if activation {
						Self::toggle_activation
					} else {
						Self::toggle_menubar
					}))
			})
			.child(switch_knob(
				if activation { "activation-knob" } else { "settings-knob" },
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
		ui_theme::settings_row()
			.px_0()
			.child(div().flex_1().child("Launch at login"))
			.child(self.launch_at_login_toggle(cx))
	}
}

impl SettingsSurface {
	fn notification_count_control(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let enabled = crate::shell::notification_count_preference(None);
		ui_theme::settings_row()
			.px_0()
			.child(div().flex_1().child("Show notification count"))
			.child(
				div()
					.id("notification-count-preference")
					.debug_selector(|| "notification-count-preference".into())
					.role(Role::Switch)
					.aria_label("Show notification count")
					.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
					.tab_index(0)
					.w(px(36.))
					.h(px(20.))
					.p(px(2.))
					.flex()
					.items_center()
					.rounded_full()
					.border_1()
					.border_color(rgb(if enabled { BLUE } else { LINE }))
					.bg(if enabled { rgba(0x8baaf730) } else { rgba(0xffffff0c) })
					.cursor_pointer()
					.hover(|d| d.border_color(rgb(TEXT_MUTED)))
					.focus_visible(|d| d.border_color(rgb(BLUE)))
					.on_click(cx.listener(move |_, _, _, cx| {
						crate::shell::notification_count_preference(Some(!enabled));
						cx.refresh_windows();
					}))
					.on_key_down(cx.listener(move |_, event: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							crate::shell::notification_count_preference(Some(!enabled));
							cx.refresh_windows();
							cx.stop_propagation();
						}
					}))
					.child(switch_knob(
						"notification-count-knob",
						enabled,
						div().size(px(14.)).rounded_full().bg(rgb(if enabled {
							BLUE
						} else {
							TEXT_MUTED
						})),
					))
					.smooth(),
			)
	}

	fn panel_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		use crate::panel_preferences::PanelDefaults;
		let current = PanelDefaults::configured();
		div().flex().flex_col().children(
			[
				(true, "Default sidebar width", current.sidebar),
				(false, "Default dock height", current.dock),
			]
			.into_iter()
			.map(|(sidebar, title, value)| {
				ui_theme::settings_row().px_0().child(div().flex_1().child(title)).child(
					div()
						.flex()
						.items_center()
						.gap_2()
						.child(div().text_size(px(12.)).child(format!("{value} px")))
						.children(
							[(-24i32, "−"), (24, "+")]
								.into_iter()
								.map(|(delta, label)| {
									div()
										.id(gpui::SharedString::from(format!(
											"panel-default-{sidebar}-{delta}"
										)))
										.role(Role::Button)
										.aria_label(format!(
											"{} {title}",
											if delta < 0 { "Decrease" } else { "Increase" }
										))
										.tab_index(0)
										.size(px(26.))
										.flex()
										.items_center()
										.justify_center()
										.rounded(px(7.))
										.cursor_pointer()
										.hover(|s| s.bg(rgba(0xffffff12)))
										.on_click(cx.listener(move |_, _, _, cx| {
											let mut pref = PanelDefaults::configured();
											if sidebar {
												pref.sidebar = (i32::from(pref.sidebar) + delta)
													.clamp(160, 480) as u16;
											} else {
												pref.dock = (i32::from(pref.dock) + delta)
													.clamp(120, 480) as u16;
											}
											pref.select(cx);
										}))
										.on_key_down(cx.listener(
											move |_, event: &gpui::KeyDownEvent, _, cx| {
												if !["enter", "space"]
													.contains(&event.keystroke.key.as_str())
												{
													return;
												}
												let mut pref = PanelDefaults::configured();
												if sidebar {
													pref.sidebar = (i32::from(pref.sidebar) + delta)
														.clamp(160, 480)
														as u16;
												} else {
													pref.dock = (i32::from(pref.dock) + delta)
														.clamp(120, 480)
														as u16;
												}
												pref.select(cx);
												cx.stop_propagation();
											},
										))
										.child(label)
										.smooth()
								})
								.collect::<Vec<_>>(),
						),
				)
			}),
		)
	}

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

impl SettingsSurface {
	pub(crate) fn quota_control(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		div()
			.flex_none()
			.flex()
			.flex_col()
			.gap(px(3.))
			.child(
				ui_theme::settings_row()
					.px_0()
					.child(div().flex_1().child("Auto-activate weekly quota"))
					.child(self.toggle(true, cx)),
			)
			.child(div().text_size(px(11.)).text_color(rgb(TEXT_MUTED)).child(
				"Start the next weekly window with a small request. Uses quota; no chat is saved.",
			))
			.into_any_element()
	}

	fn category_content(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let group = || div().flex().flex_col().gap(px(2.));
		match self.category {
			SettingsCategory::General => div()
				.flex()
				.flex_col()
				.gap(px(24.))
				.child(
					group()
						.child(settings_group_title("Startup"))
						.child(self.launch_at_login_card(cx))
						.child(
							ui_theme::settings_row()
								.px_0()
								.child(div().flex_1().child("Show in menu bar"))
								.child(self.toggle(false, cx)),
						),
				)
				.child(group().child(settings_group_title("Power")).child(self.power_control(cx)))
				.child(
					group()
						.child(settings_group_title("Notifications"))
						.child(self.notification_count_control(cx)),
				)
				.into_any_element(),
			SettingsCategory::Appearance => div()
				.flex()
				.flex_col()
				.gap(px(24.))
				.child(
					group().child(settings_group_title("Materials")).child(self.glass_controls(cx)),
				)
				.child(
					group()
						.child(settings_group_title("Panel layout"))
						.child(self.panel_controls(cx)),
				)
				.child(
					group()
						.child(settings_group_title("Text cursor"))
						.child(self.cursor_controls(cx)),
				)
				.child(quote_attribution())
				.into_any_element(),
		}
	}
}
fn settings_group_title(label: &'static str) -> impl IntoElement {
	div()
		.text_size(px(11.))
		.font_weight(gpui::FontWeight::MEDIUM)
		.text_color(rgb(TEXT_MUTED))
		.mb(px(4.))
		.child(label)
}
impl Render for SettingsSurface {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		#[cfg(not(test))]
		if self.category == SettingsCategory::General {
			self.refresh_power(cx);
		}
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
					.id(("settings-scroll-viewport", self.category as usize))
					.debug_selector(|| "settings-scroll-viewport".into())
					.size_full()
					.overflow_y_scroll()
					.px(px(ui_theme::SETTINGS_INSET))
					.pt(px(ui_theme::SETTINGS_TOP))
					.pb(px(ui_theme::SETTINGS_INSET))
					.flex()
					.justify_center()
					.items_start()
					.child(
						div()
							.w_full()
							.max_w(px(ui_theme::SETTINGS_WIDTH))
							.flex_none()
							.flex()
							.flex_col()
							.gap(px(24.))
							.child(ui_theme::settings_title(self.category.title()))
							.child(self.category_content(cx)),
					),
			)
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
		DesktopSettingsLoadState::Loading => "Loading your preferences.",
		DesktopSettingsLoadState::Ready => match snapshot.command {
			DesktopSettingsCommandState::Sending | DesktopSettingsCommandState::AwaitingResult =>
				"Saving preference…",
			DesktopSettingsCommandState::OutcomeUnknown =>
				"Reading back preferences after an uncertain response.",
			DesktopSettingsCommandState::Refused =>
				"The Decodex service refused the preference change.",
			DesktopSettingsCommandState::Idle | DesktopSettingsCommandState::Accepted =>
				"Your preferences are saved.",
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
		.debug_selector(|| "quote-source".into())
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
	fn short_settings_window_scrolls_to_last_row(cx: &mut TestAppContext) {
		struct ShortSettings(gpui::Entity<SettingsSurface>);
		impl Render for ShortSettings {
			fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
				div().w(px(800.)).h(px(300.)).overflow_hidden().child(self.0.clone())
			}
		}
		let (_, visual) = cx.add_window_view(|_, cx| {
			ShortSettings(
				cx.new(|cx| SettingsSurface::new(DesktopSettingsController::production(), cx)),
			)
		});
		visual.update(|window, cx| window.draw(cx).clear());
		let viewport = visual.debug_bounds("settings-scroll-viewport").unwrap();
		let before = visual.debug_bounds("notification-count-preference").unwrap();
		assert!(before.bottom() > viewport.bottom(), "before={before:?}, viewport={viewport:?}");
		visual.simulate_event(gpui::ScrollWheelEvent {
			position: viewport.center(),
			delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-1000.))),
			..Default::default()
		});
		visual.update(|window, cx| window.draw(cx).clear());
		let after = visual.debug_bounds("notification-count-preference").unwrap();
		assert!(after.top() < before.top(), "wheel must move the content");
		assert!(after.bottom() <= viewport.bottom(), "last setting must be reachable");
	}

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
		assert!(visual.debug_bounds("menubar-runtime-status").is_none());
		visual.update(|_, cx| {
			assert!(
				settings
					.read(cx)
					.notifications()
					.iter()
					.any(|(_, detail)| detail == "The service refused the change.")
			);
		});
		assert_eq!(visual.debug_bounds("launch-at-login-toggle"), Some(original));
	}

	#[gpui::test]
	fn glass_style_buttons_update_the_active_material(cx: &mut TestAppContext) {
		use ui_theme::window_material::GlassStyle;
		let controller = DesktopSettingsController::production();
		let (_, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(controller, cx);
			settings.category = SettingsCategory::Appearance;
			settings
		});
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
