//! Product-level settings for the sole Decodex desktop application.
//!
//! Service behavior remains daemon-owned. Menu-bar visibility and automatic recaps
//! run in the desktop and must not wait for service startup or queries.

#[path = "settings_power.rs"] mod power;
#[path = "ui_switch.rs"] mod switch;
#[path = "settings_updates.rs"] mod updates;

use gpui::{
	AnyElement, ClickEvent, Context, FontWeight, KeyDownEvent, Render, Role, SharedString, Window,
	accesskit::Toggled,
	prelude::{
		FluentBuilder, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
		Styled,
	},
};
use ui_theme::{
	appearance::Appearance,
	material::{GlassStyle, Material, Surface},
};

use crate::{
	composer_input::cursor::{Preference, Shape},
	desktop_settings::{
		DesktopSettingsCommandState, DesktopSettingsController, DesktopSettingsInputError,
		DesktopSettingsLoadState, DesktopSettingsSnapshot,
	},
	native_menu_bar::{LaunchAtLoginState, NativeMenuBarHost},
	panel_preferences::PanelDefaults,
	ui_loading,
	ui_scroll::SmoothScrollArea,
	ui_theme::{
		self, BODY_LINE_HEIGHT, BODY_SIZE, SETTINGS_GROUP_GAP, SETTINGS_INSET, SETTINGS_TOP,
		SETTINGS_WIDTH,
	},
};
use power::PowerSettings;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SettingsCategory {
	#[default]
	General,
	Appearance,
	Updates,
}
impl SettingsCategory {
	pub(crate) fn title(self) -> &'static str {
		match self {
			Self::General => "General",
			Self::Appearance => "Appearance",
			Self::Updates => "Updates",
		}
	}

	fn description(self) -> &'static str {
		match self {
			Self::General => "Choose how Decodex starts, notifies you, and manages activity.",
			Self::Appearance => "Customize your workspace colors, materials, and reading comfort.",
			Self::Updates => "Keep Decodex and its Codex component up to date.",
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

const MENU_BAR_PREFERENCE: &str = "DecodexMenuBarVisible";
const RECAP_PREFERENCE: &str = "DecodexAutomaticTaskRecaps";

pub(crate) struct SettingsSurface {
	updates: updates::Updates,
	pub(crate) category: SettingsCategory,
	power: PowerSettings,
	snapshot: DesktopSettingsSnapshot,
	runtime: MenuBarRuntimeState,
	detail: SharedString,
	menu_bar_choice: Option<bool>,
	recap_choice: Option<bool>,
	preference_error: Option<SharedString>,
	controller: DesktopSettingsController,
	menu_bar: NativeMenuBarHost,
	launch_at_login: LaunchAtLoginState,
	launch_at_login_detail: SharedString,
}
impl SettingsSurface {
	pub(crate) fn new(controller: DesktopSettingsController, cx: &mut Context<Self>) -> Self {
		let snapshot = controller.snapshot();
		let menu_bar_choice =
			crate::ui_preferences::string(MENU_BAR_PREFERENCE, None).and_then(|value| match value
				.as_str()
			{
				"shown" => Some(true),
				"hidden" => Some(false),
				_ => None,
			});
		let recap_choice =
			crate::ui_preferences::string(RECAP_PREFERENCE, None).and_then(|value| {
				match value.as_str() {
					"enabled" => Some(true),
					"disabled" => Some(false),
					_ => None,
				}
			});
		let mut menu_bar = NativeMenuBarHost::new();
		let launch_at_login =
			menu_bar.launch_at_login_state().unwrap_or(LaunchAtLoginState::OperationFailed);
		let mut surface = Self {
			updates: updates::Updates::new(),
			power: Default::default(),
			category: SettingsCategory::General,
			snapshot,
			runtime: MenuBarRuntimeState::Waiting,
			detail: "Loading settings…".into(),
			menu_bar_choice,
			recap_choice,
			preference_error: None,
			controller,
			menu_bar,
			launch_at_login,
			launch_at_login_detail: launch_at_login_detail(launch_at_login).into(),
		};

		surface.apply_snapshot(snapshot);
		// Read the real system policy before Settings opens, never from a render pass.
		#[cfg(not(test))]
		surface.refresh_power(cx);
		#[cfg(test)]
		let _ = cx;

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
			self.preference_error = None;

			self.apply_snapshot(snapshot);
			cx.notify();
		}
	}

	pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
		if self.category == SettingsCategory::General {
			self.refresh_launch_at_login();
			self.refresh_power(cx);
		}
		self.synchronize(cx);
		cx.notify();
	}

	fn apply_snapshot(&mut self, snapshot: DesktopSettingsSnapshot) {
		// Import the old service preference once. An explicit local choice always wins,
		// including a click made while the first service read is still pending.
		if snapshot.load == DesktopSettingsLoadState::Ready
			&& let Some(settings) = snapshot.settings
		{
			if self.menu_bar_choice.is_none() {
				self.save_menu_bar_choice(settings.show_in_menu_bar);
			}
			if self.recap_choice.is_none() {
				self.save_recap_choice(settings.auto_recap);
			}
		}
		self.apply_menu_bar(self.menu_bar_choice.unwrap_or(true));
	}

	fn save_menu_bar_choice(&mut self, enabled: bool) {
		crate::ui_preferences::string(
			MENU_BAR_PREFERENCE,
			Some(if enabled { "shown" } else { "hidden" }),
		);
		self.menu_bar_choice = Some(enabled);
	}

	pub(crate) fn automatic_recaps_enabled(&self) -> bool {
		self.recap_choice.unwrap_or(false)
	}

	fn save_recap_choice(&mut self, enabled: bool) {
		crate::ui_preferences::string(
			RECAP_PREFERENCE,
			Some(if enabled { "enabled" } else { "disabled" }),
		);
		self.recap_choice = Some(enabled);
	}

	fn apply_menu_bar(&mut self, enabled: bool) {
		let target =
			if enabled { MenuBarRuntimeState::Visible } else { MenuBarRuntimeState::Hidden };
		if self.runtime == target {
			return;
		}
		match self.menu_bar.apply(enabled) {
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
			},
			Err(failure) => {
				self.runtime = MenuBarRuntimeState::Unavailable;
				self.detail = failure.detail().into();
			},
		}
	}

	fn menubar_needs_attention(&self) -> bool {
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
		if let Some(error) = &self.preference_error {
			notices.push(("Settings", error.to_string()));
		} else if matches!(
			self.snapshot.load,
			DesktopSettingsLoadState::Offline
				| DesktopSettingsLoadState::Unavailable
				| DesktopSettingsLoadState::Refused
		) || matches!(
			self.snapshot.command,
			DesktopSettingsCommandState::Refused | DesktopSettingsCommandState::OutcomeUnknown
		) {
			notices.push(("Settings", settings_detail(self.snapshot).into()));
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

	fn toggle_quota(&mut self, cx: &mut Context<Self>) {
		let Some(settings) = self.snapshot.settings else { return };
		let result = self.controller.set_auto_activate_quota(!settings.auto_activate_quota);
		if let Err(error) = result {
			self.preference_error = Some(input_error_detail(error).into());
		}
		self.snapshot = self.controller.snapshot();
		cx.notify();
	}

	fn menu_bar_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		switch::settings_switch(
			"menubar-surface-toggle",
			"settings-knob",
			"Show Decodex in the menu bar",
			self.menu_bar_choice.unwrap_or(true),
			true,
		)
		.on_change(cx.listener(|s, enabled: &bool, _, cx| {
			s.apply_menu_bar(*enabled);
			if matches!(s.runtime, MenuBarRuntimeState::Visible | MenuBarRuntimeState::Hidden) {
				s.save_menu_bar_choice(*enabled);
			}
			cx.notify();
		}))
	}

	fn recap_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		switch::settings_switch(
			"automatic-recap-toggle",
			"recap-knob",
			"Automatically recap tasks",
			self.automatic_recaps_enabled(),
			true,
		)
		.on_change(cx.listener(|s, enabled: &bool, _, cx| {
			s.save_recap_choice(*enabled);
			cx.notify();
		}))
	}

	fn toggle_launch_at_login(&mut self, cx: &mut Context<Self>) {
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

	fn quota_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
		if self.snapshot.settings.is_none() {
			let loading = matches!(
				self.snapshot.load,
				DesktopSettingsLoadState::NeverRequested | DesktopSettingsLoadState::Loading
			);
			return gpui::div()
				.id("quota-loading")
				.flex_none()
				.flex()
				.items_center()
				.gap_2()
				.text_size(gpui::px(11.))
				.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
				.when(loading, |d| d.child(ui_loading::loading("")))
				.child(if loading { "Loading…" } else { "Unavailable" })
				.into_any_element();
		}
		let enabled = self.snapshot.settings.is_some_and(|s| s.auto_activate_quota);
		switch::settings_switch(
			"quota-activation-toggle",
			"activation-knob",
			"Automatically activate weekly quota",
			enabled,
			self.snapshot.can_toggle,
		)
		.on_change(cx.listener(|s, _, _, cx| s.toggle_quota(cx)))
		.into_any_element()
	}

	fn launch_at_login_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let interactive = !matches!(
			self.launch_at_login,
			LaunchAtLoginState::NotFound | LaunchAtLoginState::OperationFailed
		);
		switch::settings_switch(
			"launch-at-login-toggle",
			"login-knob",
			"Launch Decodex at login",
			self.launch_at_login.is_requested(),
			interactive,
		)
		.on_change(cx.listener(|s, _, _, cx| s.toggle_launch_at_login(cx)))
	}

	fn launch_at_login_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let description = if matches!(
			self.launch_at_login,
			LaunchAtLoginState::NotRegistered | LaunchAtLoginState::Enabled
		) {
			SharedString::from("Open Decodex quietly when you sign in to your Mac.")
		} else {
			self.launch_at_login_detail.clone()
		};
		ui_theme::settings_row()
			.child(ui_theme::settings_label("Launch at login", description))
			.child(self.launch_at_login_toggle(cx))
	}
}

