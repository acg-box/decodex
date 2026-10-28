//! Shared desktop creation defaults and session-setting choices.
use decodex_protocol::{PermissionMode, ServiceTier};
use gpui::{
	Context, Div, SharedString,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
use serde::{Deserialize, Serialize};

use crate::{ui_preferences, ui_theme};

const DEFAULTS_KEY: &str = "DecodexSessionDefaults";

/// Creation intent only. Existing native conversations remain authoritative.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Defaults {
	pub permissions: PermissionMode,
	pub speed: Option<ServiceTier>,
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

pub(crate) fn permission_choices<T: 'static>(
	selected: Option<PermissionMode>,
	available: impl Fn(PermissionMode) -> bool,
	choose: fn(&mut T, PermissionMode, &mut Context<T>),
	cx: &Context<T>,
) -> Div {
	let mut menu = gpui::div().flex().flex_col().gap_1();
	for (index, mode) in PermissionMode::ALL.into_iter().enumerate() {
		let enabled = available(mode);
		let chosen = selected == Some(mode);
		menu = menu.child(
			gpui::div()
				.id(SharedString::from(format!("permission-choice-{index}")))
				.debug_selector(move || format!("permission-choice-{index}"))
				.role(gpui::Role::Button)
				.aria_label(format!("{}: {}", mode.label(), mode.description()))
				.aria_selected(chosen)
				.tab_index(if enabled { 0 } else { -1 })
				.px_3()
				.py_2()
				.rounded_lg()
				.flex()
				.flex_col()
				.gap_1()
				.when(enabled, |d| {
					d.cursor_pointer().hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
				})
				.when(!enabled, |d| d.opacity(0.4))
				.child(gpui::div().text_color(ui_theme::rgb(ui_theme::TEXT)).child(format!(
					"{}{}",
					if chosen { "✓ " } else { "" },
					mode.label()
				)))
				.child(
					gpui::div()
						.text_size(gpui::px(11.))
						.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
						.child(mode.description()),
				)
				.on_click(cx.listener(move |this, _, _, cx| {
					if enabled && !chosen {
						choose(this, mode, cx);
					}
				}))
				.on_key_down(cx.listener(move |this, key: &gpui::KeyDownEvent, _, cx| {
					if enabled
						&& !chosen
						&& ["enter", "space"].contains(&key.keystroke.key.as_str())
					{
						cx.stop_propagation();
						choose(this, mode, cx);
					}
				})),
		);
	}
	menu
}

pub(crate) fn speed_choices<T: 'static>(
	selected: Option<&ServiceTier>,
	fast_available: bool,
	choose: fn(&mut T, ServiceTier, &mut Context<T>),
	cx: &Context<T>,
) -> Div {
	let mut menu = gpui::div().flex().items_center().gap_1();
	for (id, name, available) in [
		(ServiceTier::standard(), "Standard", true),
		(ServiceTier::from_fast(true), "Fast", fast_available),
	] {
		let chosen = selected == Some(&id);
		let keyboard_id = id.clone();
		menu = menu.child(
			gpui::div()
				.id(SharedString::from(format!("tier-{}", id.as_str())))
				.debug_selector({
					let id = id.clone();
					move || format!("tier-{}", id.as_str())
				})
				.role(gpui::Role::Button)
				.aria_label(name)
				.aria_selected(chosen)
				.tab_index(if available { 0 } else { -1 })
				.px_3()
				.py_2()
				.rounded_lg()
				.when(available, |d| {
					d.cursor_pointer().hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
				})
				.when(!available, |d| d.opacity(0.4))
				.child(format!("{}{}", if chosen { "✓ " } else { "" }, name))
				.on_click(cx.listener(move |this, _, _, cx| {
					if available && !chosen {
						choose(this, id.clone(), cx);
					}
				}))
				.on_key_down(cx.listener(move |this, key: &gpui::KeyDownEvent, _, cx| {
					if available
						&& !chosen
						&& ["enter", "space"].contains(&key.keystroke.key.as_str())
					{
						cx.stop_propagation();
						choose(this, keyboard_id.clone(), cx);
					}
				})),
		);
	}
	menu
}
