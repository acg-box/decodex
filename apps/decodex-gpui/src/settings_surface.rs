//! Product-level settings for the sole Decodex desktop application.
//!
//! Service behavior remains daemon-owned. Menu-bar visibility and automatic recaps
//! run in the desktop and must not wait for service startup or queries.

#[path = "settings_components.rs"] mod components;
#[path = "settings_native.rs"] mod native;
#[path = "settings_power.rs"] mod power;
#[path = "ui_switch.rs"] mod switch;
#[path = "settings_themes.rs"] mod themes;
#[path = "settings_updates.rs"] mod updates;

use gpui::{
	AnyElement, ClickEvent, Context, Div, Entity, KeyDownEvent, Render, Role, SharedString,
	Stateful, Window,
	prelude::{
		FluentBuilder, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
		Styled,
	},
};
use ui_theme::{
	appearance::Appearance,
	material::{GlassScope, GlassStyle, Material, Surface},
};

use crate::{
	composer_input::cursor::{Preference, Shape},
	desktop_settings::{
		DesktopSettingsCommandState, DesktopSettingsController, DesktopSettingsInputError,
		DesktopSettingsLoadState, DesktopSettingsSnapshot,
	},
	native_menu_bar::{LaunchAtLoginState, NativeMenuBarHost},
	panel_preferences::PanelDefaults,
	shell::agent_surface::AgentSurface,
	ui_feedback::{self, Feedback},
	ui_loading,
	ui_motion::{Choice, Segmented},
	ui_preferences,
	ui_scroll::SmoothScrollArea,
	ui_theme::{
		self, SETTINGS_CONTROL_HEIGHT, SETTINGS_INSET, SETTINGS_LABEL_SIZE, SETTINGS_TOP,
		SETTINGS_WIDTH, TEXT,
	},
};
use power::PowerSettings;
pub(crate) use switch::settings_switch;

