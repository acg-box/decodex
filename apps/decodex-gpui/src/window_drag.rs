//! Enter AppKit window tracking without holding GPUI's platform-window mutex.

#[cfg(target_os = "macos")]
pub(crate) fn start(window: &gpui::Window) {
	use objc2::MainThreadMarker;
	use objc2_app_kit::{NSApplication, NSView};
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};

	let Some(main_thread) = MainThreadMarker::new() else { return };
	let Some(event) = NSApplication::sharedApplication(main_thread).currentEvent() else { return };
	let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
	let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
	// GPUI owns the live view; this callback runs on the application's main thread.
	let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
	let Some(native) = view.window() else { return };
	let trace = std::env::var_os("DECODEX_WINDOW_DRAG_TRACE");
	if let Some(path) = trace.as_ref() {
		write_trace(
			path,
			&format!(
				"press type={:?} modifiers={:?} event_window={} window={} movable={} position={:?} frame={:?}",
				event.r#type(),
				event.modifierFlags(),
				event.windowNumber(),
				native.windowNumber(),
				native.isMovable(),
				event.locationInWindow(),
				native.frame()
			),
		);
	}
	// AppKit may call windowDidMove during tracking. That callback needs GPUI's
	// platform mutex, so do not enter through MacWindow::start_window_move.
	let started = std::time::Instant::now();
	native.performWindowDragWithEvent(&event);
	if let Some(path) = trace.as_ref() {
		write_trace(
			path,
			&format!(
				"return elapsed_ms={} frame={:?}",
				started.elapsed().as_millis(),
				native.frame()
			),
		);
	}
}

#[cfg(target_os = "macos")]
fn write_trace(path: &std::ffi::OsStr, line: &str) {
	use std::io::Write as _;
	if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
		let _ = writeln!(file, "{line}");
	}
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn start(window: &gpui::Window) {
	window.start_window_move();
}