impl SettingsSurface {
	fn theme_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let current = ui_theme::ColorTheme::configured();
		gpui::div()
			.id("color-theme-picker")
			.role(Role::RadioGroup)
			.aria_label("Color theme")
			.grid()
			.grid_cols(3)
			.gap(gpui::px(8.))
			.children(ui_theme::ColorTheme::ALL.into_iter().map(|theme| {
				let colors = theme.palette();
				let selected = current == theme;
				let id = format!("color-theme-{}", theme.id());
				gpui::div()
					.id(SharedString::from(id.clone()))
					.debug_selector(move || id.clone())
					.role(Role::RadioButton)
					.aria_label(format!("{} color theme", theme.name()))
					.aria_toggled(if selected { Toggled::True } else { Toggled::False })
					.tab_index(0)
					.min_w_0()
					.h(gpui::px(42.))
					.px(gpui::px(8.))
					.flex()
					.items_center()
					.gap(gpui::px(8.))
					.rounded(gpui::px(6.))
					.border_1()
					.border_color(ui_theme::rgba(if selected {
						ui_theme::SELECTED_BORDER
					} else {
						0xffffff16
					}))
					.bg(ui_theme::rgba(if selected { ui_theme::SELECTED_FILL } else { 0xffffff04 }))
					.text_size(gpui::px(11.5))
					.line_height(gpui::px(17.))
					.text_color(ui_theme::rgb(ui_theme::TEXT))
					.cursor_pointer()
					.hover(move |s| {
						s.border_color(ui_theme::rgba(if selected {
							(ui_theme::ACCENT << 8) | 0x90
						} else {
							0xffffff28
						}))
						.bg(ui_theme::rgba(if selected {
							ui_theme::SELECTED_HOVER_FILL
						} else {
							0xffffff09
						}))
					})
					.active(|s| s.bg(ui_theme::rgba(ui_theme::PRESSED_FILL)))
					.focus_visible(|s| s.border_color(ui_theme::rgb(ui_theme::ACCENT)))
					.child(
						gpui::div()
							.w(gpui::px(32.))
							.h(gpui::px(28.))
							.flex_none()
							.rounded(gpui::px(4.))
							.bg(gpui::rgb(colors.canvas))
							.p(gpui::px(6.))
							.flex()
							.flex_col()
							.justify_between()
							.children(
								[(18., colors.accent), (13., colors.blue), (16., colors.green)]
									.map(|(width, color)| {
										gpui::div()
											.w(gpui::px(width))
											.h(gpui::px(3.))
											.rounded_full()
											.bg(gpui::rgb(color))
									}),
							),
					)
					.child(gpui::div().flex_1().truncate().child(theme.name()))
					.child(
						gpui::div()
							.size(gpui::px(12.))
							.flex_none()
							.text_color(ui_theme::rgb(ui_theme::ACCENT))
							.when(selected, |s| {
								s.child(crate::shell::workspace_symbols::icon_sized(
									crate::shell::workspace_symbols::Symbol::Confirm,
									12.,
								))
							}),
					)
					.on_click(cx.listener(move |_, _, _, cx| theme.select(cx)))
					.on_key_down(cx.listener(move |_, e: &KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							theme.select(cx);
							cx.stop_propagation();
						}
					}))
			}))
	}

	fn preference_control(
		&self,
		id: &'static str,
		knob: &'static str,
		label: &'static str,
		description: &'static str,
		preference: fn(Option<bool>) -> bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let enabled = preference(None);
		ui_theme::settings_row().child(ui_theme::settings_label(label, description)).child(
			switch::settings_switch(id, knob, label, enabled, true).on_change(cx.listener(
				move |_, desired: &bool, _, cx| {
					preference(Some(*desired));
					cx.refresh_windows();
				},
			)),
		)
	}

	fn adjust_panel(&mut self, sidebar: bool, delta: i32, cx: &mut Context<Self>) {
		let mut value = PanelDefaults::configured();
		if sidebar {
			value.sidebar = (i32::from(value.sidebar) + delta).clamp(160, 480) as u16;
		} else {
			value.dock = (i32::from(value.dock) + delta).clamp(120, 480) as u16;
		}
		value.select(cx);
	}

	fn panel_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let current = PanelDefaults::configured();
		gpui::div()
			.flex()
			.flex_col()
			.children(
				[(true, "Sidebar width", current.sidebar), (false, "Dock height", current.dock)]
					.into_iter()
					.map(|(sidebar, title, value)| {
						ui_theme::settings_row()
							.child(ui_theme::settings_label(
								title,
								"Default size when a panel opens.",
							))
							.child(
								gpui::div()
									.flex()
									.items_center()
									.gap_3()
									.child(
										gpui::div()
											.w(gpui::px(54.))
											.text_size(gpui::px(12.))
											.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
											.child(format!("{value} px")),
									)
									.child(
										ui_theme::settings_segments(title)
											.children([(-24_i32, "−"), (24, "+")].into_iter().map(
												|(delta, label)| {
													ui_theme::settings_button(
														SharedString::from(format!(
															"panel-default-{sidebar}-{delta}"
														)),
														label,
													)
													.w(gpui::px(ui_theme::SETTINGS_CONTROL_HEIGHT))
													.aria_label(format!(
														"{} {title}",
														if delta < 0 {
															"Decrease"
														} else {
															"Increase"
														}
													))
													.on_click(cx.listener(move |s, _, _, cx| {
														s.adjust_panel(sidebar, delta, cx)
													}))
													.on_key_down(cx.listener(
														move |s, e: &KeyDownEvent, _, cx| {
															if !e.is_held
																&& ["enter", "space"].contains(
																	&e.keystroke.key.as_str(),
																) {
																s.adjust_panel(sidebar, delta, cx);
																cx.stop_propagation();
															}
														},
													))
												},
											))
											.role(Role::Group),
									),
							)
					}),
			)
			.child(
				ui_theme::settings_row()
					.min_h(gpui::px(44.))
					.child(
						gpui::div()
							.flex_1()
							.text_size(gpui::px(11.5))
							.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
							.child("You can also resize panels by dragging their edges."),
					)
					.child(
						ui_theme::settings_button("reset-panel-sizes", "Reset sizes")
							.on_click(cx.listener(|_, _, _, cx| {
								PanelDefaults { sidebar: 240, dock: 240 }.select(cx)
							}))
							.on_key_down(cx.listener(|_, e: &KeyDownEvent, _, cx| {
								if !e.is_held
									&& ["enter", "space"].contains(&e.keystroke.key.as_str())
								{
									PanelDefaults { sidebar: 240, dock: 240 }.select(cx);
									cx.stop_propagation();
								}
							})),
					),
			)
	}

	fn cursor_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let current = Preference::configured();
		gpui::div()
			.flex()
			.flex_col()
			.child(
				ui_theme::settings_row()
					.child(ui_theme::settings_label(
						"Cursor style",
						"Choose how the text insertion point looks.",
					))
					.child(
						ui_theme::settings_segments("Cursor style").children(
							[
								("Bar", Shape::Bar),
								("Block", Shape::Block),
								("Underline", Shape::Underline),
							]
							.into_iter()
							.map(|(label, shape)| {
								let value = Preference { shape, ..current };
								ui_theme::settings_segment(
									SharedString::from(format!("Cursor style-{label}")),
									label,
									current.shape == shape,
								)
								.aria_label(format!("Cursor style: {label}"))
								.on_click(cx.listener(move |_, _, _, cx| value.select(cx)))
								.on_key_down(cx.listener(
									move |_, e: &KeyDownEvent, _, cx| {
										if !e.is_held
											&& ["enter", "space"]
												.contains(&e.keystroke.key.as_str())
										{
											value.select(cx);
											cx.stop_propagation();
										}
									},
								))
							}),
						),
					),
			)
			.children(
				[
					(
						"cursor-blinking",
						"Blinking cursor",
						"Blink while the insertion point is idle.",
						current.blinking,
						Preference { blinking: !current.blinking, ..current },
					),
					(
						"cursor-animation",
						"Cursor motion",
						"Animate the insertion point as you type.",
						current.animated,
						Preference { animated: !current.animated, ..current },
					),
				]
				.into_iter()
				.map(|(id, label, description, enabled, value)| {
					ui_theme::settings_row()
						.child(ui_theme::settings_label(label, description))
						.child(
							switch::settings_switch(id, id, label, enabled, true)
								.on_change(cx.listener(move |_, _, _, cx| value.select(cx))),
						)
				}),
			)
	}

	fn appearance_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let selected = Appearance::current();
		ui_theme::settings_row()
			.child(ui_theme::settings_label(
				"Appearance",
				"Follow your system, or choose a light or dark workspace.",
			))
			.child(
				ui_theme::settings_segments("Appearance").children(
					[
						(Appearance::System, "System"),
						(Appearance::Light, "Light"),
						(Appearance::Dark, "Dark"),
					]
					.into_iter()
					.map(|(value, label)| {
						ui_theme::settings_segment(label, label, value == selected)
							.debug_selector(move || format!("theme-{label}"))
							.aria_label(format!("Appearance: {label}"))
							.on_click(cx.listener(move |_, _, _, cx| value.select(cx)))
							.on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
								if !event.is_held
									&& ["enter", "space"].contains(&event.keystroke.key.as_str())
								{
									value.select(cx);
									cx.stop_propagation();
								}
							}))
					}),
				),
			)
	}

	fn material_controls(&self, surface: Surface, cx: &mut Context<Self>) -> impl IntoElement {
		let selected = surface.material();
		let id = match surface {
			Surface::Window => "window-material",
			Surface::InputBar => "input-bar-material",
		};
		let description = match surface {
			Surface::Window => "Choose the finish behind the workspace.",
			Surface::InputBar => "Choose the finish behind the message composer.",
		};
		ui_theme::settings_row()
			.id(id)
			.child(ui_theme::settings_label(surface.label(), description))
			.child(ui_theme::settings_segments(surface.label()).children(
				[Material::Thin, Material::LiquidGlass].into_iter().map(|material| {
					let label = material.label();
					ui_theme::settings_segment(label, label, material == selected)
						.debug_selector(move || format!("{id}-{label}"))
						.aria_label(format!("{}: {label}", surface.label()))
						.on_click(cx.listener(move |_, _, _, cx| surface.select(material, cx)))
						.on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
							if !event.is_held
								&& ["enter", "space"].contains(&event.keystroke.key.as_str())
							{
								surface.select(material, cx);
								cx.stop_propagation();
							}
						}))
				}),
			))
	}

	fn glass_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let style = GlassStyle::configured();
		ui_theme::settings_row()
			.child(ui_theme::settings_label(
				"Glass finish",
				"Use a softer or more transparent window finish.",
			))
			.child(ui_theme::settings_segments("Glass appearance").children(
				[(GlassStyle::Regular, "Regular"), (GlassStyle::Clear, "Clear")].into_iter().map(
					|(value, label)| {
						ui_theme::settings_segment(label, label, value == style)
							.debug_selector(move || label.into())
							.aria_label(format!("Window Liquid Glass style: {label}"))
							.on_click(cx.listener(move |_, _, _, cx| value.select(cx)))
							.on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
								if !event.is_held
									&& ["enter", "space"].contains(&event.keystroke.key.as_str())
								{
									value.select(cx);
									cx.stop_propagation();
								}
							}))
					},
				),
			))
	}
}

