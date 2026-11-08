//! AppKit owns the native titlebar; GPUI follows its control row.
#[cfg(all(target_os = "macos", not(test)))]
mod macos {
	use gpui::Window;
	use objc2::{MainThreadMarker, MainThreadOnly as _, rc::Retained};
	use objc2_app_kit::{
		NSTitlebarSeparatorStyle, NSToolbar, NSToolbarDisplayMode, NSView, NSWindow,
		NSWindowButton, NSWindowToolbarStyle,
	};
	use objc2_foundation::NSString;
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};

	fn native(window: &Window) -> Option<Retained<NSWindow>> {
		let handle = HasWindowHandle::window_handle(window).ok()?;
		let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
		// The GPUI window owns the view. All callers run on its main thread.
		unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() }.window()
	}

	pub(super) fn configure(window: &Window) {
		let Some(native) = native(window) else { return };
		let main = MainThreadMarker::new().expect("window configuration runs on the main thread");
		let toolbar = NSToolbar::initWithIdentifier(
			NSToolbar::alloc(main),
			&NSString::from_str("box.acg.decodex.window-chrome"),
		);
		toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
		native.setToolbarStyle(NSWindowToolbarStyle::UnifiedCompact);
		native.setToolbar(Some(&toolbar));
		native.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
	}

	pub(super) fn controls_top(window: &Window) -> Option<f32> {
		let native = native(window)?;
		let button = native.standardWindowButton(NSWindowButton::CloseButton)?;
		let bounds = button.convertRect_toView(button.bounds(), None);
		let center = native.frame().size.height - bounds.origin.y - bounds.size.height / 2.;
		let top = center as f32 - crate::ui_theme::CONTROL_GROUP_HEIGHT / 2.;
		let layout = native.contentLayoutRect();
		let frame = native.frame();
		crate::frame_trace::native_chrome(
			center,
			f64::from(top),
			native.frame().size.height - layout.origin.y - layout.size.height,
			[frame.origin.x, frame.origin.y, frame.size.width, frame.size.height],
			native
				.screen()
				.map_or(0., |screen| screen.frame().origin.y + screen.frame().size.height),
		);
		Some(top)
	}
}

pub(crate) fn configure(window: &gpui::Window) {
	#[cfg(all(target_os = "macos", not(test)))]
	macos::configure(window);
	#[cfg(any(not(target_os = "macos"), test))]
	let _ = window;
}

pub(crate) fn controls_top(window: &gpui::Window) -> f32 {
	#[cfg(all(target_os = "macos", not(test)))]
	if let Some(top) = macos::controls_top(window) {
		return top;
	}
	#[cfg(any(not(target_os = "macos"), test))]
	let _ = window;
	crate::ui_theme::CONTROL_MARGIN
}
