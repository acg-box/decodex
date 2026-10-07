//! Product-level settings for the sole Decodex desktop application.
//!
//! Persistent settings remain daemon-owned. This presentation controls the restored native
//! Swift menu-bar panel only after it applies an authoritative protocol readback.

#[path = "settings_power.rs"] mod power;

use gpui::{
	AnyElement, ClickEvent, Context, FontWeight, KeyDownEvent, Render, Role, SharedString, Window,
	accesskit::Toggled,
	prelude::{
		FluentBuilder, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
		Styled,
	},
};
use ui_theme::window_material::GlassStyle;

use crate::{
	composer_input::cursor::{Preference, Shape},
	desktop_settings::{
		DesktopSettingsCommandState, DesktopSettingsController, DesktopSettingsInputError,
		DesktopSettingsLoadState, DesktopSettingsSnapshot,
	},
	native_menu_bar::{LaunchAtLoginState, NativeMenuBarHost},
	panel_preferences::PanelDefaults,
	ui_loading,
	ui_motion::{self, SmoothControl},
	ui_scroll::SmoothScrollArea,
	ui_theme::{
		self, BODY_LINE_HEIGHT, BODY_SIZE, HOVER_FILL, LINE_STRONG, SETTINGS_GROUP_GAP,
		SETTINGS_INSET, SETTINGS_TOP, SETTINGS_WIDTH,
	},
};
use power::PowerSettings;

const LINE: u32 = LINE_STRONG;
const TEXT: u32 = ui_theme::TEXT;
const TEXT_MUTED: u32 = ui_theme::TEXT_MUTED;
const BLUE: u32 = ui_theme::BLUE;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuBarRuntimeState {
	Visible,
	Hidden,
	Waiting,
	Unavailable,
}

#[derive(Clone, Copy)]
enum DesktopPreference {
	MenuBar,
	Quota,
	Recap,
}

pub(crate) struct SettingsSurface {
	pub(crate) category: SettingsCategory,
	power: PowerSettings,
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
			detail: "Loading settings…".into(),
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
		self.refresh_launch_at_login();
		self.refresh_power(cx);
		self.synchronize(cx);
		cx.notify();
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