impl SettingsSurface {
	pub(crate) fn quota_control(&self, cx: &mut Context<Self>) -> AnyElement {
		ui_theme::settings_row()
			.child(ui_theme::settings_label(
				"Auto-activate weekly quota",
				"Start the next weekly window with a small request. Uses quota; no chat is saved.",
			))
			.child(self.quota_toggle(cx))
			.into_any_element()
	}

	fn category_content(&self, cx: &mut Context<Self>) -> AnyElement {
		match self.category {
            SettingsCategory::Updates => self.updates_content(cx),
            SettingsCategory::General => gpui::div().flex().flex_col().gap(gpui::px(SETTINGS_GROUP_GAP))
                .child(ui_theme::settings_group("Startup", vec![
                    self.launch_at_login_card(cx).into_any_element(),
                    ui_theme::settings_row().child(ui_theme::settings_label("Show in menu bar", "Keep quick access to Decodex in the macOS menu bar."))
                        .child(self.menu_bar_toggle(cx)).into_any_element(),
                ]))
                .child(ui_theme::settings_group("Notifications", vec![
                    self.preference_control("notification-count-preference", "notification-count-knob", "Notification count", "Show unread notices on the notification button.", crate::shell::notification_count_preference, cx).into_any_element(),
                    self.preference_control("question-notice-preference", "question-notice-knob", "New question notices", "Let me know when an agent needs a response.", crate::shell::question_notice_preference, cx).into_any_element(),
                ]))
                .child(ui_theme::settings_group("Task recaps", vec![
                    ui_theme::settings_row().child(ui_theme::settings_label("Automatic task recaps", "After 30 minutes away, recap new completed work in the selected task. Uses its model and quota."))
                        .child(self.recap_toggle(cx)).into_any_element(),
                ]))
                .child(ui_theme::settings_group("Power", vec![self.power_control(cx)]))
                .into_any_element(),
            SettingsCategory::Appearance => gpui::div().flex().flex_col().gap(gpui::px(SETTINGS_GROUP_GAP))
                .child(ui_theme::settings_card().child(self.appearance_controls(cx)))
                .child(gpui::div().flex().flex_col().gap_2()
                    .child(settings_group_title("Color theme"))
                    .child(gpui::div().text_size(gpui::px(11.5)).text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
                        .child("Custom palettes use dark appearance."))
                    .child(self.theme_controls(cx)))
                .child(ui_theme::settings_group("Materials", {
                    let mut controls = vec![
                        self.material_controls(Surface::Window, cx).into_any_element(),
                        self.material_controls(Surface::InputBar, cx).into_any_element(),
                    ];
                    if Surface::Window.material() == Material::LiquidGlass {
                        controls.push(self.glass_controls(cx).into_any_element());
                    }
                    controls
                }))
                .child(ui_theme::settings_group("Panel layout", vec![self.panel_controls(cx).into_any_element()]))
                .child(ui_theme::settings_group("Text cursor", vec![self.cursor_controls(cx).into_any_element()]))
                .child(ui_theme::settings_group("Motion", vec![
                    self.preference_control("smooth-scrolling", "smooth-scrolling-knob", "Smooth scrolling", "Animate movement when you scroll with a mouse wheel.", crate::ui_scroll::preference, cx).into_any_element(),
                ]))
                .child(ui_theme::settings_group("Advanced rendering", vec![
                    self.preference_control("debug-mode", "debug-mode-knob", "Debug overlay", "Show rendering performance and system GPU metrics in the top bar.", crate::ui_performance::preference, cx).into_any_element(),
                    self.preference_control("full-background-rendering", "full-background-rendering-knob", "Render while inactive", "Keep the full frame rate when Decodex is in the background. Uses more energy.", crate::ui_frame::background_rendering, cx).into_any_element(),
                ]))
                .child(quote_attribution()).into_any_element(),
        }
	}
}

