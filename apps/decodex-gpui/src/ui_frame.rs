//! Application-wide animation cadence, independent of keyboard focus.
use gpui::{App, Window};
#[cfg(all(target_os = "macos", not(test)))]
use objc2::{
	rc::Retained,
	runtime::{AnyClass, AnyObject},
};

/// Native focus selects the keyboard target, not the cadence of sibling windows.
pub(crate) fn window_options() -> gpui::WindowOptions {
	gpui::WindowOptions { inactive_frame_interval: None, ..Default::default() }
}

pub(crate) fn observe_activation<T: 'static>(window: &mut Window, cx: &mut gpui::Context<T>) {
	// Paint a static state on departure and resume all surfaces on return.
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

/// Follow the display while the app is foreground; pause decorative motion in background.
pub(crate) fn request_frame(window: &Window, _: &mut App) {
	if application_active() {
		// Interpolation changes pixels, not the model. GPUI's animation helper
		// notifies the rendering entity, which also runs draft and window observers.
		window.on_next_frame(|window, _| window.refresh());
	}
}

#[cfg(test)]
mod tests {
	use gpui::{IntoElement, Render};
	use std::{cell::Cell, rc::Rc};

	struct Motion;
	impl Render for Motion {
		fn render(
			&mut self,
			window: &mut gpui::Window,
			cx: &mut gpui::Context<Self>,
		) -> impl IntoElement {
			super::request_frame(window, cx);
			gpui::div()
		}
	}

	#[gpui::test]
	fn animation_redraw_does_not_notify_model_observers(cx: &mut gpui::TestAppContext) {
		let (view, visual) = cx.add_window_view(|_, _| Motion);
		let notifications = Rc::new(Cell::new(0));
		visual.update(|window, cx| {
			let notifications = notifications.clone();
			cx.observe(&view, move |_, _| notifications.set(notifications.get() + 1)).detach();
			window.draw(cx).clear(cx);
			assert!(window.simulate_next_frame(cx) > 0);
		});
		cx.run_until_parked();
		assert_eq!(notifications.get(), 0);
	}
}