	fn toggle_menubar(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
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

	fn toggle_activation(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
		let Some(settings) = self.snapshot.settings else {
			return;
		};

		if let Err(error) = self.controller.set_auto_activate_quota(!settings.auto_activate_quota) {
			self.detail = input_error_detail(error).into();
		}

		self.snapshot = self.controller.snapshot();

		cx.notify();
	}

	fn toggle_recap(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
		let Some(settings) = self.snapshot.settings else { return };

		if let Err(error) = self.controller.set_auto_recap(!settings.auto_recap) {
			self.detail = input_error_detail(error).into();
		}

		self.snapshot = self.controller.snapshot();

		cx.notify();
	}

	fn toggle_launch_at_login(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
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
		_: &ClickEvent,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		if let Err(failure) = self.menu_bar.open_login_items_settings() {
			self.launch_at_login_detail = failure.detail().into();
		}

		cx.notify();
	}

	fn toggle(&self, preference: DesktopPreference, cx: &mut Context<Self>) -> AnyElement {
		if self.snapshot.settings.is_none() {
			return gpui::div()
				.id(match preference {
					DesktopPreference::MenuBar => "menubar-loading",
					DesktopPreference::Quota => "quota-loading",
					DesktopPreference::Recap => "recap-loading",
				})
				.w(gpui::px(28.))
				.flex()
				.justify_center()
				.child(
					if matches!(
						self.snapshot.load,
						DesktopSettingsLoadState::NeverRequested
							| DesktopSettingsLoadState::Loading
					) {
						ui_loading::loading("").into_any_element()
					} else {
						gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child("—").into_any_element()
					},
				)
				.into_any_element();
		}

		let enabled = self.snapshot.settings.is_some_and(|settings| match preference {
			DesktopPreference::MenuBar => settings.show_in_menu_bar,
			DesktopPreference::Quota => settings.auto_activate_quota,
			DesktopPreference::Recap => settings.auto_recap,
		});
		let (id, label, knob) = match preference {
			DesktopPreference::MenuBar =>
				("menubar-surface-toggle", "Show Decodex in the menu bar", "settings-knob"),
			DesktopPreference::Quota => (
				"quota-activation-toggle",
				"Automatically activate weekly quota",
				"activation-knob",
			),
			DesktopPreference::Recap =>
				("automatic-recap-toggle", "Automatically recap tasks", "recap-knob"),
		};
		let interactive = self.snapshot.can_toggle;

		gpui::div()
			.id(id)
			.debug_selector(move || id.into())
			.role(Role::Switch)
			.aria_label(label)
			.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
			.w(gpui::px(36.0))
			.h(gpui::px(20.0))
			.p(gpui::px(2.0))
			.flex()
			.items_center()
			.rounded_full()
			.border_1()
			.border_color(gpui::rgb(if enabled { BLUE } else { LINE }))
			.bg(if enabled { gpui::rgba(0x8baaf730) } else { gpui::rgba(0xffffff0c) })
			.opacity(if interactive { 1.0 } else { 0.58 })
			.when(interactive, |toggle| {
				toggle
					.cursor_pointer()
					.hover(move |element| {
						element.border_color(gpui::rgb(if enabled { BLUE } else { TEXT_MUTED }))
					})
					.active(|element| element.opacity(0.9))
					.focus_visible(|element| element.border_color(gpui::rgb(BLUE)))
					.on_click(cx.listener(match preference {
						DesktopPreference::Quota => Self::toggle_activation,
						DesktopPreference::MenuBar => Self::toggle_menubar,
						DesktopPreference::Recap => Self::toggle_recap,
					}))
			})
			.child(ui_motion::switch_knob(
				knob,
				enabled,
				gpui::div().size(gpui::px(14.0)).rounded_full().bg(gpui::rgb(if enabled {
					BLUE
				} else {
					TEXT_MUTED
				})),
			))
			.smooth()
			.enabled(interactive)
			.into_any_element()
	}

	fn launch_at_login_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let enabled = self.launch_at_login.is_requested();
		let interactive = !matches!(
			self.launch_at_login,
			LaunchAtLoginState::NotFound | LaunchAtLoginState::OperationFailed
		);

		gpui::div()
			.id("launch-at-login-toggle")
			.debug_selector(|| "launch-at-login-toggle".into())
			.role(Role::Switch)
			.aria_label("Launch Decodex at login")
			.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
			.w(gpui::px(36.0))
			.h(gpui::px(20.0))
			.p(gpui::px(2.0))
			.flex()
			.items_center()
			.rounded_full()
			.border_1()
			.border_color(gpui::rgb(if enabled { BLUE } else { LINE }))
			.bg(if enabled { gpui::rgba(0x8baaf730) } else { gpui::rgba(0xffffff0c) })
			.opacity(if interactive { 1.0 } else { 0.58 })
			.when(interactive, |toggle| {
				toggle
					.cursor_pointer()
					.hover(move |element| {
						element.border_color(gpui::rgb(if enabled { BLUE } else { TEXT_MUTED }))
					})
					.active(|element| element.opacity(0.9))
					.focus_visible(|element| element.border_color(gpui::rgb(BLUE)))
					.on_click(cx.listener(Self::toggle_launch_at_login))
			})
			.child(ui_motion::switch_knob(
				"login-knob",
				enabled,
				gpui::div().size(gpui::px(14.0)).rounded_full().bg(gpui::rgb(if enabled {
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
			.child(gpui::div().flex_1().child("Launch at login"))
			.child(self.launch_at_login_toggle(cx))
	}
}

impl SettingsSurface {
	fn preference_control(
		&self,
		id: &'static str,
		knob: &'static str,
		label: &'static str,
		preference: fn(Option<bool>) -> bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let enabled = preference(None);

		ui_theme::settings_row().px_0().child(gpui::div().flex_1().child(label)).child(
			gpui::div()
				.id(id)
				.debug_selector(move || id.into())
				.role(Role::Switch)
				.aria_label(label)
				.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
				.tab_index(0)
				.w(gpui::px(36.))
				.h(gpui::px(20.))
				.p(gpui::px(2.))
				.flex()
				.items_center()
				.rounded_full()
				.border_1()
				.border_color(gpui::rgb(if enabled { BLUE } else { LINE }))
				.bg(if enabled { gpui::rgba(0x8baaf730) } else { gpui::rgba(0xffffff0c) })
				.cursor_pointer()
				.hover(move |d| d.border_color(gpui::rgb(if enabled { BLUE } else { TEXT_MUTED })))
				.focus_visible(|d| d.border_color(gpui::rgb(BLUE)))
				.on_click(cx.listener(move |_, _, _, cx| {
					preference(Some(!enabled));

					cx.refresh_windows();
				}))
				.on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
					if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
						preference(Some(!enabled));

						cx.refresh_windows();
						cx.stop_propagation();
					}
				}))
				.child(ui_motion::switch_knob(
					knob,
					enabled,
					gpui::div().size(gpui::px(14.)).rounded_full().bg(gpui::rgb(if enabled {
						BLUE
					} else {
						TEXT_MUTED
					})),
				))
				.smooth(),
		)
	}

	fn panel_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let current = PanelDefaults::configured();

		gpui::div().flex().flex_col().children(
			[
				(true, "Default sidebar width", current.sidebar),
				(false, "Default dock height", current.dock),
			]
			.into_iter()
			.map(|(sidebar, title, value)| {
				ui_theme::settings_row().px_0().child(gpui::div().flex_1().child(title)).child(
					gpui::div()
						.flex()
						.items_center()
						.gap_2()
						.child(gpui::div().text_size(gpui::px(12.)).child(format!("{value} px")))
						.children(
							[(-24_i32, "−"), (24, "+")]
								.into_iter()
								.map(|(delta, label)| {
									gpui::div()
										.id(SharedString::from(format!(
											"panel-default-{sidebar}-{delta}"
										)))
										.role(Role::Button)
										.aria_label(format!(
											"{} {title}",
											if delta < 0 { "Decrease" } else { "Increase" }
										))
										.tab_index(0)
										.size(gpui::px(26.))
										.flex()
										.items_center()
										.justify_center()
										.rounded(gpui::px(7.))
										.cursor_pointer()
										.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
										.on_click(cx.listener(move |_, _, _, cx| {
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
										}))
										.on_key_down(cx.listener(
											move |_, event: &KeyDownEvent, _, cx| {
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

		gpui::div().flex().flex_col().children(choices.into_iter().map(|(title, values)| {
			ui_theme::settings_row().px_0().child(gpui::div().flex_1().child(title)).child(
				gpui::div()
					.flex()
					.gap_1()
					.p_1()
					.rounded(gpui::px(9.))
					.bg(gpui::rgba(0xffffff08))
					.children(values.into_iter().map(|(label, value)| {
						gpui::div()
							.id(SharedString::from(format!("{title}-{label}")))
							.role(Role::Button)
							.aria_label(format!("{title}: {label}"))
							.aria_toggled(if value == current {
								Toggled::True
							} else {
								Toggled::False
							})
							.tab_index(0)
							.px_3()
							.h(gpui::px(26.))
							.flex()
							.items_center()
							.rounded(gpui::px(6.))
							.text_size(gpui::px(11.))
							.cursor_pointer()
							.when(value == current, |d| d.bg(gpui::rgba(0xffffff16)))
							.hover(|d| d.bg(gpui::rgba(HOVER_FILL)))
							.on_click(cx.listener(move |_, _, _, cx| {
								value.select(cx);
								cx.notify();
							}))
							.on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
								if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
									value.select(cx);
									cx.notify();
									cx.stop_propagation();
								}
							}))
							.child(label)
							.smooth()
					})),
			)
		}))
	}

	fn glass_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let style = GlassStyle::configured();

		ui_theme::settings_row().px_0().child(gpui::div().flex_1().child("Glass appearance")).child(
			gpui::div()
				.flex()
				.gap_1()
				.p_1()
				.rounded(gpui::px(9.))
				.bg(gpui::rgba(0xffffff08))
				.children(
					[(GlassStyle::Regular, "Regular"), (GlassStyle::Clear, "Clear")]
						.into_iter()
						.map(|(value, label)| {
							gpui::div()
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
								.h(gpui::px(26.))
								.flex()
								.items_center()
								.rounded(gpui::px(6.))
								.text_size(gpui::px(11.))
								.cursor_pointer()
								.when(value == style, |d| d.bg(gpui::rgba(0xffffff16)))
								.hover(|d| d.bg(gpui::rgba(HOVER_FILL)))
								.on_click(cx.listener(move |_, _, _, cx| {
									value.select(cx);
									cx.notify();
								}))
								.on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										value.select(cx);
										cx.notify();
										cx.stop_propagation();
									}
								}))
								.child(label)
								.smooth()
						}),
				),
		)
	}
}

impl SettingsSurface {
	pub(crate) fn quota_control(&self, cx: &mut Context<Self>) -> AnyElement {
		gpui::div()
			.flex_none()
			.flex()
			.flex_col()
			.gap(gpui::px(3.))
			.child(
				ui_theme::settings_row()
					.px_0()
					.child(gpui::div().flex_1().child("Auto-activate weekly quota"))
					.child(self.toggle(DesktopPreference::Quota, cx)),
			)
			.child(gpui::div().text_size(gpui::px(11.)).text_color(gpui::rgb(TEXT_MUTED)).child(
				"Start the next weekly window with a small request. Uses quota; no chat is saved.",
			))
			.into_any_element()
	}

