//! Shared desktop creation defaults and session-setting choices.
use decodex_protocol::{PermissionMode, ServiceTier};
use gpui::{
	Context, Div,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
use serde::{Deserialize, Serialize};

use crate::{shell::workspace_symbols, ui_motion::SmoothControl as _, ui_preferences, ui_theme};

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

pub(crate) fn permission_choices<T: 'static>(
	selected: Option<PermissionMode>,
	available: impl Fn(PermissionMode) -> bool,
	choose: fn(&mut T, PermissionMode, &mut Context<T>),
	cx: &Context<T>,
) -> Div {
	let mut menu = gpui::div().flex().flex_col().gap(gpui::px(1.));
	for (index, mode) in PermissionMode::ALL.into_iter().enumerate() {
		let enabled = available(mode);
		let chosen = selected == Some(mode);
		menu = menu.child(
			ui_theme::menu_row(format!("permission-choice-{index}"), enabled)
				.debug_selector(move || format!("permission-choice-{index}"))
				.role(gpui::Role::Button)
				.aria_label(format!("{}: {}", mode.label(), mode.description()))
				.aria_selected(chosen)
				.px(gpui::px(8.))
				.py(gpui::px(4.))
				.flex()
				.items_center()
				.gap(gpui::px(8.))
				.when(!enabled, |d| d.opacity(0.4))
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
						.child(
							gpui::div()
								.line_height(gpui::px(16.))
								.whitespace_nowrap()
								.child(mode.label()),
						)
						.child(
							gpui::div()
								.debug_selector(move || format!("permission-description-{index}"))
								.text_size(gpui::px(11.))
								.line_height(gpui::px(14.))
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
				.on_click(cx.listener(move |this, _, _, cx| {
					if enabled && !chosen {
						choose(this, mode, cx);
					}
				}))
				.on_key_down(cx.listener(move |this, key: &gpui::KeyDownEvent, _, cx| {
					if enabled
						&& !chosen
						&& !key.is_held
						&& ["enter", "space"].contains(&key.keystroke.key.as_str())
					{
						cx.stop_propagation();
						choose(this, mode, cx);
					}
				}))
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
) -> Div {
	let mut menu = gpui::div().flex().flex_wrap().items_center().gap(gpui::px(2.));
	let choices = std::iter::once((ServiceTier::standard(), "Standard".into()))
		.chain(tiers.into_iter().filter(|(id, _)| id.as_str() != "default"));
	for (id, name) in choices {
		let label = match id.as_str() {
			"default" | "priority" | "ultrafast" => speed_label(id.as_str()).to_owned(),
			_ => name,
		};
		let chosen = selected == Some(&id);
		let keyboard_id = id.clone();
		menu = menu.child(
			ui_theme::menu_row(format!("tier-{}", id.as_str()), true)
				.w_auto()
				.flex_shrink_0()
				.debug_selector({
					let id = id.clone();
					move || format!("tier-{}", id.as_str())
				})
				.role(gpui::Role::Button)
				.aria_label(label.clone())
				.aria_selected(chosen)
				.h(gpui::px(24.))
				.min_h(gpui::px(24.))
				.px(gpui::px(6.))
				.py_0()
				.rounded(gpui::px(5.))
				.flex()
				.items_center()
				.whitespace_nowrap()
				.when(chosen, |d| d.bg(ui_theme::rgba(ui_theme::PRESSED_FILL)))
				.child(label)
				.on_click(cx.listener(move |this, _, _, cx| {
					if !chosen {
						choose(this, id.clone(), cx);
					}
				}))
				.on_key_down(cx.listener(move |this, key: &gpui::KeyDownEvent, _, cx| {
					if !chosen
						&& !key.is_held
						&& ["enter", "space"].contains(&key.keystroke.key.as_str())
					{
						cx.stop_propagation();
						choose(this, keyboard_id.clone(), cx);
					}
				}))
				.smooth(),
		);
	}
	menu
}

pub(crate) fn speed_label(id: &str) -> &str {
	match id {
		"default" => "Standard",
		"priority" => "Fast",
		"ultrafast" => "Ultrafast",
		_ => id,
	}
}
