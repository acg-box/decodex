//! Shared desktop creation defaults and session-setting choices.
use std::rc::Rc;

use decodex_protocol::{PermissionMode, ServiceTier};
use gpui::{
	App, Context, Div, Entity, FocusHandle, KeyBinding, RenderOnce, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
use serde::{Deserialize, Serialize};

use crate::{
	shell::workspace_symbols,
	ui_motion::{self, SmoothControl as _},
	ui_preferences, ui_theme,
};

const DEFAULTS_KEY: &str = "DecodexSessionDefaults";

/// Creation intent only. Existing native conversations remain authoritative.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Defaults {
	pub permissions: PermissionMode,
}
impl Defaults {
	pub(crate) fn load() -> Self {
		ui_preferences::string(DEFAULTS_KEY, None)
			.and_then(|s| serde_json::from_str(&s).ok())
			.unwrap_or_default()
	}

	pub(crate) fn save(&self) {
		let value = serde_json::to_string(self).expect("session defaults");
		ui_preferences::string(DEFAULTS_KEY, Some(&value));
	}
}

#[cfg(not(test))]
pub(crate) use decodex_protocol::{
	codex_default_service_tier as default_speed,
	set_codex_default_service_tier as set_default_speed,
};

#[cfg(test)]
pub(crate) fn default_speed() -> Result<ServiceTier, decodex_protocol::CodexConfigError> {
	let value = ui_preferences::string("test-shared-codex-speed", None);
	value.map_or_else(
		|| Ok(ServiceTier::standard()),
		|s| ServiceTier::new(s).map_err(|_| decodex_protocol::CodexConfigError::ServiceTierInvalid),
	)
}

#[cfg(test)]
pub(crate) fn set_default_speed(
	tier: &ServiceTier,
) -> Result<(), decodex_protocol::CodexConfigError> {
	ui_preferences::string("test-shared-codex-speed", Some(tier.as_str()));
	Ok(())
}

gpui::actions!(decodex_settings, [DismissPermissionMenu]);
const PERMISSION_MENU_CONTEXT: &str = "PermissionMenu";
type PermissionChange = dyn Fn(PermissionMode, &mut Window, &mut App);

pub(crate) fn bind_keys(cx: &mut App) {
	cx.bind_keys([KeyBinding::new("escape", DismissPermissionMenu, Some(PERMISSION_MENU_CONTEXT))]);
}

#[derive(gpui::IntoElement)]
pub(crate) struct PermissionPicker {
	selected: PermissionMode,
	choose: Rc<PermissionChange>,
}
struct PermissionPickerState {
	open: bool,
	trigger_focus: FocusHandle,
	option_focus: [FocusHandle; 4],
	anchor: Option<gpui::Bounds<gpui::Pixels>>,
}

pub(crate) fn permission_picker<T: 'static>(
	selected: PermissionMode,
	choose: fn(&mut T, PermissionMode, &mut Context<T>),
	cx: &Context<T>,
) -> PermissionPicker {
	let change = cx.listener(move |this, mode: &PermissionMode, _, cx| choose(this, *mode, cx));
	PermissionPicker {
		selected,
		choose: Rc::new(move |mode, window, cx| change(&mode, window, cx)),
	}
}