const MENU_BAR_PREFERENCE: &str = "DecodexMenuBarVisible";
const RECAP_PREFERENCE: &str = "DecodexAutomaticTaskRecaps";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SettingsCategory {
	#[default]
	General,
	Agents,
	Appearance,
	Components,
}
impl SettingsCategory {
	pub(crate) fn title(self) -> &'static str {
		match self {
			Self::General => "General",
			Self::Agents => "Agents",
			Self::Appearance => "Appearance",
			Self::Components => "Components",
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

pub(crate) struct SettingsSurface {
	components: components::Components,
	updates: updates::Updates,
	pub(crate) category: SettingsCategory,
	native_agent: Option<gpui::Entity<crate::shell::agent_surface::AgentSurface>>,
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
	session_defaults: crate::session_settings::Defaults,
	default_speed: Option<decodex_protocol::ServiceTier>,
	guardian: Option<Entity<crate::ui_frame::Presentation<AgentSurface>>>,
}
impl SettingsSurface {
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
			.child(
				gpui::div()
					.text_size(gpui::px(11.5))
					.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
					.child("Initial panel sizes. Drag panel edges to resize while working."),
			)
			.child(ui_theme::settings_divided_rows(
				[
					(true, "Default sidebar width", current.sidebar),
					(false, "Default Dock height", current.dock),
				]
				.into_iter()
				.map(|(sidebar, title, value)| {
					ui_theme::settings_row()
						.child(ui_theme::settings_label(title, ""))
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
									ui_theme::settings_button_group(title)
										.children([(-24_i32, "−"), (24, "+")].into_iter().map(
											|(delta, label)| {
												ui_theme::settings_button(
													SharedString::from(format!(
														"panel-default-{sidebar}-{delta}"
													)),
													label,
												)
												.w(gpui::px(SETTINGS_CONTROL_HEIGHT))
												.aria_label(format!(
													"{} {title}",
													if delta < 0 { "Decrease" } else { "Increase" }
												))
												.on_click(cx.listener(move |s, _, _, cx| {
													s.adjust_panel(sidebar, delta, cx)
												}))
												.on_key_down(cx.listener(
													move |s, e: &KeyDownEvent, _, cx| {
														if !e.is_held
															&& ["enter", "space"]
																.contains(&e.keystroke.key.as_str())
														{
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
						.into_any_element()
				})
				.collect(),
			))
			.child(
				ui_theme::settings_row().min_h(gpui::px(44.)).justify_end().child(
					ui_theme::settings_button("reset-panel-sizes", "Reset sizes")
						.on_click(cx.listener(|_, _, _, cx| {
							PanelDefaults { sidebar: 240, dock: 240 }.select(cx)
						}))
						.on_key_down(cx.listener(|_, e: &KeyDownEvent, _, cx| {
							if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str())
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

		let shape = ui_theme::settings_row()
			.child(ui_theme::settings_label("Cursor style", ""))
			.child(Segmented::new(
				"settings-segments-Cursor style",
				"Cursor style",
				[("Bar", Shape::Bar), ("Block", Shape::Block), ("Underline", Shape::Underline)]
					.map(|(label, _)| {
						Choice::new(
							format!("Cursor style-{label}"),
							label,
							format!("Cursor style: {label}"),
						)
					}),
				[Shape::Bar, Shape::Block, Shape::Underline]
					.iter()
					.position(|shape| *shape == current.shape)
					.unwrap_or(0),
				cx.listener(move |_, index: &usize, _, cx| {
					Preference {
						shape: [Shape::Bar, Shape::Block, Shape::Underline][*index],
						..current
					}
					.select(cx);
				}),
			))
			.into_any_element();
		let mut rows = vec![shape];
		rows.extend(
			[
				(
					"cursor-blinking",
					"Blinking cursor",
					"",
					current.blinking,
					Preference { blinking: !current.blinking, ..current },
				),
				(
					"cursor-animation",
					"Smooth cursor movement",
					"",
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
					.into_any_element()
			}),
		);
		ui_theme::settings_divided_rows(rows)
	}

	fn appearance_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let selected = Appearance::current();

		ui_theme::settings_row()
			.child(ui_theme::settings_label(
				"Color scheme",
				if selected == Appearance::System {
					"Follows your Mac’s appearance."
				} else {
					""
				},
			))
			.child(Segmented::new(
				"settings-segments-Appearance",
				"Appearance",
				["System", "Light", "Dark"].map(|label| {
					Choice::new(format!("theme-{label}"), label, format!("Appearance: {label}"))
				}),
				[Appearance::System, Appearance::Light, Appearance::Dark]
					.iter()
					.position(|value| *value == selected)
					.unwrap_or(0),
				cx.listener(|_, index: &usize, _, cx| {
					[Appearance::System, Appearance::Light, Appearance::Dark][*index].select(cx)
				}),
			))
	}

	fn material_controls(&self, surface: Surface, cx: &mut Context<Self>) -> impl IntoElement {
		let selected = surface.material();
		let id = match surface {
			Surface::Window => "window-material",
			Surface::InputBar => "input-bar-material",
			Surface::Popover => "popover-material",
		};
		let description = match surface {
			Surface::Window => "Thin uses a flat tint; Liquid Glass uses macOS glass.",
			Surface::InputBar => "",
			Surface::Popover => "Applies to menus, hover previews and activity capsules.",
		};

		ui_theme::settings_row()
			.id(id)
			.child(ui_theme::settings_label(surface.label(), description))
			.child(Segmented::new(
				format!("settings-segments-{}", surface.label()),
				surface.label(),
				[Material::Thin, Material::LiquidGlass].map(|material| {
					Choice::new(
						format!("{id}-{}", material.label()),
						material.label(),
						format!("{}: {}", surface.label(), material.label()),
					)
				}),
				usize::from(selected == Material::LiquidGlass),
				cx.listener(move |_, index: &usize, _, cx| {
					surface.select([Material::Thin, Material::LiquidGlass][*index], cx)
				}),
			))
	}

	fn glass_controls(&self, scope: GlassScope, cx: &mut Context<Self>) -> impl IntoElement {
		let style = scope.configured();

		ui_theme::settings_row()
			.child(ui_theme::settings_label(
				scope.label(),
				"Regular softens the background; Clear shows more through.",
			))
			.child(Segmented::new(
				format!("settings-segments-{}", scope.label()),
				scope.label(),
				["Regular", "Clear"].map(|label| {
					Choice::new(
						format!("{}-{label}", scope.id()),
						label,
						format!("{}: {label}", scope.label()),
					)
				}),
				usize::from(style == GlassStyle::Clear),
				cx.listener(move |_, index: &usize, _, cx| {
					scope.select([GlassStyle::Regular, GlassStyle::Clear][*index], cx)
				}),
			))
	}
}

impl SettingsSurface {
	fn quota_feedback(&self) -> Option<Feedback> {
		self.preference_error
			.as_ref()
			.map(|message| Feedback::error(message.to_string()))
			.or_else(|| match self.snapshot.command {
				DesktopSettingsCommandState::Refused =>
					Some(Feedback::error(settings_detail(self.snapshot))),
				DesktopSettingsCommandState::OutcomeUnknown =>
					Some(Feedback::warning(settings_detail(self.snapshot))),
				_ => None,
			})
			.or_else(|| match self.snapshot.load {
				DesktopSettingsLoadState::Refused =>
					Some(Feedback::error(settings_detail(self.snapshot))),
				DesktopSettingsLoadState::Offline | DesktopSettingsLoadState::Unavailable =>
					Some(Feedback::warning(settings_detail(self.snapshot))),
				_ => None,
			})
	}

	pub(crate) fn quota_control(&self, cx: &mut Context<Self>) -> AnyElement {
		gpui::div()
			.w_full()
			.flex()
			.flex_col()
			.child(
				ui_theme::settings_row()
					.child(ui_theme::settings_label(
						"Auto-activate weekly quota",
						"Start each weekly quota window with a small request. Uses quota; no chat is saved.",
					))
					.child(self.quota_toggle(cx)),
			)
			.children(setting_notice("quota-settings-feedback", self.quota_feedback()))
			.into_any_element()
	}

	fn category_content(&self, cx: &mut Context<Self>) -> AnyElement {
		match self.category {
			SettingsCategory::General => self.general_content(cx),
			SettingsCategory::Agents => ui_theme::settings_sections()
				.child(ui_theme::settings_group(
					"New conversations",
					vec![self.agent_defaults_content(cx)],
				))
				.child(ui_theme::settings_group("Context defaults", vec![self.native_defaults(cx)]))
				.child(ui_theme::settings_group("Assistance", {
					let mut rows = vec![
						self.preference_control(
							"automatic-prompt-suggestions",
							"automatic-prompt-suggestions-knob",
							"Next-message suggestions",
							"Suggest after each reply. Uses quota; never sends automatically.",
							crate::ui_preferences::automatic_prompt_suggestions,
							cx,
						)
						.into_any_element(),
					];
					rows.extend(self.guardian.clone().map(|guardian| guardian.into_any_element()));
					rows
				}))
				.into_any_element(),
			SettingsCategory::Appearance => self.appearance_content(cx),
			SettingsCategory::Components => self.components_content(cx),
		}
	}

	fn choose_default_permissions(
		&mut self,
		mode: decodex_protocol::PermissionMode,
		cx: &mut Context<Self>,
	) {
		if self.session_defaults.permissions != mode {
			self.session_defaults.permissions = mode;
			self.session_defaults.save();
		}
		cx.refresh_windows();
	}

	fn choose_default_speed(
		&mut self,
		tier: decodex_protocol::ServiceTier,
		cx: &mut Context<Self>,
	) {
		if self.default_speed.as_ref() == Some(&tier) {
			return;
		}
		if let Err(error) = crate::session_settings::set_default_speed(&tier) {
			self.preference_error = Some(format!("Could not save speed: {error}.").into());
			cx.notify();
			return;
		}
		self.default_speed = Some(tier);
		self.preference_error = None;
		cx.refresh_windows();
	}

	fn agent_defaults_content(&self, cx: &mut Context<Self>) -> AnyElement {
		// ChatGPT usage relative to Standard: https://learn.chatgpt.com/docs/pricing
		let speed_detail = match self.default_speed.as_ref().map(|tier| tier.as_str()) {
			Some("default") => "Shared with Codex (included usage: 1×; credits: 1×).",
			Some("priority") => "Shared with Codex (included usage: 2.5×; credits: 2×).",
			Some("ultrafast") => "Shared with Codex (included usage: 8×; credits: 6×).",
			_ => "Shared with Codex.",
		};
		ui_theme::settings_divided_rows(vec![
			ui_theme::settings_row()
				.child(ui_theme::settings_label(
					"Permissions",
					self.session_defaults.permissions.description(),
				))
				.child(crate::session_settings::permission_picker(
					self.session_defaults.permissions,
					Self::choose_default_permissions,
					cx,
				))
				.into_any_element(),
			ui_theme::settings_row()
				.child(ui_theme::settings_label("Speed", speed_detail))
				.child(
					crate::session_settings::speed_choices(
						self.default_speed.as_ref(),
						["priority", "ultrafast"].map(|id| {
							(
								decodex_protocol::ServiceTier::new(id).expect("built-in speed"),
								crate::session_settings::speed_label(id).to_owned(),
							)
						}),
						Self::choose_default_speed,
						cx,
					)
					.fixed_choice_width(56.),
				)
				.into_any_element(),
		])
		.into_any_element()
	}

	fn general_content(&self, cx: &mut Context<Self>) -> AnyElement {
		ui_theme::settings_sections()
			.child(ui_theme::settings_group(
				"Startup",
				vec![
					self.launch_at_login_card(cx).into_any_element(),
					self.menu_bar_card(cx).into_any_element(),
				],
			))
			.child(ui_theme::settings_group(
				"Notifications",
				vec![
					self.preference_control(
						"notification-count-preference",
						"notification-count-knob",
						"Show notification count",
						"",
						crate::shell::notification_count_preference,
						cx,
					)
					.into_any_element(),
					self.preference_control(
						"question-notice-preference",
						"question-notice-knob",
						"Show Agent question notices",
						"Only for the selected conversation.",
						crate::shell::question_notice_preference,
						cx,
					)
					.into_any_element(),
				],
			))
			.child(ui_theme::settings_group(
				"Background",
				vec![
					ui_theme::settings_row()
						.child(ui_theme::settings_label(
							"Automatic task recaps",
							"Recap the selected chat after 30 quiet minutes. This uses model quota.",
						))
						.child(self.recap_toggle(cx))
						.into_any_element(),
					self.power_control(cx),
				],
			))
			.child(self.app_updates(cx))
			.into_any_element()
	}

	fn appearance_content(&self, cx: &mut Context<Self>) -> AnyElement {
		ui_theme::settings_sections()
			.child(ui_theme::settings_panel(self.appearance_controls(cx)))
			.child(ui_theme::settings_group(
				"Color theme",
				vec![
					ui_theme::settings_rows()
						.py(gpui::px(8.))
						.gap_2()
						.child(ui_theme::settings_description(self.theme_description()))
						.child(self.theme_controls(cx))
						.into_any_element(),
				],
			))
			.child(ui_theme::settings_group("Materials", {
				let mut controls = Vec::new();
				for (surface, scope) in [
					(Surface::Window, Some(GlassScope::Window)),
					(Surface::InputBar, None),
					(Surface::Popover, Some(GlassScope::Popover)),
				] {
					controls.push(self.material_controls(surface, cx).into_any_element());
					if surface.material() == Material::LiquidGlass
						&& let Some(scope) = scope
					{
						controls.push(self.glass_controls(scope, cx).into_any_element());
					}
				}

				controls
			}))
			.child(ui_theme::settings_group(
				"Panel layout",
				vec![self.panel_controls(cx).into_any_element()],
			))
			.child(ui_theme::settings_group(
				"Text cursor",
				vec![self.cursor_controls(cx).into_any_element()],
			))
			.child(ui_theme::settings_group(
				"Motion",
				vec![
					self.preference_control(
						"smooth-scrolling",
						"smooth-scrolling-knob",
						"Smooth scrolling",
						"Animate mouse-wheel scrolling; trackpad gestures stay direct.",
						crate::ui_scroll::preference,
						cx,
					)
					.into_any_element(),
				],
			))
			.child(ui_theme::settings_group(
				"Advanced rendering",
				vec![
					self.preference_control(
						"debug-mode",
						"debug-mode-knob",
						"Show frame rate and GPU usage",
						"",
						crate::ui_performance::preference,
						cx,
					)
					.into_any_element(),
					self.preference_control(
						"full-background-rendering",
						"full-background-rendering-knob",
						"Render while inactive",
						"Full frame rate in the background; uses more energy.",
						crate::ui_frame::background_rendering,
						cx,
					)
					.into_any_element(),
				],
			))
			.child(quote_attribution())
			.into_any_element()
	}
}

impl SettingsSurface {
	pub(crate) fn new(controller: DesktopSettingsController, cx: &mut Context<Self>) -> Self {
		let snapshot = controller.snapshot();
		let menu_bar_choice = ui_preferences::string(MENU_BAR_PREFERENCE, None).and_then(|value| {
			match value.as_str() {
				"shown" => Some(true),
				"hidden" => Some(false),
				_ => None,
			}
		});
		let recap_choice =
			ui_preferences::string(RECAP_PREFERENCE, None).and_then(|value| match value.as_str() {
				"enabled" => Some(true),
				"disabled" => Some(false),
				_ => None,
			});
		let mut menu_bar = NativeMenuBarHost::new();
		let launch_at_login =
			menu_bar.launch_at_login_state().unwrap_or(LaunchAtLoginState::OperationFailed);
		let mut surface = Self {
			updates: updates::Updates::new(),
			components: components::Components::new(crate::component_runtime::shared(cx)),
			power: Default::default(),
			category: SettingsCategory::General,
			native_agent: None,
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
			session_defaults: crate::session_settings::Defaults::load(),
			default_speed: crate::session_settings::default_speed().ok(),
			guardian: None,
		};

		surface.apply_snapshot(snapshot);
		// Read the real system policy before Settings opens, never from a render pass.
		#[cfg(not(test))]
		surface.refresh_power(cx);

		#[cfg(test)]
		let _ = cx;

		surface
	}

	pub(crate) fn bind_agent(&mut self, agent: &Entity<AgentSurface>, cx: &mut Context<Self>) {
		self.guardian = Some(crate::ui_frame::Presentation::new(
			agent,
			AgentSurface::render_guardian_settings,
			cx,
		));
		cx.notify();
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

			if snapshot.command == DesktopSettingsCommandState::Accepted {
				self.preference_error = None;
			}

			self.apply_snapshot(snapshot);
			cx.notify();
		}
	}

	pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
		if self.category == SettingsCategory::Agents {
			self.default_speed = crate::session_settings::default_speed().ok();
			self.session_defaults = crate::session_settings::Defaults::load();
		}
		if self.category == SettingsCategory::Agents
			&& let Some(agent) = &self.native_agent
		{
			agent.update(cx, |agent, cx| agent.read_native_defaults(cx));
		}

		#[cfg(not(test))]
		if self.category == SettingsCategory::Components {
			self.components.refresh_optional();
			if let Some(agent) = &self.native_agent {
				agent.update(cx, |agent, cx| agent.refresh_ragflow_status(cx));
			}
		}

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
		ui_preferences::string(MENU_BAR_PREFERENCE, Some(if enabled { "shown" } else { "hidden" }));

		self.menu_bar_choice = Some(enabled);
	}

	pub(crate) fn automatic_recaps_enabled(&self) -> bool {
		self.recap_choice.unwrap_or(false)
	}

	fn save_recap_choice(&mut self, enabled: bool) {
		ui_preferences::string(
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
				self.detail =
					if visible { "Menu bar enabled." } else { "Menu bar disabled." }.into();
			},
			Err(failure) => {
				self.runtime = MenuBarRuntimeState::Unavailable;
				self.detail = failure.detail().into();
			},
		}
	}

	fn menubar_needs_attention(&self) -> bool {
		self.runtime == MenuBarRuntimeState::Unavailable
	}

	pub(crate) fn notifications(&self) -> Vec<(&'static str, Feedback)> {
		let mut notices = Vec::new();

		if let Some(error) = &self.power.error {
			notices.push(("System sleep", Feedback::error(error.clone())));
		}

		if self.menubar_needs_attention() {
			notices.push(("Menu bar", Feedback::error(self.detail.to_string())));
		}

		if let Some(feedback) = self.quota_feedback() {
			notices.push(("Quota", feedback));
		}
		if let Some(feedback) = self.launch_at_login_feedback() {
			notices.push(("Launch at login", feedback));
		}

		notices
	}

	fn toggle_quota(&mut self, cx: &mut Context<Self>) {
		let Some(settings) = self.snapshot.settings else { return };

		self.preference_error = None;

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
		self.open_login_items(cx);
	}

	fn open_login_items(&mut self, cx: &mut Context<Self>) {
		if let Err(failure) = self.menu_bar.open_login_items_settings() {
			self.launch_at_login_detail = failure.detail().into();
		}

		cx.notify();
	}

	fn retry_launch_at_login(&mut self, cx: &mut Context<Self>) {
		self.refresh_launch_at_login();
		cx.notify();
	}

	fn quota_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
		if self.snapshot.settings.is_none() {
			let loading = self.snapshot.load == DesktopSettingsLoadState::Loading;

			return gpui::div()
				.id("quota-read-state")
				.debug_selector(move || {
					if loading { "quota-request-pending" } else { "quota-unavailable" }.into()
				})
				.flex_none()
				.flex()
				.items_center()
				.gap_2()
				.text_size(gpui::px(11.))
				.text_color(ui_theme::rgb(crate::ui_theme::TEXT_MUTED))
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

	fn launch_at_login_feedback(&self) -> Option<Feedback> {
		(!matches!(
			self.launch_at_login,
			LaunchAtLoginState::NotRegistered | LaunchAtLoginState::Enabled
		) || self.launch_at_login_detail.as_ref() != launch_at_login_detail(self.launch_at_login))
		.then(|| match self.launch_at_login {
			LaunchAtLoginState::RequiresApproval =>
				Feedback::info(self.launch_at_login_detail.to_string()),
			LaunchAtLoginState::NotFound =>
				Feedback::warning(self.launch_at_login_detail.to_string()),
			_ => Feedback::error(self.launch_at_login_detail.to_string()),
		})
	}

	fn launch_at_login_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.w_full()
			.flex()
			.flex_col()
			.child(
				ui_theme::settings_row()
					.child(ui_theme::settings_label("Launch at login", ""))
					.child(self.launch_at_login_toggle(cx)),
			)
			.children(
				setting_notice("launch-at-login-feedback", self.launch_at_login_feedback()).map(
					|notice| {
						notice
							.when(
								matches!(
									self.launch_at_login,
									LaunchAtLoginState::RequiresApproval
										| LaunchAtLoginState::NotFound
								),
								|notice| {
									notice.child(setting_action(
										"launch-at-login-open-settings",
										"Open Login Items",
										cx,
										Self::open_login_items,
									))
								},
							)
							.when(
								self.launch_at_login == LaunchAtLoginState::OperationFailed,
								|notice| {
									notice.child(setting_action(
										"launch-at-login-retry",
										"Retry",
										cx,
										Self::retry_launch_at_login,
									))
								},
							)
					},
				),
			)
	}

	fn menu_bar_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.w_full()
			.flex()
			.flex_col()
			.child(
				ui_theme::settings_row()
					.child(ui_theme::settings_label("Show in menu bar", ""))
					.child(self.menu_bar_toggle(cx)),
			)
			.children(setting_notice(
				"menubar-runtime-status",
				self.menubar_needs_attention().then(|| Feedback::error(self.detail.to_string())),
			))
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
			.text_size(gpui::px(SETTINGS_LABEL_SIZE))
			.line_height(gpui::px(18.0))
			.text_color(ui_theme::rgb(TEXT))
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
						ui_theme::settings_sections()
							.max_w(gpui::px(SETTINGS_WIDTH))
							.flex_none()
							.child(ui_theme::settings_title(self.category.title()))
							.child(self.category_content(cx)),
					)
					.smooth_scroll(("settings-smooth-scroll", self.category as usize))
					.with_scrollbar("settings-scrollbar", "Scroll settings"),
			)
	}
}

fn setting_action(
	id: &'static str,
	label: &'static str,
	cx: &Context<SettingsSurface>,
	action: fn(&mut SettingsSurface, &mut Context<SettingsSurface>),
) -> AnyElement {
	ui_theme::settings_button(id, label)
		.on_click(cx.listener(move |s, _, _, cx| action(s, cx)))
		.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
			if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				action(s, cx);

				cx.stop_propagation();
			}
		}))
		.into_any_element()
}

fn setting_notice(id: &'static str, feedback: Option<Feedback>) -> Option<Stateful<Div>> {
	feedback
		.and_then(|feedback| ui_feedback::inline_feedback(id, "", &feedback))
		.map(|notice| notice.mb_2())
}

const fn launch_at_login_detail(state: LaunchAtLoginState) -> &'static str {
	match state {
		LaunchAtLoginState::NotRegistered => "Off",
		LaunchAtLoginState::Enabled => "On (opens in the background)",
		LaunchAtLoginState::RequiresApproval => "Allow in System Settings > General > Login Items.",
		LaunchAtLoginState::NotFound => "macOS could not find Decodex’s login item.",
		LaunchAtLoginState::OperationFailed => "Login item status unavailable.",
	}
}

