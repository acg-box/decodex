//! Native editing commands for the focused input.
use gpui::{Action, Context, Window};
use objc2_app_kit::NSEventModifierFlags as Modifiers;

use crate::ui_native_menu::{self, Item};

use super::{ComposerInput, Copy, Cut, Delete, Paste, PastePlainText, Redo, SelectAll, Undo};

pub(super) fn show(input: &ComposerInput, window: &Window, cx: &Context<ComposerInput>) {
	let selected = !input.selected_range.is_empty();
	let can_copy = selected && !input.secret;
	let items: Vec<Item<Box<dyn Action>>> = vec![
		Item::command("Undo", "z", !input.undo.is_empty(), Box::new(Undo)),
		Item::Command {
			title: "Redo",
			key: "z",
			modifiers: Modifiers::Command | Modifiers::Shift,
			enabled: !input.redo.is_empty(),
			action: Box::new(Redo),
		},
		Item::Separator,
		Item::command("Cut", "x", can_copy, Box::new(Cut)),
		Item::command("Copy", "c", can_copy, Box::new(Copy)),
		Item::command("Paste", "v", true, Box::new(Paste)),
		Item::Command {
			title: "Paste as Plain Text",
			key: "v",
			modifiers: Modifiers::Command | Modifiers::Option | Modifiers::Shift,
			enabled: true,
			action: Box::new(PastePlainText),
		},
		Item::command("Delete", "", selected, Box::new(Delete)),
		Item::Separator,
		Item::command("Select All", "a", !input.content.is_empty(), Box::new(SelectAll)),
	];
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