impl RenderOnce for PermissionPicker {
	fn render(self, window: &mut Window, cx: &mut App) -> impl gpui::IntoElement {
		let state = window.use_keyed_state("default-permission-picker", cx, |_, cx| {
			PermissionPickerState {
				open: false,
				trigger_focus: cx.focus_handle(),
				option_focus: std::array::from_fn(|_| cx.focus_handle()),
				anchor: None,
			}
		});
		let (open, trigger_focus, option_focus) = {
			let s = state.read(cx);
			(s.open, s.trigger_focus.clone(), s.option_focus.clone())
		};
		let click = state.clone();
		let key = state.clone();
		let measured = state.clone();
		let outside = state.clone();
		let dismiss = state.clone();
		let next = state.clone();
		let previous = state.clone();
		let navigation = state.clone();
		let selected = state.clone();
		let mode = self.selected;
		let choose = self.choose;
		let change: Rc<PermissionChange> = Rc::new(move |mode, window, cx| {
			choose(mode, window, cx);
			close_permission_picker(&selected, window, cx);
		});
		let trigger = ui_theme::settings_button("default-permissions", self.selected.label())
			.aria_label(format!("Default permissions: {}", self.selected.label()))
			.aria_expanded(open)
			.track_focus(&trigger_focus)
			.w(gpui::px(174.))
			.h(gpui::px(ui_theme::CONTROL_GROUP_HEIGHT))
			.justify_between()
			.child(workspace_symbols::icon_sized(workspace_symbols::Symbol::ChevronDown, 10.))
			.on_click(move |_, window, cx| toggle_permission_picker(&click, mode, window, cx))
			.on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
				if !event.is_held
					&& ["enter", "space", "down"].contains(&event.keystroke.key.as_str())
				{
					toggle_permission_picker(&key, mode, window, cx);
					cx.stop_propagation();
				}
			});
		let menu = gpui::div()
			.id("default-permissions-menu")
			.role(gpui::Role::Menu)
			.aria_label("Default permissions")
			.key_context(PERMISSION_MENU_CONTEXT)
			.tab_group()
			.occlude()
			.p(gpui::px(ui_theme::POPOVER_CONTENT_PADDING))
			.on_action(move |_: &DismissPermissionMenu, window, cx| {
				close_permission_picker(&dismiss, window, cx);
				cx.stop_propagation();
			})
			.on_action(move |_: &crate::shell::FocusNext, window, cx| {
				close_permission_picker(&next, window, cx);
				window.focus_next(cx);
				cx.stop_propagation();
			})
			.on_action(move |_: &crate::shell::FocusPrevious, window, cx| {
				close_permission_picker(&previous, window, cx);
				window.focus_prev(cx);
				cx.stop_propagation();
			})
			.on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
				let direction = match event.keystroke.key.as_str() {
					"down" => 1,
					"up" => -1,
					_ => return,
				};
				let s = navigation.read(cx);
				let index =
					s.option_focus.iter().position(|focus| focus.is_focused(window)).unwrap_or(0);
				let next =
					(index as isize + direction).rem_euclid(s.option_focus.len() as isize) as usize;
				let focus = s.option_focus[next].clone();
				window.focus(&focus, cx);
				cx.stop_propagation();
			})
			.on_mouse_down_out(move |event: &gpui::MouseDownEvent, _, cx| {
				if !outside.read(cx).anchor.is_some_and(|bounds| bounds.contains(&event.position)) {
					outside.update(cx, |s, cx| {
						s.open = false;
						cx.notify();
					});
				}
			})
			.child(permission_rows(Some(self.selected), |_| true, change, Some(&option_focus)))
			.child(
				ui_theme::settings_description("Subagents inherit their parent’s permissions.")
					.px(gpui::px(8.))
					.pt(gpui::px(8.)),
			);
		gpui::div()
			.relative()
			.flex_none()
			.on_children_prepainted(move |children, _, cx| {
				measured.update(cx, |s, _| s.anchor = children.first().copied())
			})
			.child(trigger)
			.child(
				gpui::deferred(
					gpui::div()
						.absolute()
						.top(gpui::px(ui_theme::CONTROL_GROUP_HEIGHT))
						// Align the painted surface, excluding the shared shadow gutter.
						.right(gpui::px(-ui_theme::POPOVER_PADDING))
						.w(gpui::px(330. + ui_theme::POPOVER_PADDING * 2.))
						.child(ui_motion::popover("default-permissions-popover", open, menu)),
				)
				.priority(3),
			)
	}
}

fn toggle_permission_picker(
	state: &Entity<PermissionPickerState>,
	selected: PermissionMode,
	window: &mut Window,
	cx: &mut App,
) {
	state.update(cx, |s, cx| {
		s.open = !s.open;
		let index = PermissionMode::ALL
			.iter()
			.position(|mode| *mode == selected)
			.expect("built-in permission mode");
		window.focus(if s.open { &s.option_focus[index] } else { &s.trigger_focus }, cx);
		cx.notify();
	});
}
fn close_permission_picker(
	state: &Entity<PermissionPickerState>,
	window: &mut Window,
	cx: &mut App,
) {
	state.update(cx, |s, cx| {
		s.open = false;
		window.focus(&s.trigger_focus, cx);
		cx.notify();
	});
}

pub(crate) fn permission_choices<T: 'static>(
	selected: Option<PermissionMode>,
	available: impl Fn(PermissionMode) -> bool,
	choose: fn(&mut T, PermissionMode, &mut Context<T>),
	cx: &Context<T>,
) -> Div {
	let change = cx.listener(move |this, mode: &PermissionMode, _, cx| choose(this, *mode, cx));
	permission_rows(
		selected,
		available,
		Rc::new(move |mode, window, cx| change(&mode, window, cx)),
		None,
	)
}

