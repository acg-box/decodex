//! Host-local insertion cursor preferences and low-frequency blink lifecycle.
use super::ComposerInput;
use gpui::{App, Context, Subscription, Task, Window};
use std::{
	ops::Range,
	sync::atomic::{AtomicU8, Ordering},
	time::Duration,
};

static PREFERENCE: AtomicU8 = AtomicU8::new(u8::MAX);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
	Bar,
	Block,
	Underline,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Preference {
	pub shape: Shape,
	pub blinking: bool,
}
impl Preference {
	fn decode(value: u8) -> Self {
		Self {
			shape: match value & 3 {
				1 => Shape::Block,
				2 => Shape::Underline,
				_ => Shape::Bar,
			},
			blinking: value & 4 != 0,
		}
	}

	fn encode(self) -> u8 {
		(match self.shape {
			Shape::Bar => 0,
			Shape::Block => 1,
			Shape::Underline => 2,
		}) | if self.blinking { 4 } else { 0 }
	}

	pub fn configured() -> Self {
		let value = PREFERENCE.load(Ordering::Relaxed);
		if value != u8::MAX {
			return Self::decode(value);
		}
		let value = saved().unwrap_or(4);
		PREFERENCE.store(value, Ordering::Relaxed);
		Self::decode(value)
	}

	pub fn select(self, cx: &mut App) {
		PREFERENCE.store(self.encode(), Ordering::Relaxed);
		save(self.encode());
		cx.refresh_windows();
	}
}
#[cfg(all(target_os = "macos", not(test)))]
fn saved() -> Option<u8> {
	use objc2::{
		msg_send,
		rc::Retained,
		runtime::{AnyClass, AnyObject},
	};
	unsafe {
		let defaults: Retained<AnyObject> =
			msg_send![AnyClass::get(c"NSUserDefaults").expect("Foundation"), standardUserDefaults];
		let key = objc2_foundation::NSString::from_str("DecodexInsertionCursor");
		let value: Option<Retained<AnyObject>> = msg_send![&*defaults, objectForKey: &*key];
		value.map(|_| {
			let value: isize = msg_send![&*defaults, integerForKey: &*key];
			value as u8
		})
	}
}
#[cfg(all(target_os = "macos", not(test)))]
fn save(value: u8) {
	use objc2::{
		msg_send,
		rc::Retained,
		runtime::{AnyClass, AnyObject},
	};
	unsafe {
		let defaults: Retained<AnyObject> =
			msg_send![AnyClass::get(c"NSUserDefaults").expect("Foundation"), standardUserDefaults];
		let key = objc2_foundation::NSString::from_str("DecodexInsertionCursor");
		let _: () = msg_send![&*defaults, setInteger: isize::from(value), forKey: &*key];
	}
}
#[cfg(not(all(target_os = "macos", not(test))))]
fn saved() -> Option<u8> {
	None
}
#[cfg(not(all(target_os = "macos", not(test))))]
fn save(_: u8) {}

#[derive(Default)]
pub(super) struct Cursor {
	pub visible: bool,
	active: bool,
	selection: Range<usize>,
	length: usize,
	timer: Option<Task<()>>,
	window: Option<gpui::WindowId>,
	subscriptions: Vec<Subscription>,
}
impl Cursor {
	pub fn reset(&mut self) {
		self.visible = true;
		self.timer = None;
	}
}
pub(super) fn eligible(focused: bool, window_active: bool, selection_empty: bool) -> bool {
	focused && window_active && selection_empty
}
impl ComposerInput {
	pub(super) fn update_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.cursor.window != Some(window.window_handle().window_id()) {
			self.cursor.window = Some(window.window_handle().window_id());
			self.cursor.subscriptions = vec![
				cx.observe_window_activation(window, |_, _, cx| cx.notify()),
				cx.on_focus(&self.focus_handle, window, |s, _, cx| {
					s.cursor.reset();
					cx.notify();
				}),
				cx.on_blur(&self.focus_handle, window, |s, _, cx| {
					s.cursor.timer = None;
					s.cursor.visible = false;
					cx.notify();
				}),
			];
		}
		let active = eligible(
			self.focus_handle.is_focused(window),
			window.is_window_active(),
			self.selected_range.is_empty(),
		);
		let preference = Preference::configured();
		if active != self.cursor.active
			|| self.selected_range != self.cursor.selection
			|| self.content.len() != self.cursor.length
		{
			self.cursor.reset();
		}
		self.cursor.active = active;
		self.cursor.selection = self.selected_range.clone();
		self.cursor.length = self.content.len();
		if !active || !preference.blinking {
			self.cursor.timer = None;
			self.cursor.visible = active;
			return;
		}
		if self.cursor.timer.is_none() {
			self.cursor.timer = Some(cx.spawn(async |input, cx| {
				cx.background_executor().timer(Duration::from_millis(530)).await;
				let _ = input.update(cx, |s, cx| {
					s.cursor.timer = None;
					s.cursor.visible = !s.cursor.visible;
					cx.notify();
				});
			}));
		}
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn saved_cursor_choices_round_trip() {
		for shape in [Shape::Bar, Shape::Block, Shape::Underline] {
			for blinking in [false, true] {
				let value = Preference { shape, blinking };
				assert_eq!(Preference::decode(value.encode()), value);
			}
		}
	}
	#[test]
	fn retained_focus_in_an_inactive_window_never_shows_a_caret() {
		assert!(!eligible(true, false, true));
		assert!(!eligible(false, true, true));
		assert!(!eligible(true, true, false));
		assert!(eligible(true, true, true));
	}
}
