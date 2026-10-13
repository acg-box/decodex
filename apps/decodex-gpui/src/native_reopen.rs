//! Preserve GPUI's reopen callback when an auxiliary window remains visible.
use std::mem;

use objc2::{
	MainThreadMarker, ffi,
	runtime::{AnyClass, AnyObject, Imp, Sel},
};
use objc2_app_kit::NSApplication;

pub(crate) fn install() -> bool {
	let Some(main) = MainThreadMarker::new() else { return false };
	let application = NSApplication::sharedApplication(main);
	let Some(delegate) = application.delegate() else { return false };
	let object: &AnyObject = (*delegate).as_ref();
	let class = object.class();

	class.name().to_bytes() == b"GPUIApplicationDelegate" && install_on_class(class)
}

fn install_on_class(class: &AnyClass) -> bool {
	let selector = objc2::sel!(applicationShouldHandleReopen:hasVisibleWindows:);
	let original_selector = objc2::sel!(decodexOriginalReopen:hasVisibleWindows:);

	if class.instance_method(original_selector).is_some() {
		return true;
	}
	let Some(method) = class.instance_method(selector) else { return false };

	// The pinned GPUI delegate uses a void return and suppresses its callback if
	// *any* window is visible. Keep its exact ABI and implementation, but let the
	// Decodex callback restore the workspace even when Settings or a panel is open.
	unsafe {
		if !ffi::class_addMethod(
			(class as *const AnyClass).cast_mut(),
			original_selector,
			method.implementation(),
			ffi::method_getTypeEncoding(method),
		)
		.as_bool()
		{
			return false;
		}

		let implementation: Imp = mem::transmute::<
			extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, bool),
			Imp,
		>(reopen);

		method.set_implementation(implementation);
	}

	true
}

extern "C-unwind" fn reopen(this: *mut AnyObject, _: Sel, application: *mut AnyObject, _: bool) {
	// Only explicit application reopen events take this path. Opening the
	// nonactivating menu-bar panel must not reveal the workspace.
	unsafe {
		let _: () =
			objc2::msg_send![this, decodexOriginalReopen: application, hasVisibleWindows: false];
	}
}

#[cfg(test)]
mod tests {
	use std::{cell::Cell, mem, ptr};

	use objc2::{
		ClassType, ffi,
		runtime::{AnyClass, AnyObject, ClassBuilder, Imp, Sel},
	};
	use objc2_foundation::NSObject;

	use super::install_on_class;

	thread_local! {
		static REOPENS: Cell<usize> = const { Cell::new(0) };
	}

	extern "C-unwind" fn gpui_reopen(_: *mut AnyObject, _: Sel, _: *mut AnyObject, visible: bool) {
		if !visible {
			REOPENS.with(|count| count.set(count.get() + 1));
		}
	}

	#[test]
	fn reopen_reaches_the_original_callback_with_auxiliary_windows() {
		let class = ClassBuilder::new(c"DecodexReopenDelegateFixture", NSObject::class())
			.expect("unique fixture class")
			.register();
		let selector = objc2::sel!(applicationShouldHandleReopen:hasVisibleWindows:);
		let description = class.instance_method(objc2::sel!(description)).unwrap().implementation();

		unsafe {
			let implementation: Imp = mem::transmute::<
				extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, bool),
				Imp,
			>(gpui_reopen);

			assert!(
				ffi::class_addMethod(
					(class as *const AnyClass).cast_mut(),
					selector,
					implementation,
					c"v@:@B".as_ptr(),
				)
				.as_bool()
			);
		}
		let instance: objc2::rc::Retained<NSObject> = unsafe { objc2::msg_send![class, new] };
		let send = |visible: bool| unsafe {
			let _: () = objc2::msg_send![&*instance, applicationShouldHandleReopen: ptr::null::<AnyObject>(), hasVisibleWindows: visible];
		};

		send(true);
		REOPENS.with(|count| assert_eq!(count.get(), 0));
		assert!(install_on_class(class));
		assert!(install_on_class(class), "installation must not wrap itself twice");
		send(true);
		send(false);
		REOPENS.with(|count| assert_eq!(count.get(), 2));
		assert!(ptr::fn_addr_eq(
			description,
			class.instance_method(objc2::sel!(description)).unwrap().implementation()
		));
	}
}
