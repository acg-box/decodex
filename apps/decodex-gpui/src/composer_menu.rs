//! Native editing commands for the focused input.
use gpui::{Action, Context, Window};

use crate::ui_native_menu::{self, Item};

use super::{ComposerInput, Copy, Cut, Delete, Paste, PastePlainText, Redo, SelectAll, Undo};

pub(super) fn show(input: &ComposerInput, window: &Window, cx: &Context<ComposerInput>) {
	let selected = !input.selected_range.is_empty();
	let can_copy = selected && !input.secret;
	let mut items: Vec<Item<Box<dyn Action>>> = Vec::new();
	let mut add = |title, enabled, action: Box<dyn Action>| {
		let item = Item::bound(title, &*action, cx, enabled, action.boxed_clone());
		items.push(item);
	};
	add("Undo", !input.undo.is_empty(), Box::new(Undo));
	add("Redo", !input.redo.is_empty(), Box::new(Redo));
	add("Cut", can_copy, Box::new(Cut));
	add("Copy", can_copy, Box::new(Copy));
	add("Paste", true, Box::new(Paste));
	add("Paste as Plain Text", true, Box::new(PastePlainText));
	add("Delete", selected, Box::new(Delete));
	add("Select All", !input.content.is_empty(), Box::new(SelectAll));
	if can_copy && window.is_action_available(&crate::keyboard_shortcuts::OpenSideChat, cx) {
		items.push(Item::Separator);
		items.push(Item::bound(
			"Ask in side chat",
			&crate::keyboard_shortcuts::OpenSideChat,
			cx,
			true,
			Box::new(crate::keyboard_shortcuts::OpenSideChat),
		));
	}
	let owner = cx.entity().downgrade();
	let handle = window.window_handle();
	ui_native_menu::show(window, cx, items, move |action, cx| {
		let _ = handle.update(cx, |_, window, cx| {
			if owner.upgrade().is_some_and(|input| input.read(cx).focus_handle.is_focused(window)) {
				window.dispatch_action(action, cx);
			}
		});
	});
}
