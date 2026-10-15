//! Hand the current held movement to AppKit, as GPUI's native titlebar does.

#[cfg(target_os = "macos")]
pub(crate) fn start(window: &gpui::Window) {
	use objc2::MainThreadMarker;
	use objc2_app_kit::{NSApplication, NSEvent, NSView};
	use objc2_foundation::NSProcessInfo;
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
				"handoff type={:?} modifiers={:?} event_window={} window={} movable={} active={} pressed={} timestamp={:.6} age_ms={:.3} position={:?} frame={:?}",
				event.r#type(),
				event.modifierFlags(),
				event.windowNumber(),
				native.windowNumber(),
				native.isMovable(),
				NSApplication::sharedApplication(main_thread).isActive(),
				NSEvent::pressedMouseButtons(),
				event.timestamp(),
				(NSProcessInfo::processInfo().systemUptime() - event.timestamp()) * 1_000.,
				event.locationInWindow(),
				native.frame()
			),
		);
	}
	// Keep GPUI's platform mutex free for native callbacks during the handoff.
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
