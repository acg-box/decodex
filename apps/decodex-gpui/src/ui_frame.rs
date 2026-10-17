//! Application-wide animation cadence, independent of keyboard focus.
use gpui::{AnyElement, App, AppContext as _, Context, Entity, IntoElement as _, Render, Window};
#[cfg(all(target_os = "macos", not(test)))]
use objc2::{
	rc::Retained,
	runtime::{AnyClass, AnyObject},
};
use std::sync::atomic::AtomicU8;

/// A model publishes state changes; its presentation owns animation frames.
pub(crate) struct Presentation<T: 'static> {
	owner: gpui::WeakEntity<T>,
	render: fn(&mut T, &mut Window, &mut Context<T>) -> AnyElement,
	_observation: gpui::Subscription,
}
impl<T: 'static> Presentation<T> {
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
impl<T: 'static> Render for Presentation<T> {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
		self.owner
			.update(cx, |owner, cx| (self.render)(owner, window, cx))
			.unwrap_or_else(|_| gpui::div().into_any_element())
	}
}

/// Native focus selects the keyboard target, not the cadence of sibling windows.
pub(crate) fn window_options() -> gpui::WindowOptions {
	gpui::WindowOptions { inactive_frame_interval: None, ..Default::default() }
}

pub(crate) fn observe_activation<T: 'static>(window: &mut Window, cx: &mut gpui::Context<T>) {
	// Re-evaluate the rendering policy on departure and resume all surfaces on return.
	// A sibling composer becoming key does not deactivate the application.
	cx.observe_window_activation(window, |_, _, cx| {
		cx.refresh_windows();
	})
	.detach();
}

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
	crate::ui_preferences::boolean("fullBackgroundRendering", &CACHE, value, false)
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
	window.draw(cx).clear(cx);
}

#[cfg(test)]
mod tests {
	use gpui::{AppContext as _, Entity, IntoElement, Render};
	use std::{cell::Cell, rc::Rc};

	struct Motion(Entity<()>);
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
}
