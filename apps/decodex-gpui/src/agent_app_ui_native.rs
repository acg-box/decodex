//! Main-thread ownership of the bundled native App UI window.
use super::*;
use serde_json::Value;
#[cfg(all(target_os = "macos", not(test)))] use serde_json::json;
#[cfg(all(target_os = "macos", not(test)))]
pub(super) struct AppHost {
	host: *mut std::ffi::c_void,
	command_fn: unsafe extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_char) -> bool,
	poll_fn: unsafe extern "C" fn(*mut std::ffi::c_void) -> *const std::ffi::c_char,
	destroy: unsafe extern "C" fn(*mut std::ffi::c_void),
	_main_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
#[cfg(all(target_os = "macos", not(test)))]
impl AppHost {
	pub(super) fn new(window: &Window) -> Result<Self, ()> {
		use crate::native_menu_bar::{bundled_library_path, symbol};
		use std::{ffi::CString, os::unix::ffi::OsStrExt as _};
		let path =
			bundled_library_path(&std::env::current_exe().map_err(|_| ())?).map_err(|_| ())?;
		if !std::fs::symlink_metadata(&path).map_err(|_| ())?.file_type().is_file() {
			return Err(());
		}
		let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| ())?;
		// SAFETY: fixed signed-app library and exact versioned C ABI; this object cannot cross
		// threads.
		unsafe {
			let image = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
			if image.is_null() {
				return Err(());
			}
			let version: unsafe extern "C" fn() -> u32 =
				symbol(image, c"decodex_mcp_app_abi_version").map_err(|_| ())?;
			if version() != 1 {
				return Err(());
			}
			let create: unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void =
				symbol(image, c"decodex_mcp_app_create").map_err(|_| ())?;
			let command_fn = symbol(image, c"decodex_mcp_app_command").map_err(|_| ())?;
			let poll_fn = symbol(image, c"decodex_mcp_app_poll").map_err(|_| ())?;
			let destroy = symbol(image, c"decodex_mcp_app_destroy").map_err(|_| ())?;
			let native =
				raw_window_handle::HasWindowHandle::window_handle(window).map_err(|_| ())?;
			let raw_window_handle::RawWindowHandle::AppKit(handle) = native.as_raw() else {
				return Err(());
			};
			let host = create(handle.ns_view.as_ptr());
			if host.is_null() {
				return Err(());
			}
			// Keep the image loaded: WebKit can complete cleanup asynchronously.
			Ok(Self { host, command_fn, poll_fn, destroy, _main_thread: std::marker::PhantomData })
		}
	}

	pub(super) fn command(&mut self, value: Value) -> bool {
		let Ok(text) = std::ffi::CString::new(value.to_string()) else { return false };
		// SAFETY: retained native host; copied UTF-8 argument lives through the synchronous call.
		unsafe { (self.command_fn)(self.host, text.as_ptr()) }
	}

	pub(super) fn poll(&mut self) -> Option<Value> {
		// SAFETY: native data remains valid until the next poll or destroy. Copy it immediately.
		unsafe {
			let event = (self.poll_fn)(self.host);
			if event.is_null() {
				None
			} else {
				serde_json::from_slice(std::ffi::CStr::from_ptr(event).to_bytes()).ok()
			}
		}
	}
}
#[cfg(all(target_os = "macos", not(test)))]
impl Drop for AppHost {
	fn drop(&mut self) {
		self.command(json!({"operation":"close"}));
		// SAFETY: unique host, destroyed exactly once on the GPUI main thread.
		unsafe { (self.destroy)(self.host) };
	}
}
#[cfg(any(not(target_os = "macos"), test))]
pub(super) struct AppHost;
#[cfg(any(not(target_os = "macos"), test))]
impl AppHost {
	pub(super) fn new(_: &Window) -> Result<Self, ()> {
		Err(())
	}

	pub(super) fn command(&mut self, _: Value) -> bool {
		false
	}

	pub(super) fn poll(&mut self) -> Option<Value> {
		None
	}
}
