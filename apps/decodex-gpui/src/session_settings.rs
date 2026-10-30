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
	compact: bool,
	available: impl Fn(PermissionMode) -> bool,
	choose: fn(&mut T, PermissionMode, &mut Context<T>),
	cx: &Context<T>,
) -> Div {
	let mut menu = gpui::div().flex().flex_col().gap_1();
	for (index, mode) in PermissionMode::ALL.into_iter().enumerate() {
		let enabled = available(mode);
		let chosen = selected == Some(mode);
		menu = menu.child(
			ui_theme::menu_row(format!("permission-choice-{index}"), enabled)
				.debug_selector(move || format!("permission-choice-{index}"))
				.role(gpui::Role::Button)
				.aria_label(format!("{}: {}", mode.label(), mode.description()))
				.aria_selected(chosen)
				.px_3()
				.py(gpui::px(if compact { 6. } else { 8. }))
				.rounded_lg()
				.flex()
				.flex_col()
				.gap_1()
				.when(!enabled, |d| d.opacity(0.4))
				.child(choice_label(mode.label(), chosen))
				.when(!compact, |row| {
					row.child(
						gpui::div()
							.text_size(gpui::px(11.))
							.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
							.child(mode.description()),
					)
				})
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
			ui_theme::menu_row(format!("tier-{}", id.as_str()), available)
				.w_auto()
				.debug_selector({
					let id = id.clone();
					move || format!("tier-{}", id.as_str())
				})
				.role(gpui::Role::Button)
				.aria_label(name)
				.aria_selected(chosen)
				.px_3()
				.py_2()
				.rounded_lg()
				.when(!available, |d| d.opacity(0.4))
				.child(choice_label(name, chosen))
				.on_click(cx.listener(move |this, _, _, cx| {
					if available && !chosen {
						choose(this, id.clone(), cx);
					}
				}))
				.on_key_down(cx.listener(move |this, key: &gpui::KeyDownEvent, _, cx| {
					if available
						&& !chosen
						&& !key.is_held
						&& ["enter", "space"].contains(&key.keystroke.key.as_str())
					{
						cx.stop_propagation();
						choose(this, keyboard_id.clone(), cx);
					}
				}))
				.smooth()
				.enabled(available),
		);
	}
	menu
}

fn choice_label(label: &'static str, selected: bool) -> Div {
	gpui::div()
		.flex()
		.items_center()
		.gap(gpui::px(8.))
		.child(gpui::div().flex_1().min_w_0().whitespace_nowrap().child(label))
		.child(workspace_symbols::menu_check(selected))
}