const fn settings_detail(snapshot: DesktopSettingsSnapshot) -> &'static str {
	match snapshot.load {
		DesktopSettingsLoadState::NeverRequested => "Settings have not loaded yet.",
		DesktopSettingsLoadState::Loading => "Loading settings…",
		DesktopSettingsLoadState::Ready => match snapshot.command {
			DesktopSettingsCommandState::Sending | DesktopSettingsCommandState::AwaitingResult =>
				"Saving settings…",
			DesktopSettingsCommandState::OutcomeUnknown => "Checking save status…",
			DesktopSettingsCommandState::Refused => "Could not change this setting.",
			DesktopSettingsCommandState::Idle | DesktopSettingsCommandState::Accepted =>
				"Settings saved.",
		},
		DesktopSettingsLoadState::Offline => "Reconnect to load settings.",
		DesktopSettingsLoadState::Unavailable => "Settings unavailable. Try again.",
		DesktopSettingsLoadState::Refused => "Invalid settings response. Retry.",
	}
}

const fn input_error_detail(error: DesktopSettingsInputError) -> &'static str {
	match error {
		DesktopSettingsInputError::Offline => "Reconnect to change this setting.",
		DesktopSettingsInputError::Busy => "Wait for the current settings request.",
		DesktopSettingsInputError::NotLoaded => "Wait for settings to load.",
		DesktopSettingsInputError::IdentityUnavailable => "Could not save. Try again.",
	}
}