	fn category_content(&self, cx: &mut Context<Self>) -> AnyElement {
		let group = || gpui::div().flex().flex_col().gap(gpui::px(2.));

		match self.category {
			SettingsCategory::General => gpui::div()
				.flex()
				.flex_col()
				.gap(gpui::px(24.))
				.child(
					group()
						.child(settings_group_title("Startup"))
						.child(self.launch_at_login_card(cx))
						.child(
							ui_theme::settings_row()
								.px_0()
								.child(gpui::div().flex_1().child("Show in menu bar"))
								.child(self.toggle(DesktopPreference::MenuBar, cx)),
						),
				)
				.child(group().child(settings_group_title("Task recaps"))
						.child(ui_theme::settings_row().px_0().child(gpui::div().flex_1().child("Automatic task recaps")).child(self.toggle(DesktopPreference::Recap, cx)))
						.child(gpui::div().text_size(gpui::px(11.)).text_color(gpui::rgb(TEXT_MUTED)).child("After 30 minutes away, recap the selected task when it has new completed work. Uses its model and quota."))
				)
				.child(group().child(settings_group_title("Power")).child(self.power_control(cx)))
				.child(
					group()
						.child(settings_group_title("Notifications"))
						.child(self.preference_control(
							"notification-count-preference",
							"notification-count-knob",
							"Show notification count",
							crate::shell::notification_count_preference,
							cx,
						))
						.child(self.preference_control(
							"question-notice-preference",
							"question-notice-knob",
							"Show new question notices",
							crate::shell::question_notice_preference,
							cx,
						)),
				)
				.into_any_element(),
			SettingsCategory::Appearance => gpui::div()
				.flex()
				.flex_col()
				.gap(gpui::px(24.))
				.child(
					group().child(settings_group_title("Materials")).child(self.glass_controls(cx)),
				)
				.child(
					group()
						.child(settings_group_title("Panel layout"))
						.child(self.panel_controls(cx)),
				)
				.child(
					group().child(settings_group_title("Scrolling")).child(self.preference_control(
						"smooth-scrolling", "smooth-scrolling-knob", "Smooth scrolling",
						crate::ui_scroll::preference, cx,
					)),
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

impl Render for SettingsSurface {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		#[cfg(not(test))]
		if self.category == SettingsCategory::General {
			self.refresh_power(cx);
		}

		gpui::div()
			.id("settings-surface")
			.role(Role::Main)
			.aria_label("Decodex settings")
			.size_full()
			.min_w_0()
			.min_h_0()
			.text_size(gpui::px(BODY_SIZE))
			.line_height(gpui::px(BODY_LINE_HEIGHT))
			.text_color(gpui::rgb(TEXT))
			.child(
				gpui::div()
					.id(("settings-scroll-viewport", self.category as usize))
					.debug_selector(|| "settings-scroll-viewport".into())
					.size_full()
					.overflow_y_scroll()
					.px(gpui::px(SETTINGS_INSET))
					.pt(gpui::px(SETTINGS_TOP))
					.pb(gpui::px(SETTINGS_INSET))
					.flex()
					.justify_center()
					.items_start()
					.child(
						gpui::div()
							.w_full()
							.max_w(gpui::px(SETTINGS_WIDTH))
							.flex_none()
							.flex()
							.flex_col()
							.gap(gpui::px(SETTINGS_GROUP_GAP))
							.child(ui_theme::settings_title(self.category.title()))
							.child(self.category_content(cx)),
					)
					.smooth_scroll(("settings-smooth-scroll", self.category as usize)),
			)
	}
}

fn settings_group_title(label: &'static str) -> impl IntoElement {
	gpui::div()
		.text_size(gpui::px(11.))
		.font_weight(FontWeight::MEDIUM)
		.text_color(gpui::rgb(TEXT_MUTED))
		.mb(gpui::px(4.))
		.child(label)
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
		DesktopSettingsLoadState::NeverRequested => "Settings have not loaded yet.",
		DesktopSettingsLoadState::Loading => "Loading settings…",
		DesktopSettingsLoadState::Ready => match snapshot.command {
			DesktopSettingsCommandState::Sending | DesktopSettingsCommandState::AwaitingResult =>
				"Saving settings…",
			DesktopSettingsCommandState::OutcomeUnknown =>
				"Checking whether your changes were saved…",
			DesktopSettingsCommandState::Refused => "This setting could not be changed.",
			DesktopSettingsCommandState::Idle | DesktopSettingsCommandState::Accepted =>
				"Settings saved.",
		},
		DesktopSettingsLoadState::Offline =>
			"Connect to the Decodex service to read desktop settings.",
		DesktopSettingsLoadState::Unavailable => "Settings are temporarily unavailable.",
		DesktopSettingsLoadState::Refused => "The desktop settings response was invalid.",
	}
}

const fn input_error_detail(error: DesktopSettingsInputError) -> &'static str {
	match error {
		DesktopSettingsInputError::Offline =>
			"Connect to the Decodex service before changing this setting.",
		DesktopSettingsInputError::Busy => "Wait for the current settings request to finish.",
		DesktopSettingsInputError::NotLoaded => "Wait for settings to load.",
		DesktopSettingsInputError::IdentityUnavailable => "Could not save this setting. Try again.",
	}
}

fn quote_attribution() -> impl IntoElement {
	gpui::div()
		.id("quote-source")
		.debug_selector(|| "quote-source".into())
		.role(Role::Link)
		.tab_index(0)
		.aria_label("Quotes from ZenQuotes. Visit the website.")
		.text_size(gpui::px(11.))
		.text_color(gpui::rgb(TEXT_MUTED))
		.cursor_pointer()
		.hover(|d| d.text_color(gpui::rgb(TEXT)))
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
	use gpui::{self, AppContext as _, TestAppContext};
	use ui_theme::window_material::GlassStyle;

	use crate::settings_surface::{
		self, Context, DesktopSettingsCommandState, DesktopSettingsController,
		DesktopSettingsLoadState, IntoElement, LaunchAtLoginState, MenuBarRuntimeState,
		ParentElement, Render, SettingsCategory, SettingsSurface, Styled, Window, ui_theme,
	};

	#[gpui::test]
	fn refresh_reads_external_login_item_changes(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});

