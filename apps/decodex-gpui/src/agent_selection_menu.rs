//! A native copy menu, presented outside GPUI's application borrow.
use std::{ffi::c_void, ptr};

use gpui::{App, AsyncApp, ClipboardItem, Window};
use objc2::{
	rc::Retained,
	runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
};
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

struct Request {
	view: Retained<NSView>,
	position: NSPoint,
	markdown: String,
	text: String,
	app: AsyncApp,
}

pub(super) fn show(window: &Window, cx: &App, markdown: String, text: String) {
	let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
	let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
	// The retained GPUI view and menu snapshot live until native tracking ends.
	let Some(view) = (unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }) else {
		return;
	};
	let mouse = window.mouse_position();
	let position = NSPoint::new(
		f64::from(f32::from(mouse.x)),
		if view.isFlipped() {
			f64::from(f32::from(mouse.y))
		} else {
			view.bounds().size.height - f64::from(f32::from(mouse.y))
		},
	);
	let request = Request { view, position, markdown, text, app: cx.to_async() };

	// AppKit tracks a menu in a nested event loop. Do not enter it inside a GPUI callback.
	unsafe {
		dispatch_async_f(
			std::ptr::addr_of!(_dispatch_main_q),
			Box::into_raw(Box::new(request)).cast(),
			present,
		);
	}
}

extern "C" fn present(context: *mut c_void) {
	// The main queue exclusively owns this snapshot and all AppKit calls stay on main.
	let request = unsafe { Box::from_raw(context.cast::<Request>()) };

	if request.view.window().is_none() {
		return;
	}

	let chosen = unsafe { menu(&request) };
	let text = match chosen {
		1 => request.markdown,
		2 => request.text,
		_ => return,
	};

	request.app.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
}

unsafe fn menu(request: &Request) -> isize {
	// Every Objective-C object is retained for the synchronous native tracking call.
	unsafe {
		let menu_class = AnyClass::get(c"NSMenu").expect("AppKit menu");
		let menu: Retained<AnyObject> = objc2::msg_send![menu_class, new];
		let target: Retained<AnyObject> = objc2::msg_send![target_class(), new];
		let _: () = objc2::msg_send![&*menu, setAutoenablesItems: false];

		for (tag, label, key) in [(1_isize, "Copy Markdown", "c"), (2, "Copy Text", "")] {
			let item: Retained<AnyObject> =
				objc2::msg_send![AnyClass::get(c"NSMenuItem").expect("AppKit menu item"), new];
			let _: () = objc2::msg_send![&*item, setTitle: &*NSString::from_str(label)];
			let _: () = objc2::msg_send![&*item, setAction: objc2::sel!(choose:)];
			let _: () = objc2::msg_send![&*item, setKeyEquivalent: &*NSString::from_str(key)];
			let _: () = objc2::msg_send![&*item, setTarget: &*target];
			let _: () = objc2::msg_send![&*item, setTag: tag];
			let _: () = objc2::msg_send![&*menu, addItem: &*item];
		}

		let _: bool = objc2::msg_send![&*menu, popUpMenuPositioningItem: ptr::null::<AnyObject>(), atLocation: request.position, inView: &*request.view];

		objc2::msg_send![&*target, tag]
	}
}

fn target_class() -> &'static AnyClass {
	if let Some(class) = AnyClass::get(c"DecodexSelectionMenuTarget") {
		return class;
	}

	let mut class = ClassBuilder::new(
		c"DecodexSelectionMenuTarget",
		AnyClass::get(c"NSMenuItem").expect("AppKit menu item"),
	)
	.expect("unique menu target");

	// A menu-local tag records the choice. App state is updated after tracking ends.
	unsafe {
		class.add_method(objc2::sel!(choose:), choose as extern "C-unwind" fn(_, _, _));
	}

	class.register()
}

extern "C-unwind" fn choose(target: &AnyObject, _: Sel, sender: &AnyObject) {
	// Both arguments are the retained NSMenuItems created by menu().
	unsafe {
		let tag: isize = objc2::msg_send![sender, tag];
		let _: () = objc2::msg_send![target, setTag: tag];
	}
}

unsafe extern "C" {
	static _dispatch_main_q: c_void;
	fn dispatch_async_f(
		queue: *const c_void,
		context: *mut c_void,
		work: extern "C" fn(*mut c_void),
	);
}
