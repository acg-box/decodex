//! A keyboard cue after the last line of a full-width suggestion.
use gpui::{
	AnyElement, App, AppContext as _, AvailableSpace, Context, Entity, InteractiveElement,
	IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, Point, Render, Role,
	SharedString, Size, StatefulInteractiveElement, Styled, WeakEntity, Window,
};

use crate::{
	composer_input::{ComposerEvent, ComposerInput},
	ui_loading, ui_motion,
	ui_theme::{self, BODY_LINE_HEIGHT, METADATA_GAP},
	workspace_symbols::{self, Symbol},
};

pub(super) struct InlineHint {
	pub element: AnyElement,
	pub size: Size<Pixels>,
}

impl InlineHint {
	pub(super) fn new(
		label: Option<SharedString>,
		busy: bool,
		input: Entity<ComposerInput>,
		window: &mut Window,
		cx: &mut App,
	) -> Option<Self> {
		let accept = button(
			"composer-suggestion-accept",
			"Use suggestion (Tab)",
			busy,
			&input,
			ComposerEvent::AcceptSuggestion,
			ui_theme::shortcut_keycap(label?)
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
			&input,
			ComposerEvent::RefreshSuggestion,
			gpui::div()
				.size(gpui::px(18.))
				.flex()
				.items_center()
				.justify_center()
				.rounded(gpui::px(4.))
				.child(refresh_icon)
				.into_any_element(),
		);
		let mut element = gpui::div()
			.flex()
			.items_center()
			.gap(gpui::px(METADATA_GAP))
			.child(accept)
			.child(refresh)
			.into_any_element();
		let size = element.layout_as_root(
			gpui::size(AvailableSpace::MinContent, AvailableSpace::MinContent),
			window,
			cx,
		);
		Some(Self { element, size })
	}

	pub(super) fn flow(
		size: Option<Size<Pixels>>,
		end: Point<Pixels>,
		text_height: Pixels,
		width: Pixels,
	) -> (Point<Pixels>, Pixels) {
		let Some(size) = size else { return (end, text_height) };
		let mut position = end + gpui::point(gpui::px(METADATA_GAP), gpui::px(0.));
		if position.x + size.width > width {
			position.x = gpui::px(0.);
			position.y += gpui::px(BODY_LINE_HEIGHT);
		}
		let height = text_height.max(position.y + gpui::px(BODY_LINE_HEIGHT));
		position.y += (gpui::px(BODY_LINE_HEIGHT) - size.height) / 2.;
		(position, height)
	}

	pub(super) fn prepaint(
		&mut self,
		end: Point<Pixels>,
		width: Pixels,
		origin: Point<Pixels>,
		window: &mut Window,
		cx: &mut App,
	) {
		let (position, _) = Self::flow(Some(self.size), end, gpui::px(0.), width);
		self.element.prepaint_at(origin + position, window, cx);
	}
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
		.min_h(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
		.min_w(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
		.flex()
		.items_center()
		.justify_center()
		.cursor_pointer()
		.rounded(gpui::px(4.))
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
		.tooltip(move |_, cx| cx.new(|_| HintTip(label)).into())
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

struct HintTip(&'static str);
impl Render for HintTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		ui_motion::tooltip_surface(gpui::div()).child(self.0)
	}
}