impl Render for SettingsSurface {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.id("settings-surface")
			.role(Role::Main)
			.aria_label("Decodex settings")
			.size_full()
			.min_w_0()
			.min_h_0()
			.text_size(gpui::px(BODY_SIZE))
			.line_height(gpui::px(BODY_LINE_HEIGHT))
			.text_color(ui_theme::rgb(ui_theme::TEXT))
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
							.child(ui_theme::settings_heading(
								self.category.title(),
								self.category.description(),
							))
							.child(self.category_content(cx)),
					)
					.smooth_scroll(("settings-smooth-scroll", self.category as usize))
					.with_scrollbar("settings-scrollbar", "Scroll settings"),
			)
	}
}

fn settings_group_title(label: &'static str) -> impl IntoElement {
	gpui::div()
		.text_size(gpui::px(11.5))
		.font_weight(FontWeight::MEDIUM)
		.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
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
		.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
		.cursor_pointer()
		.hover(|d| d.text_color(ui_theme::rgb(ui_theme::TEXT)))
		.on_click(|_, _, cx| cx.open_url("https://zenquotes.io/"))
		.on_key_down(|event, _, cx| {
			if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				cx.open_url("https://zenquotes.io/");
				cx.stop_propagation();
			}
		})
		.child("Quotes by ZenQuotes")
}

