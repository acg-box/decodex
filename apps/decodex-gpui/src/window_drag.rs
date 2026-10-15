//! Keep the original press alive until AppKit takes over window movement.

pub(crate) struct WindowDrag {
	#[cfg(all(target_os = "macos", not(test)))]
	event: objc2::rc::Retained<objc2::runtime::AnyObject>,
}

impl WindowDrag {
	pub(crate) fn capture() -> Option<Self> {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			let app =
				objc2_app_kit::NSApplication::sharedApplication(objc2::MainThreadMarker::new()?);
			let event: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> =
				unsafe { objc2::msg_send![&*app, currentEvent] };
			event.map(|event| Self { event })
		}
		#[cfg(any(not(target_os = "macos"), test))]
		Some(Self {})
	}

	pub(crate) fn start(self, window: &gpui::Window) {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			use raw_window_handle::{HasWindowHandle, RawWindowHandle};
			let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
			let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
			// The live GPUI window owns the view. Input dispatch runs on the main thread.
			let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
			if let Some(native) = view.window() {
				// NSWindow requires the original mouseDown, not the current mouseDragged.
				unsafe {
					let _: () =
						objc2::msg_send![&*native, performWindowDragWithEvent: &*self.event];
				}
			}
		}
		#[cfg(any(not(target_os = "macos"), test))]
		window.start_window_move();
	}
}
