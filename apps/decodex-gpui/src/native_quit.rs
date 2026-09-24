//! Defer AppKit termination without replacing GPUI's delegate or lifecycle hooks.
use objc2::{
	MainThreadMarker,
	runtime::{AnyClass, AnyObject, Imp, Sel},
	sel,
};
use objc2_app_kit::NSApplication;
use std::{
	ffi::c_void,
	sync::atomic::{AtomicBool, Ordering},
};

type RequestHandler = Box<dyn FnMut()>;
thread_local! {
	static REQUEST_HANDLER: std::cell::RefCell<Option<RequestHandler>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn set_request_handler(handler: impl FnMut() + 'static) {
	REQUEST_HANDLER.with(|slot| *slot.borrow_mut() = Some(Box::new(handler)));
}

extern "C" fn notify_request(_: *mut c_void) {
	REQUEST_HANDLER.with(|slot| {
		if let Some(handler) = slot.borrow_mut().as_mut()
			&& take_request()
		{
			handler();
		}
	});
}

static REQUESTED: AtomicBool = AtomicBool::new(false);
static AWAITING_REPLY: AtomicBool = AtomicBool::new(false);
static REPLY_QUEUED: AtomicBool = AtomicBool::new(false);

pub(crate) fn install() -> bool {
	let Some(main) = MainThreadMarker::new() else { return false };
	let application = NSApplication::sharedApplication(main);
	let Some(delegate) = application.delegate() else { return false };
	let object: &AnyObject = (*delegate).as_ref();
	let class = object.class();
	if class.name().to_bytes() != b"GPUIApplicationDelegate" {
		return false;
	}
	if !install_on_class(class) {
		return false;
	}
	// Refresh AppKit's optional-delegate-method cache while retaining the exact
	// GPUI object, its ivars, and every existing lifecycle implementation.
	application.setDelegate(None);
	application.setDelegate(Some(&delegate));
	true
}

fn install_on_class(class: &AnyClass) -> bool {
	let selector = sel!(applicationShouldTerminate:);
	if class.instance_method(selector).is_some() {
		return false;
	}
	// macOS uses a 64-bit NSUInteger result, followed by self, selector, and sender.
	// Only add the absent optional method; never replace an existing implementation.
	unsafe {
		let implementation: Imp = std::mem::transmute::<
			extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
			Imp,
		>(should_terminate);
		objc2::ffi::class_addMethod(
			(class as *const AnyClass).cast_mut(),
			selector,
			implementation,
			c"Q@:@".as_ptr(),
		)
		.as_bool()
	}
}

extern "C-unwind" fn should_terminate(_: *mut AnyObject, _: Sel, _: *mut AnyObject) -> usize {
	if !AWAITING_REPLY.swap(true, Ordering::SeqCst) {
		REQUESTED.store(true, Ordering::SeqCst);
		// Queue once outside AppKit's delegate call and any active GPUI borrow.
		unsafe {
			dispatch_async_f(
				std::ptr::addr_of!(_dispatch_main_q),
				std::ptr::null_mut(),
				notify_request,
			);
		}
	}
	2 // NSTerminateLater: the shared draft preflight must decide.
}

pub(crate) fn take_request() -> bool {
	REQUESTED.swap(false, Ordering::SeqCst)
}

pub(crate) fn awaiting_reply() -> bool {
	AWAITING_REPLY.load(Ordering::SeqCst)
}

pub(crate) fn request() {
	let Some(main) = MainThreadMarker::new() else { return };
	let application = NSApplication::sharedApplication(main);
	// NSTerminateLater starts a nested run loop. Start termination from a run-loop
	// selector, not a main-dispatch-queue block which would prevent queued GPUI
	// futures from running until that nested loop has already returned.
	unsafe {
		let _: () = objc2::msg_send![&*application, performSelector: sel!(terminate:), withObject: std::ptr::null::<AnyObject>(), afterDelay: 0.0f64];
	}
}

pub(crate) fn reply(saved: bool) {
	if !awaiting_reply() || REPLY_QUEUED.swap(true, Ordering::SeqCst) {
		return;
	}
	// Termination calls GPUI shutdown synchronously. Dispatch outside the current
	// App borrow, as GPUI's own Platform::quit does for NSApplication.terminate.
	unsafe {
		dispatch_async_f(
			std::ptr::addr_of!(_dispatch_main_q),
			Box::into_raw(Box::new(saved)).cast(),
			finish_reply,
		);
	}
}

extern "C" fn finish_reply(context: *mut c_void) {
	// The queued closure exclusively owns this boolean and always runs on main.
	let saved = unsafe { *Box::from_raw(context.cast::<bool>()) };
	if let Some(main) = MainThreadMarker::new() {
		AWAITING_REPLY.store(false, Ordering::SeqCst);
		REPLY_QUEUED.store(false, Ordering::SeqCst);
		NSApplication::sharedApplication(main).replyToApplicationShouldTerminate(saved);
	}
}

unsafe extern "C" {
	static _dispatch_main_q: c_void;
	fn dispatch_async_f(
		queue: *const c_void,
		context: *mut c_void,
		callback: extern "C" fn(*mut c_void),
	);
}

#[cfg(test)]
mod tests {
	use super::*;
	use objc2::{ClassType, msg_send, runtime::ClassBuilder};
	use objc2_foundation::NSObject;

	#[test]
	fn native_quit_adds_only_missing_delegate_method_and_coalesces_requests() {
		let class =
			ClassBuilder::new(c"DecodexQuitDelegateFixture", NSObject::class()).unwrap().register();
		let original = class.instance_method(sel!(description)).unwrap().implementation();
		assert!(install_on_class(class));
		assert!(!install_on_class(class), "never overwrite an existing delegate decision");
		assert!(std::ptr::fn_addr_eq(
			original,
			class.instance_method(sel!(description)).unwrap().implementation()
		));
		let instance: objc2::rc::Retained<NSObject> = unsafe { msg_send![class, new] };
		for _ in 0..2 {
			let response: usize = unsafe {
				msg_send![&*instance, applicationShouldTerminate: std::ptr::null::<AnyObject>()]
			};
			assert_eq!(response, 2);
		}
		assert!(awaiting_reply());
		assert!(take_request());
		assert!(!take_request());
		AWAITING_REPLY.store(false, Ordering::SeqCst);
	}
}