#[cfg(test)]
mod tests {
	use gpui::{self, AppContext as _, TestAppContext};
	use ui_theme::{
		appearance::Appearance,
		material::{GlassStyle, Material, Surface},
	};

	use crate::settings_surface::{
		self, Context, DesktopSettingsCommandState, DesktopSettingsController,
		DesktopSettingsLoadState, IntoElement, LaunchAtLoginState, MenuBarRuntimeState,
		ParentElement, Render, SettingsCategory, SettingsSurface, Styled, Window, switch, ui_theme,
	};

	fn scroll_to_control(visual: &mut gpui::VisualTestContext, selector: &'static str) {
		let viewport = visual.debug_bounds("settings-scroll-viewport").expect("settings viewport");
		let target = visual.debug_bounds(selector).expect("setting control");
		let distance = (target.bottom() - viewport.bottom() + gpui::px(24.)).max(gpui::px(0.));
		if distance > gpui::px(0.) {
			visual.simulate_event(gpui::ScrollEvent {
				position: viewport.center(),
				delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), -distance)),
				..Default::default()
			});
			visual.update(|window, cx| window.draw(cx).clear(cx));
			std::thread::sleep(std::time::Duration::from_millis(300));
			visual.update(|window, cx| window.draw(cx).clear(cx));
		}
	}

	#[gpui::test]
	fn switch_thumbs_fit_both_track_positions(cx: &mut TestAppContext) {
		struct Switches;
		impl Render for Switches {
			fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
				gpui::div()
					.flex()
					.flex_col()
					.gap_3()
					.child(switch::settings_switch("on-track", "on-thumb", "On", true, true))
					.child(switch::settings_switch("off-track", "off-thumb", "Off", false, true))
			}
		}
		let (_, visual) = cx.add_window_view(|_, _| Switches);
		visual.update(|window, cx| window.draw(cx).clear(cx));
		for (track, thumb) in [("on-track-track", "on-thumb"), ("off-track-track", "off-thumb")] {
			let track = visual.debug_bounds(track).unwrap();
			let thumb = visual.debug_bounds(thumb).unwrap();
			assert!(thumb.left() >= track.left() && thumb.right() <= track.right());
			assert!(thumb.top() >= track.top() && thumb.bottom() <= track.bottom());
		}
	}

	#[gpui::test]
	fn short_appearance_keeps_the_last_control_reachable(cx: &mut TestAppContext) {
		struct ShortAppearance(gpui::Entity<SettingsSurface>);
		impl Render for ShortAppearance {
			fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
				gpui::div()
					.w(gpui::px(800.))
					.h(gpui::px(360.))
					.overflow_hidden()
					.child(self.0.clone())
			}
		}
		let (_, visual) = cx.add_window_view(|_, cx| {
			ShortAppearance(cx.new(|cx| {
				let mut surface = SettingsSurface::new(DesktopSettingsController::production(), cx);
				surface.category = SettingsCategory::Appearance;
				surface
			}))
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		let viewport = visual.debug_bounds("settings-scroll-viewport").unwrap();
		assert!(
			visual.debug_bounds("full-background-rendering").unwrap().bottom() > viewport.bottom()
		);
		scroll_to_control(visual, "full-background-rendering");
		let control = visual.debug_bounds("full-background-rendering").unwrap();
		assert!(control.top() >= viewport.top() && control.bottom() <= viewport.bottom());
		assert!(visual.debug_bounds("settings-scrollbar").is_some());
	}

	#[gpui::test]
	fn reset_sizes_restores_the_panel_defaults(cx: &mut TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, cx| {
			crate::panel_preferences::PanelDefaults { sidebar: 312, dock: 288 }.select(cx);
			let mut surface = SettingsSurface::new(DesktopSettingsController::production(), cx);
			surface.category = SettingsCategory::Appearance;
			surface
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		scroll_to_control(visual, "reset-panel-sizes");
		let reset = visual.debug_bounds("reset-panel-sizes").unwrap();
		visual.simulate_click(reset.center(), Default::default());
		let defaults = crate::panel_preferences::PanelDefaults::configured();
		assert_eq!((defaults.sidebar, defaults.dock), (240, 240));
	}

	#[gpui::test]
	fn theme_cards_apply_every_preset_and_keep_other_windows_in_sync(cx: &mut TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, cx| {
			Appearance::Light.apply(cx);
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.category = SettingsCategory::Appearance;
			settings
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(800.), gpui::px(700.)));
			window.draw(cx).clear(cx);
		});

		let option_width = visual.debug_bounds("color-theme-decodex").unwrap().size.width;
		for (theme, selector) in ui_theme::ColorTheme::ALL.into_iter().zip([
			"color-theme-decodex",
			"color-theme-tokyo-night",
			"color-theme-catppuccin-mocha",
			"color-theme-dracula",
			"color-theme-nord",
			"color-theme-gruvbox-dark",
			"color-theme-one-dark",
			"color-theme-ayu-dark",
			"color-theme-rose-pine",
			"color-theme-kanagawa",
			"color-theme-everforest",
			"color-theme-solarized-dark",
			"color-theme-github-dark",
		]) {
			let bounds = visual.debug_bounds(selector).expect("theme card");
			assert!(
				(bounds.size.width - option_width).abs() <= gpui::px(1.),
				"theme options must have equal widths"
			);
			visual.simulate_click(bounds.center(), Default::default());
			assert_eq!(ui_theme::ColorTheme::configured(), theme);
			if theme != ui_theme::ColorTheme::Decodex {
				assert_eq!(Appearance::current(), Appearance::Dark);
				assert_eq!(
					ui_theme::ColorScheme::current().hex(ui_theme::CANVAS),
					theme.palette().canvas
				);
			}
			visual.update(|window, cx| window.draw(cx).clear(cx));
		}

		let (_, second) = visual.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});
		second.update(|window, cx| {
			assert_eq!(ui_theme::ColorTheme::configured(), ui_theme::ColorTheme::GithubDark);
			window.draw(cx).clear(cx);
		});
		second.update(|_, cx| ui_theme::ColorTheme::TokyoNight.select(cx));
		visual.update(|window, cx| {
			assert_eq!(ui_theme::ColorScheme::current().hex(ui_theme::TEXT), 0xc0caf5);
			window.draw(cx).clear(cx);
		});
		visual.update(|_, cx| Appearance::Light.apply(cx));
		visual.update(|window, cx| {
			assert_eq!(ui_theme::ColorTheme::configured(), ui_theme::ColorTheme::TokyoNight);
			assert_eq!(ui_theme::ColorScheme::current().hex(ui_theme::TEXT), 0x2c2f36);
			window.draw(cx).clear(cx);
		});
		visual.update(|_, cx| Appearance::Dark.apply(cx));
		visual.update(|window, cx| {
			assert_eq!(ui_theme::ColorScheme::current().hex(ui_theme::TEXT), 0xc0caf5);
			window.draw(cx).clear(cx);
		});
	}

	#[gpui::test]
	fn theme_cards_support_keyboard_selection(cx: &mut TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, cx| {
			ui_theme::ColorTheme::Nord.apply(cx);
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.category = SettingsCategory::Appearance;
			settings
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(800.), gpui::px(700.)));
			window.draw(cx).clear(cx);
			for _ in 0..4 {
				window.focus_next(cx);
			}
		});
		visual.simulate_keystrokes("space");
		assert_eq!(ui_theme::ColorTheme::configured(), ui_theme::ColorTheme::Decodex);
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
			window.focus_next(cx);
		});
		visual.simulate_keystrokes("enter");
		assert_eq!(ui_theme::ColorTheme::configured(), ui_theme::ColorTheme::TokyoNight);
	}

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
	fn service_loading_does_not_replace_local_switches(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});

		surface.update(visual, |s, cx| {
			s.snapshot.settings = None;
			s.snapshot.load = DesktopSettingsLoadState::Loading;

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear(cx));

		assert!(visual.debug_bounds("menubar-surface-toggle").is_some());
		assert!(visual.debug_bounds("menubar-loading").is_none());
		assert!(visual.debug_bounds("automatic-recap-toggle").is_some());
		assert!(visual.debug_bounds("recap-loading").is_none());
		assert!(visual.debug_bounds("loading-feedback-").is_none());
	}

	#[gpui::test]
	fn automatic_recaps_change_without_a_service_connection(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});
		visual.update(|_, cx| assert!(!surface.read(cx).automatic_recaps_enabled()));
		for expected in [true, false] {
			visual.update(|window, cx| window.draw(cx).clear(cx));
			let toggle = visual.debug_bounds("automatic-recap-toggle").expect("local recap switch");
			visual.simulate_click(toggle.center(), Default::default());
			visual.update(|_, cx| {
				let settings = surface.read(cx);
				assert!(settings.controller.snapshot().settings.is_none());
				assert_eq!(settings.recap_choice, Some(expected));
				assert_eq!(settings.automatic_recaps_enabled(), expected);
			});
		}
		// A delayed service response cannot re-enable recaps after an explicit opt-out.
		surface.update(visual, |settings, _| {
			let mut snapshot = settings.snapshot;
			snapshot.load = DesktopSettingsLoadState::Ready;
			let mut legacy = decodex_protocol::DesktopSettingsDto::new(
				true,
				decodex_protocol::EntityRevision(1),
			)
			.expect("legacy settings");
			legacy.auto_recap = true;
			snapshot.settings = Some(legacy);
			settings.apply_snapshot(snapshot);
			assert_eq!(settings.recap_choice, Some(false));
			assert!(!settings.automatic_recaps_enabled());
		});
	}

	#[gpui::test]
	fn existing_recap_choice_is_imported_once(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});
		surface.update(visual, |settings, _| {
			let mut snapshot = settings.snapshot;
			snapshot.load = DesktopSettingsLoadState::Ready;
			let mut legacy = decodex_protocol::DesktopSettingsDto::new(
				true,
				decodex_protocol::EntityRevision(1),
			)
			.expect("legacy settings");
			legacy.auto_recap = true;
			snapshot.settings = Some(legacy);
			settings.apply_snapshot(snapshot);
			assert!(settings.automatic_recaps_enabled());
			snapshot.settings.as_mut().expect("settings").auto_recap = false;
			settings.apply_snapshot(snapshot);
			assert!(settings.automatic_recaps_enabled());
		});
	}

	#[gpui::test]
	fn menu_bar_changes_immediately_without_a_service_connection(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});
		for expected in [false, true] {
			visual.update(|window, cx| window.draw(cx).clear(cx));
			let toggle = visual.debug_bounds("menubar-surface-toggle").expect("local switch");
			visual.simulate_click(toggle.center(), Default::default());
			visual.update(|_, cx| {
				let settings = surface.read(cx);
				assert!(settings.controller.snapshot().settings.is_none());
				assert_eq!(settings.menu_bar_choice, Some(expected));
				assert_eq!(
					settings.runtime,
					if expected {
						MenuBarRuntimeState::Visible
					} else {
						MenuBarRuntimeState::Hidden
					}
				);
			});
		}
		// A late read from an older client must not undo the user's local choice.
		surface.update(visual, |settings, _| {
			let mut snapshot = settings.snapshot;
			snapshot.load = DesktopSettingsLoadState::Ready;
			snapshot.settings = Some(
				decodex_protocol::DesktopSettingsDto::new(
					false,
					decodex_protocol::EntityRevision(1),
				)
				.expect("legacy preference"),
			);
			settings.apply_snapshot(snapshot);
			assert_eq!(settings.menu_bar_choice, Some(true));
			assert_eq!(settings.runtime, MenuBarRuntimeState::Visible);
		});
	}

	#[gpui::test]
	fn existing_menu_bar_opt_out_is_imported_once(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});
		surface.update(visual, |settings, _| {
			let mut snapshot = settings.snapshot;
			snapshot.load = DesktopSettingsLoadState::Ready;
			snapshot.settings = Some(
				decodex_protocol::DesktopSettingsDto::new(
					false,
					decodex_protocol::EntityRevision(1),
				)
				.expect("legacy opt-out"),
			);
			settings.apply_snapshot(snapshot);
			assert_eq!(settings.menu_bar_choice, Some(false));
			assert_eq!(settings.runtime, MenuBarRuntimeState::Hidden);
			snapshot.settings.as_mut().expect("settings").show_in_menu_bar = true;
			settings.apply_snapshot(snapshot);
			assert_eq!(settings.menu_bar_choice, Some(false));
			assert_eq!(settings.runtime, MenuBarRuntimeState::Hidden);
		});
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

		visual.update(|window, cx| window.draw(cx).clear(cx));

		let viewport = visual.debug_bounds("settings-scroll-viewport").unwrap();
		let before = visual.debug_bounds("prevent-system-sleep").unwrap();

		assert!(before.bottom() > viewport.bottom(), "before={before:?}, viewport={viewport:?}");

		visual.simulate_event(gpui::ScrollEvent {
			position: viewport.center(),
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-1_000.))),
			..Default::default()
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));

		let after = visual.debug_bounds("prevent-system-sleep").unwrap();

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
			window.draw(cx).clear(cx);
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

			visual.update(|window, cx| window.draw(cx).clear(cx));

			assert!(visual.debug_bounds("menubar-runtime-status").is_none());
			assert_eq!(visual.debug_bounds("launch-at-login-toggle"), Some(original));
		}

		settings.update(visual, |s, cx| {
			s.snapshot.command = DesktopSettingsCommandState::Refused;
			s.preference_error = Some("The service refused the change.".into());

			cx.notify();
		});

		visual.update(|window, cx| window.draw(cx).clear(cx));

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
			window.draw(cx).clear(cx);
		});

		scroll_to_control(visual, "Clear");
		let clear = visual.debug_bounds("Clear").expect("Clear control");

		visual.simulate_click(clear.center(), Default::default());

		assert_eq!(GlassStyle::configured(), GlassStyle::Clear);

		let regular = visual.debug_bounds("Regular").expect("Regular control");

		visual.simulate_click(regular.center(), Default::default());

		assert_eq!(GlassStyle::configured(), GlassStyle::Regular);
	}

	#[gpui::test]
	fn window_and_input_material_controls_stay_independent(cx: &mut TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.category = SettingsCategory::Appearance;
			settings
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(800.), gpui::px(900.)));
			window.draw(cx).clear(cx);
		});
		for target in [Surface::InputBar, Surface::Window] {
			let id = if target == Surface::Window {
				"window-material-Thin"
			} else {
				"input-bar-material-Thin"
			};
			let bounds = visual.debug_bounds(id).expect("material control");
			visual.simulate_click(bounds.center(), Default::default());
			assert_eq!(target.material(), Material::Thin);
			if target == Surface::InputBar {
				assert_eq!(Surface::Window.material(), Material::LiquidGlass);
			}
			visual.update(|window, cx| window.draw(cx).clear(cx));
		}
		assert!(visual.debug_bounds("Regular").is_none(), "glass style has no effect on Thin");
		for target in [Surface::Window, Surface::InputBar] {
			let id = if target == Surface::Window {
				"window-material-Liquid Glass"
			} else {
				"input-bar-material-Liquid Glass"
			};
			let bounds = visual.debug_bounds(id).expect("material control");
			visual.simulate_click(bounds.center(), Default::default());
			visual.update(|window, cx| window.draw(cx).clear(cx));
		}
		assert!(visual.debug_bounds("Regular").is_some());
	}

	#[gpui::test]
	fn debug_mode_is_opt_in_and_can_be_disabled_again(cx: &mut TestAppContext) {
		assert!(!crate::ui_performance::preference(None));
		let (_, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.category = SettingsCategory::Appearance;
			settings
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(800.), gpui::px(620.)));
			window.draw(cx).clear(cx);
		});
		scroll_to_control(visual, "debug-mode");
		let toggle = visual.debug_bounds("debug-mode").expect("debug switch");
		assert!(
			toggle.bottom() <= visual.debug_bounds("settings-scroll-viewport").unwrap().bottom()
		);
		visual.simulate_click(toggle.center(), Default::default());
		assert!(crate::ui_performance::preference(None));
		visual.update(|window, cx| window.draw(cx).clear(cx));
		visual.simulate_click(toggle.center(), Default::default());
		assert!(!crate::ui_performance::preference(None));
	}

	#[gpui::test]
	fn background_rendering_switch_applies_to_every_window(cx: &mut TestAppContext) {
		crate::ui_frame::background_rendering(Some(false));
		let (_, visual) = cx.add_window_view(|_, cx| {
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.category = SettingsCategory::Appearance;
			settings
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(800.), gpui::px(900.)));
			window.draw(cx).clear(cx);
		});
		scroll_to_control(visual, "full-background-rendering");
		let bounds =
			visual.debug_bounds("full-background-rendering").expect("background preference");
		visual.simulate_click(bounds.center(), Default::default());
		assert!(crate::ui_frame::background_rendering(None));
		visual.update(|window, cx| window.draw(cx).clear(cx));
		visual.simulate_click(bounds.center(), Default::default());
		assert!(!crate::ui_frame::background_rendering(None));
	}

	#[gpui::test]
	fn settings_draw_at_the_selected_desktop_size(cx: &mut TestAppContext) {
		let controller = DesktopSettingsController::production();
		let (_settings, visual) = cx.add_window_view(|_, cx| SettingsSurface::new(controller, cx));

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_490.0), gpui::px(1_055.0)));
			window.draw(cx).clear(cx);
		});
	}
}
