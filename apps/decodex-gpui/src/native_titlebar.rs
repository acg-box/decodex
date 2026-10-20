//! Let AppKit own titlebar dragging while GPUI owns the visible controls.
use std::{cell::RefCell, rc::Rc};

use gpui::{AnyElement, Bounds, IntoElement as _, Pixels, Styled as _};

#[derive(Clone, Default)]
pub(crate) struct Titlebar {
	pub(super) controls: Rc<RefCell<Vec<Bounds<Pixels>>>>,
	#[cfg(all(target_os = "macos", not(test)))]
	native: Rc<RefCell<Option<macos::Controls>>>,
}

impl Titlebar {
	pub(super) fn begin_frame(&self) {
		self.controls.borrow_mut().clear();
	}

	/// Measure the existing button; do not maintain a second set of button geometry.
	pub(crate) fn control(&self) -> AnyElement {
		self.bordered_control(0.)
	}

	pub(super) fn bordered_control(&self, border: f32) -> AnyElement {
		let controls = self.controls.clone();
		gpui::canvas(
			move |bounds, _, _| controls.borrow_mut().push(bounds.dilate(gpui::px(border))),
			|_, (), _, _| {},
		)
		.absolute()
		.top_0()
		.left_0()
		.size_full()
		.into_any_element()
	}

	/// Place native hit targets after every visible control has been measured.
	pub(super) fn sync(&self) -> AnyElement {
		#[cfg(all(target_os = "macos", not(test)))]
		let titlebar = self.clone();
		gpui::canvas(
			move |_, _window, _| {
				#[cfg(all(target_os = "macos", not(test)))]
				{
					let mut native = titlebar.native.borrow_mut();
					if native.is_none() {
						*native = macos::Controls::new(_window);
					}
					if let Some(native) = native.as_mut() {
						native.place(&titlebar.controls.borrow());
					}
				}
			},
			|_, (), _, _| {},
		)
		.absolute()
		.size_full()
		.into_any_element()
	}
}

#[cfg(all(target_os = "macos", not(test)))]
mod macos {
	use gpui::{Bounds, Pixels, Window};
	use objc2::{
		rc::Retained,
		runtime::{AnyClass, Bool, ClassBuilder, Sel},
	};
	use objc2_app_kit::{NSEvent, NSView};
	use objc2_foundation::{NSPoint, NSRect, NSSize};
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};

	pub(super) struct Controls {
		foreground: Retained<NSView>,
		views: Vec<Retained<NSView>>,
	}

	impl Controls {
		pub(super) fn new(window: &Window) -> Option<Self> {
			let handle = HasWindowHandle::window_handle(window).ok()?;
			let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
			// The main GPUI window owns this view. All titlebar work runs on its main thread.
			let foreground = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }?;
			Some(Self { foreground, views: Vec::new() })
		}

		pub(super) fn place(&mut self, controls: &[Bounds<Pixels>]) {
			let Some(content) = self.foreground.window().and_then(|w| w.contentView()) else {
				return;
			};
			while self.views.len() > controls.len() {
				if let Some(view) = self.views.pop() {
					view.removeFromSuperview();
				}
			}
			for (index, bounds) in controls.iter().enumerate() {
				if index == self.views.len() {
					let view: Retained<NSView> = unsafe { objc2::msg_send![control_class(), new] };
					// GPUI remains the only accessibility and rendering owner of the button.
					unsafe {
						let _: () = objc2::msg_send![&*view, setAccessibilityElement: false];
					}
					content.addSubview(&view);
					// Controls owns the target until every forwarding view is detached.
					unsafe {
						view.setNextResponder(Some(&self.foreground));
					}
					self.views.push(view);
				}
				let frame = NSRect::new(
					NSPoint::new(
						f64::from(f32::from(bounds.origin.x)),
						content.bounds().size.height
							- f64::from(f32::from(bounds.origin.y + bounds.size.height)),
					),
					NSSize::new(
						f64::from(f32::from(bounds.size.width)),
						f64::from(f32::from(bounds.size.height)),
					),
				);
				if self.views[index].frame() != frame {
					self.views[index].setFrame(frame);
				}
			}
		}
	}

	impl Drop for Controls {
		fn drop(&mut self) {
			for view in &self.views {
				view.removeFromSuperview();
			}
		}
	}

	fn control_class() -> &'static AnyClass {
		if let Some(class) = AnyClass::get(c"DecodexTitlebarControl") {
			return class;
		}
		let mut builder = ClassBuilder::new(
			c"DecodexTitlebarControl",
			AnyClass::get(c"NSControl").expect("AppKit control class"),
		)
		.expect("unique titlebar control class");
		unsafe {
			builder.add_method(
				objc2::sel!(mouseDownCanMoveWindow),
				cannot_move as extern "C-unwind" fn(_, _) -> _,
			);
			builder.add_method(
				objc2::sel!(acceptsFirstMouse:),
				accept_first_mouse as extern "C-unwind" fn(_, _, _) -> _,
			);
			for selector in
				[objc2::sel!(mouseDown:), objc2::sel!(mouseDragged:), objc2::sel!(mouseUp:)]
			{
				builder.add_method(selector, forward_mouse as extern "C-unwind" fn(_, _, _));
			}
		}
		builder.register()
	}

	extern "C-unwind" fn cannot_move(_: &NSView, _: Sel) -> Bool {
		Bool::NO
	}

	extern "C-unwind" fn accept_first_mouse(_: &NSView, _: Sel, _: &NSEvent) -> Bool {
		Bool::YES
	}

	extern "C-unwind" fn forward_mouse(view: &NSView, selector: Sel, event: &NSEvent) {
		// This responder was installed and retained by Controls on the main thread.
		if let Some(foreground) = unsafe { view.nextResponder() } {
			if selector == objc2::sel!(mouseDown:) {
				foreground.mouseDown(event);
			} else if selector == objc2::sel!(mouseDragged:) {
				foreground.mouseDragged(event);
			} else {
				foreground.mouseUp(event);
			}
		}
	}
}
