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
	// Resume every visible surface when the app returns, including non-key children.
	cx.observe_window_activation(window, |_, window, cx| {
		if window.is_window_active() && application_active() {
			cx.refresh_windows();
		}
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
		window.request_animation_frame();
	}
}