		surface.update(visual, |s, cx| {
			// Keep this test on the simulated login bridge, with no host power query.
			s.power.pending = true;

			for (enabled, expected) in
				[(true, LaunchAtLoginState::Enabled), (false, LaunchAtLoginState::NotRegistered)]
			{
				s.menu_bar.set_launch_at_login(enabled).expect("simulate external system change");
				s.refresh(cx);

				assert_eq!(s.launch_at_login, expected);
				assert_eq!(
					s.launch_at_login_detail.as_ref(),
					settings_surface::launch_at_login_detail(expected)
				);
				assert!(!s.notifications().iter().any(|(title, _)| *title == "Launch at login"));
			}
		});
	}

	#[gpui::test]
	fn unloaded_preferences_do_not_look_disabled(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});

		surface.update(visual, |s, cx| {
			s.snapshot.settings = None;
			s.snapshot.load = DesktopSettingsLoadState::Loading;

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear());

		assert!(visual.debug_bounds("menubar-surface-toggle").is_none());
		assert!(visual.debug_bounds("automatic-recap-toggle").is_none());
		assert!(visual.debug_bounds("loading-feedback-").is_some());
	}

	#[gpui::test]
	fn short_settings_window_scrolls_to_last_row(cx: &mut TestAppContext) {
		struct ShortSettings(gpui::Entity<SettingsSurface>);

		impl Render for ShortSettings {
			fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
				gpui::div()
					.w(gpui::px(800.))
					.h(gpui::px(300.))
					.overflow_hidden()
					.child(self.0.clone())
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
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-1_000.))),
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
			window.resize(gpui::size(gpui::px(800.), gpui::px(700.)));
			window.draw(cx).clear();
		});

		let original = visual.debug_bounds("launch-at-login-toggle").expect("login toggle");

		for command in
			[DesktopSettingsCommandState::Sending, DesktopSettingsCommandState::AwaitingResult]
		{
			settings.update(visual, |s, cx| {
				s.snapshot.command = command;
				s.runtime = MenuBarRuntimeState::Waiting;
				s.detail = "Saving settings…".into();

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
		let controller = DesktopSettingsController::production();
		let (_, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(controller, cx);

			settings.category = SettingsCategory::Appearance;

			settings
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(800.), gpui::px(600.)));
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
			window.resize(gpui::size(gpui::px(1_490.0), gpui::px(1_055.0)));
			window.draw(cx).clear();
		});
	}
}
