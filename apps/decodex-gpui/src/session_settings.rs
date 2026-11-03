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

pub(crate) fn permission_picker<T: 'static>(
	selected: PermissionMode,
	choose: fn(&mut T, PermissionMode, &mut Context<T>),
	cx: &Context<T>,
) -> gpui::Stateful<Div> {
	gpui::div()
		.id("default-permissions")
		.debug_selector(|| "default-permissions".into())
		.w_full()
		.min_w_0()
		.flex_none()
		.child(permission_rows(Some(selected), |_| true, choose, true, cx))
}

/// Settings explain each speed; the composer keeps its compact segmented control.
pub(crate) fn speed_default_choices<T: 'static>(
	selected: Option<&ServiceTier>,
	choose: fn(&mut T, ServiceTier, &mut Context<T>),
	cx: &Context<T>,
) -> gpui::Stateful<Div> {
	let mut choices = gpui::div()
		.id("default-speeds")
		.debug_selector(|| "default-speeds".into())
		.w_full()
		.min_w_0()
		.flex_none()
		.flex()
		.flex_col()
		.gap(gpui::px(1.));
	// ChatGPT usage relative to Standard: https://learn.chatgpt.com/docs/pricing
	for (id, description) in [
		("default", "1× included usage · 1× credits."),
		("priority", "2.5× included usage · 2× credits."),
		("ultrafast", "8× included usage · 6× credits."),
	] {
		let tier = ServiceTier::new(id).expect("built-in speed");
		let key_tier = tier.clone();
		let chosen = selected == Some(&tier);
		choices = choices.child(
			ui_theme::menu_row(format!("default-speed-{id}"), true)
				.debug_selector(move || format!("default-speed-{id}"))
				.aria_label(format!("{}: {description}", speed_label(id)))
				.role(gpui::Role::RadioButton)
				.aria_selected(chosen)
				.px(gpui::px(8.))
				.py(gpui::px(4.))
				.flex()
				.items_center()
				.gap(gpui::px(8.))
				.when(chosen, |row| row.bg(ui_theme::rgba(ui_theme::CONTROL_SELECTED_FILL)))
				.child(ui_theme::settings_label(speed_label(id), description))
				.child(workspace_symbols::menu_check(chosen))
				.on_click(cx.listener(move |this, _, _, cx| {
					if !chosen {
						choose(this, tier.clone(), cx);
					}
				}))
				.on_key_down(cx.listener(move |this, key: &gpui::KeyDownEvent, _, cx| {
					if !key.is_held && ["enter", "space"].contains(&key.keystroke.key.as_str()) {
						cx.stop_propagation();
						if !chosen {
							choose(this, key_tier.clone(), cx);
						}
					}
				}))
				.smooth(),
		);
	}
	choices
}

pub(crate) fn permission_choices<T: 'static>(
	selected: Option<PermissionMode>,
	available: impl Fn(PermissionMode) -> bool,
	choose: fn(&mut T, PermissionMode, &mut Context<T>),
	cx: &Context<T>,
) -> Div {
	permission_rows(selected, available, choose, false, cx)
}

fn permission_rows<T: 'static>(
	selected: Option<PermissionMode>,
	available: impl Fn(PermissionMode) -> bool,
	choose: fn(&mut T, PermissionMode, &mut Context<T>),
	settings: bool,
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
						.when(settings, |d| d.gap(gpui::px(2.)))
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
