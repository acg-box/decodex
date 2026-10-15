//! Insertion cursor appearance, bounded motion, and an idle blink timer.
#[path = "composer_cursor_motion.rs"] mod motion;

use std::{
	ops::Range,
	sync::atomic::{AtomicU8, Ordering},
	time::{Duration, Instant},
};

use gpui::{App, Bounds, Context, Pixels, Point, Subscription, Task, Window, WindowId};
#[cfg(all(target_os = "macos", not(test)))]
use objc2::{
	rc::Retained,
	runtime::{AnyClass, AnyObject},
};

use crate::{composer_input::ComposerInput, ui_motion, ui_theme};

static PREFERENCE: AtomicU8 = AtomicU8::new(u8::MAX);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Shape {
	Bar,
	Block,
	Underline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Preference {
	pub shape: Shape,
	pub blinking: bool,
	pub animated: bool,
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
			// Preserve existing shape/blink choices while enabling the new movement style.
			animated: value & 8 == 0,
		}
	}

	fn encode(self) -> u8 {
		(match self.shape {
			Shape::Bar => 0,
			Shape::Block => 1,
			Shape::Underline => 2,
		}) | if self.blinking { 4 } else { 0 }
			| if self.animated { 0 } else { 8 }
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

#[derive(Default)]
pub(super) struct Cursor {
	visible: bool,
	active: bool,
	animated: bool,
	fade_at: Option<Instant>,
	motion: motion::Motion,
	selection: Range<usize>,
	length: usize,
	timer: Option<Task<()>>,
	window: Option<WindowId>,
	subscriptions: Vec<Subscription>,
}
impl Cursor {
	pub fn reset(&mut self) {
		self.visible = true;
		self.fade_at = None;
		self.timer = None;
	}

	pub fn reset_motion(&mut self) {
		self.motion.reset();
	}

	fn opacity(&self, now: Instant) -> (f32, bool) {
		let progress =
			self.fade_at.map_or(1., |at| (now.duration_since(at).as_secs_f32() / 0.1).min(1.));
		let eased = progress * progress * (3. - 2. * progress);
		(if self.visible { eased } else { 1. - eased }, progress < 1.)
	}

	pub fn paint(
		&mut self,
		logical: usize,
		cell: Bounds<Pixels>,
		bounds: Bounds<Pixels>,
		scroll: Pixels,
		window: &mut Window,
		cx: &mut App,
	) {
		let now = Instant::now();
		let shape = Preference::configured().shape;
		let (offset, size) = match shape {
			Shape::Bar => (
				gpui::point(gpui::px(0.), gpui::px(1.)),
				gpui::size(gpui::px(1.5), gpui::px(ui_theme::BODY_LINE_HEIGHT - 2.)),
			),
			Shape::Block => (Point::default(), cell.size),
			Shape::Underline => (
				gpui::point(gpui::px(0.), gpui::px(ui_theme::BODY_LINE_HEIGHT - 2.)),
				gpui::size(cell.size.width, gpui::px(2.)),
			),
		};
		let target = window.pixel_snap_bounds(Bounds::new(cell.origin + offset, size));
		let corners = if self.animated && shape != Shape::Underline {
			self.motion.sample(
				logical,
				target,
				motion::Viewport { bounds, scroll, scale: window.scale_factor(), shape },
				now,
			)
		} else {
			self.motion.reset();
			None
		};
		let (opacity, fading) = self.opacity(now);
		if fading || corners.is_some() {
			ui_motion::request_frame(window, cx);
		}
		let mut color: gpui::Hsla = gpui::rgb(ui_theme::TEXT).into();
		color.a = opacity * if shape == Shape::Block { 0.32 } else { 1. };
		if let Some(corners) = corners {
			let mut path = gpui::PathBuilder::fill();
			path.add_polygon(&corners, true);
			if let Ok(path) = path.build() {
				window.paint_path(path, color);
			}
		} else {
			let mut quad = gpui::fill(target, color);
			if shape == Shape::Bar {
				quad.corner_radii = gpui::Corners::all(gpui::px(0.75));
			}
			window.paint_quad(quad);
		}
	}
}

impl ComposerInput {
	pub(super) fn update_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.cursor.window != Some(window.window_handle().window_id()) {
			self.cursor.reset_motion();
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
					s.cursor.fade_at = None;
					s.cursor.reset_motion();

					cx.notify();
				}),
			];
		}

		let active = eligible(
			self.focus_handle.is_focused(window),
			window.is_window_active() && ui_motion::application_active(),
			self.selected_range.is_empty(),
		);
		let preference = Preference::configured();
		let reduced = ui_motion::reduced();
		self.cursor.animated = preference.animated && !reduced;
		if active != self.cursor.active {
			self.cursor.reset_motion();
		}

		if active != self.cursor.active
			|| self.selected_range != self.cursor.selection
			|| self.content.len() != self.cursor.length
		{
			self.cursor.reset();
		}

		self.cursor.active = active;
		self.cursor.selection = self.selected_range.clone();
		self.cursor.length = self.content.len();

		if !active || !preference.blinking || reduced {
			self.cursor.timer = None;
			self.cursor.visible = active;
			self.cursor.fade_at = None;
			if !active {
				self.cursor.reset_motion();
			}

			return;
		}
		if self.cursor.timer.is_none() {
			self.cursor.timer = Some(cx.spawn_in(window, async |input, cx| {
				cx.background_executor().timer(Duration::from_millis(530)).await;

				let _ = input.update_in(cx, |s, window, _| {
					s.cursor.timer = None;
					s.cursor.visible = !s.cursor.visible;
					s.cursor.fade_at = Some(Instant::now());
					// A blink changes pixels, not the draft or its observers.
					window.refresh();
				});
			}));
		}
	}
}

