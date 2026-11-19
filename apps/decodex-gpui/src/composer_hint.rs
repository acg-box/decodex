//! Compact actions for the current composer suggestion.
use crate::ui_motion::HoverHint as _;
use gpui::{
	AnyElement, App, AvailableSpace, Div, Entity, InteractiveElement, IntoElement, KeyDownEvent,
	MouseButton, ParentElement, Pixels, Role, Size, StatefulInteractiveElement, Styled, WeakEntity,
	Window,
};

use crate::{
	composer_input::{ComposerEvent, ComposerInput},
	ui_loading,
	ui_theme::{self, CHROME_CONTROL_SIZE, CONTROL_GAP, SMALL_CONTROL_RADIUS},
	workspace_symbols::{self, Symbol},
};

pub(super) struct SuggestionActions {
	pub element: AnyElement,
	pub size: Size<Pixels>,
}

impl SuggestionActions {
	pub(super) fn measure(
		input: &Entity<ComposerInput>,
		window: &mut Window,
		cx: &mut App,
	) -> Option<Self> {
		let state = input.read(cx);
		let mut element =
			suggestion_actions(state.placeholder_hint()?, state.placeholder_hint_busy, input)
				.into_any_element();
		let size = element.layout_as_root(
			gpui::size(AvailableSpace::MinContent, AvailableSpace::MinContent),
			window,
			cx,
		);
		Some(Self { element, size })
	}
}

fn suggestion_actions(label: &str, busy: bool, input: &Entity<ComposerInput>) -> Div {
	let accept = button(
		"composer-suggestion-accept",
		"Use suggestion (Tab)",
		busy,
		input,
		ComposerEvent::AcceptSuggestion,
		ui_theme::shortcut_keycap(label)
			.debug_selector(|| "composer-suggestion-keycap".into())
			.into_any_element(),
	);
	let refresh_icon = if busy {
		ui_loading::progress("Generating another suggestion").into_any_element()
	} else {
		workspace_symbols::icon_sized(Symbol::ResetCards, 12.)
	};
	let refresh = button(
		"composer-suggestion-refresh",
		if busy { "Generating another suggestion" } else { "Generate another suggestion" },
		busy,
		input,
		ComposerEvent::RefreshSuggestion,
		refresh_icon,
	);
	gpui::div()
		.flex_none()
		.flex()
		.items_center()
		.gap(gpui::px(CONTROL_GAP / 2.))
		.child(accept)
		.child(refresh)
}

fn button(
	id: &'static str,
	label: &'static str,
	busy: bool,
	input: &Entity<ComposerInput>,
	action: ComposerEvent,
	child: AnyElement,
) -> AnyElement {
	let click_input = input.downgrade();
	let key_input = click_input.clone();
	let key_action = action.clone();
	gpui::div()
		.id(id)
		.debug_selector(move || id.into())
		.role(Role::Button)
		.aria_label(label)
		.tab_index(if busy { -1 } else { 0 })
		.flex_none()
		.h(gpui::px(CHROME_CONTROL_SIZE))
		.min_w(gpui::px(CHROME_CONTROL_SIZE))
		.flex()
		.items_center()
		.justify_center()
		.cursor_pointer()
		.rounded(gpui::px(SMALL_CONTROL_RADIUS))
		.opacity(if busy { 0.5 } else { 1. })
		.hover(|style| style.bg(ui_theme::rgb(ui_theme::TEXT).opacity(0.08)))
		.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
		.on_click(move |_, window, cx| {
			if !busy {
				emit(&click_input, action.clone(), window, cx);
			}
			cx.stop_propagation();
		})
		.on_key_down(move |event: &KeyDownEvent, window, cx| {
			if !event.is_held && matches!(event.keystroke.key.as_str(), "enter" | "space") {
				if !busy {
					emit(&key_input, key_action.clone(), window, cx);
				}
				cx.stop_propagation();
			}
		})
		.hover_hint(label)
		.child(child)
		.into_any_element()
}

fn emit(
	input: &WeakEntity<ComposerInput>,
	event: ComposerEvent,
	window: &mut Window,
	cx: &mut App,
) {
	if let Some(input) = input.upgrade() {
		#[cfg(target_os = "macos")]
		super::claim_native_text_focus(window);
		window.focus(&input.read(cx).focus_handle.clone(), cx);
		input.update(cx, |_, cx| cx.emit(event));
	}
}
