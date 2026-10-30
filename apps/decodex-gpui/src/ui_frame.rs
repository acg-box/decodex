//! Application-wide animation cadence, independent of keyboard focus.
use std::sync::atomic::AtomicU8;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, IntoElement, Render, Subscription,
	WeakEntity, Window,
};
#[cfg(all(target_os = "macos", not(test)))]
use objc2::{
	rc::Retained,
	runtime::{AnyClass, AnyObject},
};

use crate::ui_preferences;

/// A model publishes state changes; its presentation owns animation frames.
pub(crate) struct Presentation<T>
where
	T: 'static,
{
	owner: WeakEntity<T>,
	render: fn(&mut T, &mut Window, &mut Context<T>) -> AnyElement,
	_observation: Subscription,
}
impl<T> Presentation<T>
where
	T: 'static,
{
	pub(crate) fn new(
		owner: &Entity<T>,
		render: fn(&mut T, &mut Window, &mut Context<T>) -> AnyElement,
		cx: &mut App,
	) -> Entity<Self> {
		cx.new(|cx| Self {
			owner: owner.downgrade(),
			render,
			_observation: cx.observe(owner, |_, _, cx| cx.notify()),
		})
	}
}
impl<T> Render for Presentation<T>
where
	T: 'static,
{
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.owner
			.update(cx, |owner, cx| (self.render)(owner, window, cx))
			.unwrap_or_else(|_| gpui::div().into_any_element())
	}
}

/// Native focus selects the keyboard target, not the cadence of sibling windows.
pub(crate) fn window_options() -> gpui::WindowOptions {
	gpui::WindowOptions { inactive_frame_interval: None, ..Default::default() }
}

pub(crate) fn observe_activation<T>(window: &mut Window, cx: &mut Context<T>)
where
	T: 'static,
{
	let _ = cx.default_global::<Activation>();
	cx.observe_window_activation(window, |_, window, cx| {
		let active = application_active();
		if std::mem::replace(&mut cx.default_global::<Activation>().0, active) != active {
			// App departure and return change every surface's animation policy.
			cx.refresh_windows();
		} else {
			// A menu taking keyboard focus does not invalidate the transcript cache.
			window.refresh();
		}
	})
	.detach();
}

struct Activation(bool);
impl Default for Activation {
	fn default() -> Self {
		Self(application_active())
	}
}
impl gpui::Global for Activation {}

pub(crate) fn application_active() -> bool {
	#[cfg(all(target_os = "macos", not(test)))]
	unsafe {
		let app: Retained<AnyObject> =
			objc2::msg_send![AnyClass::get(c"NSApplication").expect("AppKit"), sharedApplication];

		objc2::msg_send![&*app, isActive]
	}

	#[cfg(any(not(target_os = "macos"), test))]
	true
}

pub(crate) fn background_rendering(value: Option<bool>) -> bool {
	static CACHE: AtomicU8 = AtomicU8::new(u8::MAX);

	ui_preferences::boolean("fullBackgroundRendering", &CACHE, value, false)
}

/// Rendering policy is separate from keyboard focus and application activation.
pub(crate) fn animations_active() -> bool {
	application_active() || background_rendering(None)
}

/// Request the display cadence whenever the user permits animation.
pub(crate) fn request_frame(window: &Window, _: &mut App) {
	if animations_active() {
		// Presentation entities own frame demand. Their notifications leave the
		// model untouched and let static sibling views keep their layout cache.
		window.request_animation_frame();
	}
}

#[cfg(test)]
pub(crate) fn draw_frame(window: &mut Window, cx: &mut App) {
	// A cached subtree advances when the display delivers its frame request.
	window.simulate_next_frame(cx);

	draw_current_frame(window, cx);
}

#[cfg(test)]
pub(crate) fn draw_current_frame(window: &mut Window, cx: &mut App) {
	window.draw(cx).clear(cx);
}

#[cfg(test)]
mod tests {
	use std::{cell::Cell, rc::Rc};

	use gpui::{AppContext as _, Entity, IntoElement, Render};

	struct Motion(Entity<()>);
	struct Counted(Rc<Cell<usize>>);
	impl Render for Counted {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			_: &mut gpui::Context<Self>,
		) -> impl IntoElement {
			self.0.set(self.0.get() + 1);
			gpui::div()
		}
	}
	struct Cached(Entity<Counted>);
	impl Render for Cached {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			_: &mut gpui::Context<Self>,
		) -> impl IntoElement {
			self.0.clone().cached(gpui::StyleRefinement::default())
		}
	}
	impl Render for Motion {
		fn render(
			&mut self,
			window: &mut gpui::Window,
			cx: &mut gpui::Context<Self>,
		) -> impl IntoElement {
			self.0.read(cx);

			super::request_frame(window, cx);

			gpui::div()
		}
	}

	#[gpui::test]
	fn animation_redraw_does_not_notify_model_observers(cx: &mut gpui::TestAppContext) {
		let model = cx.new(|_| ());
		let (_, visual) = cx.add_window_view(|_, _| Motion(model.clone()));
		let notifications = Rc::new(Cell::new(0));

		visual.update(|window, cx| {
			let notifications = notifications.clone();

			cx.observe(&model, move |_, _| notifications.set(notifications.get() + 1)).detach();
			window.draw(cx).clear(cx);

			assert!(window.simulate_next_frame(cx) > 0);
		});

		cx.run_until_parked();

		assert_eq!(notifications.get(), 0);
	}

	#[gpui::test]
	fn menu_focus_preserves_the_other_windows_layout_cache(cx: &mut gpui::TestAppContext) {
		let renders = Rc::new(Cell::new(0));
		let mut composer_cx = cx.clone();
		let mut menu_cx = cx.clone();
		let (_, main) = cx.add_window_view(|window, cx| {
			super::observe_activation(window, cx);
			Cached(cx.new(|_| Counted(renders.clone())))
		});
		let (_, composer) = composer_cx.add_window_view(|window, cx| {
			super::observe_activation(window, cx);
			Counted(Rc::new(Cell::new(0)))
		});
		let (_, menu) = menu_cx.add_window_view(|window, cx| {
			super::observe_activation(window, cx);
			Counted(Rc::new(Cell::new(0)))
		});
		composer.update(|window, _| window.activate_window());
		main.run_until_parked();
		main.update(super::draw_current_frame);
		let before = renders.get();

		for _ in 0..4 {
			menu.update(|window, _| window.activate_window());
			main.run_until_parked();
			main.update(super::draw_current_frame);
			composer.update(|window, _| window.activate_window());
			main.run_until_parked();
			main.update(super::draw_current_frame);
		}
		assert!(before > 0);
		assert_eq!(renders.get(), before, "changing the key window must not re-layout history");
	}
}