pub(super) fn eligible(focused: bool, window_active: bool, selection_empty: bool) -> bool {
	focused && window_active && selection_empty
}

#[cfg(all(target_os = "macos", not(test)))]
fn saved() -> Option<u8> {
	unsafe {
		let defaults: Retained<AnyObject> = objc2::msg_send![
			AnyClass::get(c"NSUserDefaults").expect("Foundation"),
			standardUserDefaults
		];
		let key = objc2_foundation::NSString::from_str("DecodexInsertionCursor");
		let value: Option<Retained<AnyObject>> = objc2::msg_send![&*defaults, objectForKey: &*key];

		value.map(|_| {
			let value: isize = objc2::msg_send![&*defaults, integerForKey: &*key];

			value as u8
		})
	}
}

#[cfg(all(target_os = "macos", not(test)))]
fn save(value: u8) {
	unsafe {
		let defaults: Retained<AnyObject> = objc2::msg_send![
			AnyClass::get(c"NSUserDefaults").expect("Foundation"),
			standardUserDefaults
		];
		let key = objc2_foundation::NSString::from_str("DecodexInsertionCursor");
		let _: () = objc2::msg_send![&*defaults, setInteger: isize::from(value), forKey: &*key];
	}
}

#[cfg(not(all(target_os = "macos", not(test))))]
fn saved() -> Option<u8> {
	None
}

#[cfg(not(all(target_os = "macos", not(test))))]
fn save(_: u8) {}
#[cfg(test)]
mod tests {
	use crate::composer_input::cursor::{self, Preference, Shape};
	use gpui::Focusable as _;
	use std::{
		cell::Cell,
		rc::Rc,
		time::{Duration, Instant},
	};

	#[test]
	fn saved_cursor_choices_round_trip() {
		for shape in [Shape::Bar, Shape::Block, Shape::Underline] {
			for blinking in [false, true] {
				for animated in [false, true] {
					let value = Preference { shape, blinking, animated };
					assert_eq!(Preference::decode(value.encode()), value);
				}
			}
		}
		assert_eq!(
			Preference::decode(4),
			Preference { shape: Shape::Bar, blinking: true, animated: true }
		);
	}

	#[test]
	fn retained_focus_in_an_inactive_window_never_shows_a_caret() {
		assert!(!cursor::eligible(true, false, true));
		assert!(!cursor::eligible(false, true, true));
		assert!(!cursor::eligible(true, true, false));
		assert!(cursor::eligible(true, true, true));
	}

	#[test]
	fn blink_fades_but_typing_shows_the_cursor_immediately() {
		let now = Instant::now();
		let mut cursor = super::Cursor { fade_at: Some(now), ..Default::default() };
		assert_eq!(cursor.opacity(now), (1., true));
		assert_eq!(cursor.opacity(now + Duration::from_millis(50)), (0.5, true));
		assert_eq!(cursor.opacity(now + Duration::from_millis(100)), (0., false));
		cursor.reset();
		assert_eq!(cursor.opacity(now), (1., false));
	}

	#[gpui::test]
	fn blink_and_motion_frames_do_not_notify_draft_observers(cx: &mut gpui::TestAppContext) {
		let (input, visual) = cx.add_window_view(|_, cx| super::ComposerInput::new(0, cx));
		visual.simulate_resize(gpui::size(gpui::px(600.), gpui::px(160.)));
		input.update(visual, |input, cx| input.set_content("A quiet reading corner.", cx));
		visual.update(|window, cx| {
			window.activate_window();
			window.focus(&input.focus_handle(cx), cx);
			window.draw(cx).clear(cx);
		});
		visual.run_until_parked();
		let notifications = Rc::new(Cell::new(0));
		visual.update(|window, cx| {
			let notifications = notifications.clone();
			cx.observe(&input, move |_, _| notifications.set(notifications.get() + 1)).detach();
			window.draw(cx).clear(cx);
		});
		assert!(input.read_with(visual, |input, _| input.cursor.visible && input.cursor.active));
		visual.executor().advance_clock(Duration::from_millis(530));
		visual.run_until_parked();
		assert!(!input.read_with(visual, |input, _| input.cursor.visible));
		assert_eq!(notifications.get(), 0, "idle blinking must not trigger draft observers");
		input.update(visual, |input, cx| input.move_to(0, cx));
		visual.run_until_parked();
		notifications.set(0);
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
			assert!(window.simulate_next_frame(cx) > 0);
			window.draw(cx).clear(cx);
		});
		visual.run_until_parked();
		assert_eq!(notifications.get(), 0, "cursor interpolation must only refresh pixels");
	}
}