fn quote_attribution() -> impl IntoElement {
	gpui::div()
		.id("quote-source")
		.debug_selector(|| "quote-source".into())
		.role(Role::Link)
		.tab_index(0)
		.aria_label("Quotes by ZenQuotes. Open website.")
		.text_size(gpui::px(11.))
		.text_color(ui_theme::rgb(crate::ui_theme::TEXT_MUTED))
		.cursor_pointer()
		.hover(|d| d.text_color(ui_theme::rgb(TEXT)))
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
	use std::thread;

	use gpui::{self, AppContext as _, TestAppContext};
	use ui_theme::{
		appearance::Appearance,
		material::{GlassScope, GlassStyle, Material, Surface},
	};

	use crate::{
		settings_surface::{
			self, Context, DesktopSettingsCommandState, DesktopSettingsController,
			DesktopSettingsLoadState, IntoElement, LaunchAtLoginState, MenuBarRuntimeState,
			ParentElement, Render, SettingsCategory, SettingsSurface, Styled, Window, switch,
			ui_theme,
		},
		ui_frame, ui_performance,
	};

	fn scroll_to_control(visual: &mut gpui::VisualTestContext, selector: &'static str) {
		let viewport = visual.debug_bounds("settings-scroll-viewport").expect("settings viewport");
		let target = visual.debug_bounds(selector).expect("setting control");
		let distance = (target.bottom() - viewport.bottom() + gpui::px(24.)).max(gpui::px(0.));

		if distance > gpui::px(0.) {
			visual.simulate_event(gpui::ScrollEvent {
				// The Settings page owns scrolling, including the theme gallery.
				position: gpui::point(viewport.left() + gpui::px(8.), viewport.center().y),
				delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), -distance)),
				..Default::default()
			});
			visual.update(crate::ui_frame::draw_current_frame);

			thread::sleep(std::time::Duration::from_millis(300));

			visual.update(crate::ui_frame::draw_current_frame);
		}
	}

	fn scroll_to_theme(visual: &mut gpui::VisualTestContext, selector: &'static str) {
		scroll_to_control(visual, selector);
	}

	#[gpui::test]
	fn creation_defaults_use_compact_controls_without_moving_other_settings(
		cx: &mut TestAppContext,
	) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			let mut surface = SettingsSurface::new(DesktopSettingsController::production(), cx);
			surface.category = SettingsCategory::Agents;
			surface.default_speed = Some(decodex_protocol::ServiceTier::standard());
			surface
		});
		visual.update(|_, cx| crate::session_settings::bind_keys(cx));
		visual.simulate_resize(gpui::size(gpui::px(ui_theme::SETTINGS_WIDTH), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_current_frame);
		let permissions = visual.debug_bounds("default-permissions").unwrap();
		let speed = visual.debug_bounds("service-tier-picker").unwrap();
		assert_eq!(permissions.right(), speed.right(), "row controls share their trailing edge");
		assert!(permissions.bottom() < speed.top(), "defaults use separate compact rows");
		assert!(speed.bottom() - permissions.top() < gpui::px(100.));
		assert!(
			visual.debug_bounds("permission-choice-0").is_none(),
			"permission explanations stay in the menu"
		);

		visual.simulate_click(permissions.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		visual.simulate_keystrokes("escape");
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(
			visual.debug_bounds("permission-choice-0").is_none(),
			"Escape closes the permission menu"
		);
		visual.simulate_click(permissions.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		let read_only = visual.debug_bounds("permission-choice-0").unwrap();
		let menu = visual.debug_bounds("popover-surface").unwrap();
		assert_eq!(menu.right(), permissions.right(), "menu surface aligns with its trigger");
		assert_eq!(menu.size.width, gpui::px(330.));
		for (row, description) in [
			("permission-choice-0", "permission-description-0"),
			("permission-choice-1", "permission-description-1"),
			("permission-choice-2", "permission-description-2"),
			("permission-choice-3", "permission-description-3"),
		] {
			let row = visual.debug_bounds(row).unwrap();
			let description = visual.debug_bounds(description).unwrap();
			assert!(
				row.contains(&description.center()),
				"each permission explanation remains visible beside its option"
			);
		}
		visual.simulate_click(read_only.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		settings.read_with(visual, |s, _| {
			assert_eq!(s.session_defaults.permissions, decodex_protocol::PermissionMode::ReadOnly);
		});
		assert!(visual.debug_bounds("permission-choice-0").is_none());
		assert_eq!(visual.debug_bounds("service-tier-picker").unwrap(), speed);
		visual.simulate_click(permissions.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		let selected = visual.debug_bounds("permission-choice-0").unwrap();
		visual.simulate_click(selected.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(
			visual.debug_bounds("permission-choice-0").is_none(),
			"choosing the current value closes the menu"
		);

		for (id, expected) in [
			("tier-priority", "priority"),
			("tier-ultrafast", "ultrafast"),
			("tier-default", "default"),
		] {
			let choice = visual.debug_bounds(id).unwrap();
			visual.simulate_click(choice.center(), Default::default());
			settings.read_with(visual, |s, _| {
				assert_eq!(s.default_speed.as_ref().unwrap().as_str(), expected)
			});
			visual.update(crate::ui_frame::draw_current_frame);
			assert_eq!(visual.debug_bounds("default-permissions").unwrap(), permissions);
			assert_eq!(visual.debug_bounds("service-tier-picker").unwrap(), speed);
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

		visual.update(crate::ui_frame::draw_current_frame);

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

		visual.update(crate::ui_frame::draw_current_frame);

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

		visual.update(crate::ui_frame::draw_current_frame);

		scroll_to_control(visual, "reset-panel-sizes");

		let reset = visual.debug_bounds("reset-panel-sizes").unwrap();

		visual.simulate_click(reset.center(), Default::default());

		let defaults = crate::panel_preferences::PanelDefaults::configured();

		assert_eq!((defaults.sidebar, defaults.dock), (240, 240));
	}

	#[gpui::test]
	fn theme_gallery_shares_page_scrolling_and_keeps_all_options_in_layout(
		cx: &mut TestAppContext,
	) {
		let (_, visual) = cx.add_window_view(|_, cx| {
			let mut surface = SettingsSurface::new(DesktopSettingsController::production(), cx);

			surface.category = SettingsCategory::Appearance;

			surface
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(920.), gpui::px(700.)));
			window.draw(cx).clear(cx);
		});

		let first = visual.debug_bounds("color-theme-decodex").unwrap();
		let fifth = visual.debug_bounds("color-theme-everforest").unwrap();
		let sixth = visual.debug_bounds("color-theme-github-dark").unwrap();

		assert_eq!(first.top(), fifth.top(), "five previews fit in the standard settings window");
		assert!(fifth.right() <= visual.debug_bounds("color-theme-picker").unwrap().right());
		assert!(sixth.top() >= first.bottom());

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(800.), gpui::px(700.)));
			window.draw(cx).clear(cx);
		});

		let before = visual.debug_bounds("color-theme-picker").expect("theme gallery");
		let last = visual.debug_bounds("color-theme-tokyo-night").expect("last theme");
		assert!(before.contains(&last.center()), "all themes use normal page layout");
		visual.simulate_event(gpui::ScrollEvent {
			position: before.center(),
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-80.))),
			..Default::default()
		});
		visual.update(crate::ui_frame::draw_current_frame);
		thread::sleep(std::time::Duration::from_millis(300));
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(
			visual.debug_bounds("color-theme-picker").unwrap().top() < before.top(),
			"scrolling over a theme must scroll the page immediately"
		);
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
			"color-theme-ayu-dark",
			"color-theme-catppuccin-mocha",
			"color-theme-dracula",
			"color-theme-everforest",
			"color-theme-github-dark",
			"color-theme-gruvbox-dark",
			"color-theme-kanagawa",
			"color-theme-nightfox",
			"color-theme-nord",
			"color-theme-one-dark",
			"color-theme-rose-pine",
			"color-theme-solarized-dark",
			"color-theme-tokyo-night",
		]) {
			scroll_to_theme(visual, selector);

			let bounds = visual.debug_bounds(selector).expect("theme card");

			assert!(
				(bounds.size.width - option_width).abs() <= gpui::px(1.),
				"{} width {:?} differs from {:?}",
				theme.name(),
				bounds.size.width,
				option_width
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

			visual.update(crate::ui_frame::draw_current_frame);
		}

		let (_, second) = visual.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});

		second.update(|window, cx| {
			assert_eq!(ui_theme::ColorTheme::configured(), ui_theme::ColorTheme::TokyoNight);

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
			assert_eq!(
				ui_theme::ColorScheme::current().color_theme(),
				ui_theme::ColorTheme::Decodex
			);
			assert_eq!(ui_theme::ColorScheme::current().hex(ui_theme::TEXT), 0x252c39);

			window.draw(cx).clear(cx);
		});
		visual.update(|_, cx| Appearance::Dark.apply(cx));
		visual.update(|window, cx| {
			assert_eq!(
				ui_theme::ColorScheme::current().color_theme(),
				ui_theme::ColorTheme::TokyoNight
			);
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

		assert_eq!(ui_theme::ColorTheme::configured(), ui_theme::ColorTheme::AyuDark);
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

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("menubar-surface-toggle").is_some());
		assert!(visual.debug_bounds("menubar-loading").is_none());
		assert!(visual.debug_bounds("automatic-recap-toggle").is_some());
		assert!(visual.debug_bounds("automatic-prompt-suggestions").is_none());
		assert!(visual.debug_bounds("recap-loading").is_none());
		assert!(visual.debug_bounds("loading-feedback-").is_none());
		surface.update(visual, |s, cx| {
			s.category = SettingsCategory::Agents;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("automatic-prompt-suggestions").is_some());
		assert!(visual.debug_bounds("service-tier-picker").is_some());
		assert!(visual.debug_bounds("prompt-suggestions").is_none());
	}

	#[gpui::test]
	fn quota_shows_loading_only_for_an_active_read(cx: &mut TestAppContext) {
		struct QuotaView(gpui::Entity<SettingsSurface>);

		impl Render for QuotaView {
			fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
				self.0.update(cx, |s, cx| s.quota_control(cx))
			}
		}

		let (view, visual) = cx.add_window_view(|_, cx| {
			QuotaView(
				cx.new(|cx| SettingsSurface::new(DesktopSettingsController::production(), cx)),
			)
		});
		let settings = view.read_with(visual, |view, _| view.0.clone());

		for load in [
			DesktopSettingsLoadState::NeverRequested,
			DesktopSettingsLoadState::Loading,
			DesktopSettingsLoadState::Offline,
		] {
			settings.update(visual, |s, cx| {
				s.snapshot.load = load;

				cx.notify();
			});

			visual.update(crate::ui_frame::draw_current_frame);

			assert_eq!(
				visual.debug_bounds("quota-request-pending").is_some(),
				load == DesktopSettingsLoadState::Loading
			);
			assert_eq!(
				visual.debug_bounds("quota-unavailable").is_some(),
				load != DesktopSettingsLoadState::Loading
			);
		}
	}

	#[gpui::test]
	fn automatic_recaps_change_without_a_service_connection(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});

		visual.update(|_, cx| assert!(!surface.read(cx).automatic_recaps_enabled()));

		for expected in [true, false] {
			visual.update(crate::ui_frame::draw_current_frame);

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
			visual.update(crate::ui_frame::draw_current_frame);

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

		visual.update(crate::ui_frame::draw_current_frame);

		let viewport = visual.debug_bounds("settings-scroll-viewport").unwrap();
		let before = visual.debug_bounds("prevent-system-sleep").unwrap();

		assert!(before.bottom() > viewport.bottom(), "before={before:?}, viewport={viewport:?}");

		visual.simulate_event(gpui::ScrollEvent {
			position: viewport.center(),
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-1_000.))),
			..Default::default()
		});
		visual.update(crate::ui_frame::draw_current_frame);

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
			s.detail = "Menu bar disabled.".into();

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

			visual.update(crate::ui_frame::draw_current_frame);

			assert!(visual.debug_bounds("menubar-runtime-status").is_none());
			assert_eq!(visual.debug_bounds("launch-at-login-toggle"), Some(original));
		}

		settings.update(visual, |s, cx| {
			s.snapshot.command = DesktopSettingsCommandState::Refused;
			s.preference_error = Some("The service refused the change.".into());

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("menubar-runtime-status").is_none());

		visual.update(|_, cx| {
			assert!(
				settings
					.read(cx)
					.notifications()
					.iter()
					.any(|(_, feedback)| feedback.message == "The service refused the change.")
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

		scroll_to_control(visual, "window-glass-Clear");

		let clear = visual.debug_bounds("window-glass-Clear").expect("Clear control");

		visual.simulate_click(clear.center(), Default::default());

		assert_eq!(GlassScope::Window.configured(), GlassStyle::Clear);

		let regular = visual.debug_bounds("window-glass-Regular").expect("Regular control");

		visual.simulate_click(regular.center(), Default::default());

		assert_eq!(GlassScope::Window.configured(), GlassStyle::Regular);
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

			visual.update(crate::ui_frame::draw_current_frame);
		}

		assert!(
			visual.debug_bounds("window-glass-Regular").is_none(),
			"glass style has no effect on Thin"
		);

		for target in [Surface::Window, Surface::InputBar] {
			let id = if target == Surface::Window {
				"window-material-Liquid Glass"
			} else {
				"input-bar-material-Liquid Glass"
			};
			let bounds = visual.debug_bounds(id).expect("material control");

			visual.simulate_click(bounds.center(), Default::default());
			visual.update(crate::ui_frame::draw_current_frame);
		}

		assert!(visual.debug_bounds("window-glass-Regular").is_some());
	}

	#[gpui::test]
	fn popover_glass_can_be_set_while_window_and_input_remain_thin(cx: &mut TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, cx| {
			Surface::Window.select(Material::Thin, cx);
			Surface::InputBar.select(Material::Thin, cx);
			Surface::Popover.select(Material::Thin, cx);
			GlassScope::Window.select(GlassStyle::Regular, cx);
			let mut settings = SettingsSurface::new(DesktopSettingsController::production(), cx);
			settings.category = SettingsCategory::Appearance;
			settings
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("popover-glass-Clear").is_none());
		scroll_to_control(visual, "popover-material-Liquid Glass");
		let material = visual.debug_bounds("popover-material-Liquid Glass").unwrap();
		visual.simulate_click(material.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		scroll_to_control(visual, "popover-glass-Clear");
		let clear = visual.debug_bounds("popover-glass-Clear").unwrap();
		visual.simulate_click(clear.center(), Default::default());
		assert_eq!(GlassScope::Popover.configured(), GlassStyle::Clear);
		assert_eq!(GlassScope::Window.configured(), GlassStyle::Regular);
		assert_eq!(Surface::Window.material(), Material::Thin);
		assert_eq!(Surface::InputBar.material(), Material::Thin);
		assert_eq!(Surface::Popover.material(), Material::LiquidGlass);
	}

	#[gpui::test]
	fn debug_mode_is_opt_in_and_can_be_disabled_again(cx: &mut TestAppContext) {
		assert!(!ui_performance::preference(None));

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

		assert!(ui_performance::preference(None));

		visual.update(crate::ui_frame::draw_current_frame);
		visual.simulate_click(toggle.center(), Default::default());

		assert!(!ui_performance::preference(None));
	}

	#[gpui::test]
	fn background_rendering_switch_applies_to_every_window(cx: &mut TestAppContext) {
		ui_frame::background_rendering(Some(false));

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

		assert!(ui_frame::background_rendering(None));

		visual.update(crate::ui_frame::draw_current_frame);
		visual.simulate_click(bounds.center(), Default::default());

		assert!(!ui_frame::background_rendering(None));
	}

	#[gpui::test]
	fn settings_draw_at_the_selected_desktop_size(cx: &mut TestAppContext) {
		let controller = DesktopSettingsController::production();
		let (_settings, visual) = cx.add_window_view(|_, cx| SettingsSurface::new(controller, cx));

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_490.0), gpui::px(1_055.0)));
			window.draw(cx).clear(cx);
		});
		assert!(visual.debug_bounds("updates-check").is_some(), "General owns the update action");
	}

	#[gpui::test]
	fn local_setting_failures_are_visible_beside_their_controls(cx: &mut TestAppContext) {
		let (settings, visual) = cx.add_window_view(|_, cx| {
			SettingsSurface::new(DesktopSettingsController::production(), cx)
		});

		settings.update(visual, |s, cx| {
			s.runtime = MenuBarRuntimeState::Unavailable;
			s.detail = "The menu bar could not be updated.".into();
			s.launch_at_login = LaunchAtLoginState::OperationFailed;
			s.launch_at_login_detail = "Startup could not be changed.".into();
			s.power.error = Some("The system setting could not be read.".into());

			cx.notify();
		});

		visual.update(|w, cx| {
			w.resize(gpui::size(gpui::px(920.), gpui::px(1_000.)));
			w.draw(cx).clear(cx);
		});

		for id in ["launch-at-login-feedback", "menubar-runtime-status", "power-settings-feedback"]
		{
			assert!(visual.debug_bounds(id).is_some(), "{id} must be visible at its setting");
		}
		for (notice, action) in [
			("launch-at-login-feedback", "launch-at-login-retry"),
			("power-settings-feedback", "power-settings-retry"),
		] {
			let notice = visual.debug_bounds(notice).unwrap();
			let action = visual.debug_bounds(action).unwrap();
			assert!(notice.contains(&action.center()), "recovery belongs inside its feedback row");
			assert_eq!(
				notice.center().y,
				action.center().y,
				"recovery actions align with the message"
			);
		}

		settings.read_with(visual, |s, _| {
			for (title, feedback) in s.notifications() {
				if matches!(title, "Launch at login" | "Menu bar" | "System sleep") {
					assert_eq!(feedback.tone, crate::ui_feedback::Tone::Error);
				}
			}
		});
	}
}