fn permission_rows(
	selected: Option<PermissionMode>,
	available: impl Fn(PermissionMode) -> bool,
	choose: Rc<PermissionChange>,
	option_focus: Option<&[FocusHandle; 4]>,
) -> Div {
	let settings = option_focus.is_some();
	let mut menu = gpui::div().flex().flex_col().gap(gpui::px(1.));
	for (index, mode) in PermissionMode::ALL.into_iter().enumerate() {
		let enabled = available(mode);
		let chosen = selected == Some(mode);
		let click = choose.clone();
		let key = choose.clone();
		menu = menu.child(
			ui_theme::menu_row(format!("permission-choice-{index}"), enabled)
				.debug_selector(move || format!("permission-choice-{index}"))
				.role(gpui::Role::Button)
				.aria_label(format!("{}: {}", mode.label(), mode.description()))
				.aria_selected(chosen)
				.when_some(option_focus.map(|focus| &focus[index]), |row, focus| {
					row.track_focus(focus)
				})
				.px(gpui::px(8.))
				.py(gpui::px(4.))
				.flex()
				.items_center()
				.gap(gpui::px(8.))
				.when(!enabled, |d| d.opacity(0.4))
				.when(settings && chosen, |d| d.bg(ui_theme::rgba(ui_theme::CONTROL_SELECTED_FILL)))
				.when(chosen && mode == PermissionMode::FullAccess, |d| {
					d.text_color(ui_theme::rgb(ui_theme::WARNING))
				})
				.child(workspace_symbols::icon_sized(permission_symbol(mode), 16.))
				.child(
					gpui::div()
						.flex_1()
						.min_w_0()
						.flex()
						.flex_col()
						.when(settings, |label| label.gap(gpui::px(2.)))
						.child(
							gpui::div()
								.line_height(gpui::px(16.))
								.when(settings, |d| {
									d.text_size(gpui::px(ui_theme::SETTINGS_LABEL_SIZE))
										.line_height(gpui::px(18.))
								})
								.whitespace_nowrap()
								.child(mode.label()),
						)
						.child(
							gpui::div()
								.debug_selector(move || format!("permission-description-{index}"))
								.text_size(gpui::px(11.))
								.line_height(gpui::px(14.))
								.when(settings, |d| {
									d.text_size(gpui::px(ui_theme::SETTINGS_HELP_SIZE))
										.line_height(gpui::px(16.5))
								})
								.whitespace_nowrap()
								.text_color(ui_theme::rgb(
									if chosen && mode == PermissionMode::FullAccess {
										ui_theme::WARNING
									} else {
										ui_theme::TEXT_MUTED
									},
								))
								.child(mode.description()),
						),
				)
				.child(workspace_symbols::menu_check(chosen))
				.on_click(move |_, window, cx| {
					if enabled && (settings || !chosen) {
						click(mode, window, cx);
					}
				})
				.on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
					if enabled
						&& (settings || !chosen)
						&& !event.is_held
						&& ["enter", "space"].contains(&event.keystroke.key.as_str())
					{
						cx.stop_propagation();
						key(mode, window, cx);
					}
				})
				.smooth()
				.enabled(enabled),
		);
	}
	menu
}

pub(crate) fn permission_symbol(mode: PermissionMode) -> workspace_symbols::Symbol {
	use workspace_symbols::Symbol;
	match mode {
		PermissionMode::AskForApproval => Symbol::Shield,
		PermissionMode::ApproveForMe => Symbol::ShieldCheck,
		PermissionMode::FullAccess => Symbol::ShieldSlash,
		PermissionMode::ReadOnly => Symbol::ShieldLock,
	}
}

pub(crate) fn speed_choices<T: 'static>(
	selected: Option<&ServiceTier>,
	tiers: impl IntoIterator<Item = (ServiceTier, String)>,
	choose: fn(&mut T, ServiceTier, &mut Context<T>),
	cx: &Context<T>,
) -> crate::ui_motion::Segmented {
	let mut tiers: Vec<_> = std::iter::once((ServiceTier::standard(), "Standard".into()))
		.chain(tiers.into_iter().filter(|(id, _)| id.as_str() != "default"))
		.collect();
	tiers.sort_by_key(|(id, _)| match id.as_str() {
		"default" => 0,
		"priority" => 1,
		"ultrafast" => 2,
		_ => 3,
	});
	let selected = tiers.iter().position(|(tier, _)| Some(tier) == selected);
	let choices: Vec<_> = tiers
		.iter()
		.map(|(id, name)| {
			let label = match id.as_str() {
				"default" | "priority" | "ultrafast" => speed_label(id.as_str()),
				_ => name.as_str(),
			};
			crate::ui_motion::Choice::new(
				format!("tier-{}", id.as_str()),
				label.to_owned(),
				label.to_owned(),
			)
		})
		.collect();
	crate::ui_motion::Segmented::new(
		"service-tier-picker",
		"Speed. Click a speed or drag between speeds.",
		choices,
		selected,
		cx.listener(move |this, index: &usize, _, cx| {
			choose(this, tiers[*index].0.clone(), cx);
		}),
	)
	.fixed_choice_width(64.)
}

pub(crate) fn speed_symbol(id: &str) -> Option<workspace_symbols::Symbol> {
	match id {
		"priority" => Some(workspace_symbols::Symbol::Fast),
		"ultrafast" => Some(workspace_symbols::Symbol::Ultrafast),
		_ => None,
	}
}

pub(crate) fn speed_label(id: &str) -> &str {
	match id {
		"default" => "Standard",
		"priority" => "Fast",
		"ultrafast" => "Ultrafast",
		_ => id,
	}
}
