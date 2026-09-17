//! URLSession adapter in the signed app's existing native library.
use serde_json::Value;
#[cfg(target_os = "macos")]
mod macos {
	use super::Value;
	use std::{
		ffi::{CStr, CString, c_char, c_void},
		sync::OnceLock,
	};
	type Create = unsafe extern "C" fn(*const c_char) -> *mut c_void;
	type Command = unsafe extern "C" fn(*mut c_void, *const c_char) -> bool;
	type Poll = unsafe extern "C" fn(*mut c_void) -> *const c_char;
	type Destroy = unsafe extern "C" fn(*mut c_void);
	#[derive(Clone, Copy)]
	struct Bindings {
		create: Create,
		command: Command,
		poll: Poll,
		destroy: Destroy,
	}
	static API: OnceLock<Result<Bindings, ()>> = OnceLock::new();
	pub(crate) struct Stream {
		host: *mut c_void,
		api: Bindings,
	}
	// SAFETY: Swift serializes all adapter state on its private dispatch queue. The owning
	// Rust session mutex serializes command/poll/drop, including the returned string lifetime.
	unsafe impl Send for Stream {}
	impl Stream {
		pub(crate) fn new(token: &str) -> Result<Self, ()> {
			let api = *API.get_or_init(load).as_ref().map_err(|_| ())?;
			let token = CString::new(token).map_err(|_| ())?;
			// SAFETY: checked signed-bundle symbols and synchronous copy of the token argument.
			let host = unsafe { (api.create)(token.as_ptr()) };
			if host.is_null() { Err(()) } else { Ok(Self { host, api }) }
		}

		pub(crate) fn command(&mut self, value: Value) -> bool {
			let Ok(value) = CString::new(value.to_string()) else { return false };
			// SAFETY: retained native stream, serialized by its owner; native code copies data.
			unsafe { (self.api.command)(self.host, value.as_ptr()) }
		}

		pub(crate) fn poll(&mut self) -> Option<Value> {
			// SAFETY: pointer lives until next poll or destruction; copy while exclusively
			// borrowed.
			unsafe {
				let value = (self.api.poll)(self.host);
				if value.is_null() {
					None
				} else {
					serde_json::from_slice(CStr::from_ptr(value).to_bytes()).ok()
				}
			}
		}
	}
	impl Drop for Stream {
		fn drop(&mut self) {
			// SAFETY: unique retained stream, destroyed once after all owner operations finish.
			unsafe { (self.api.destroy)(self.host) };
		}
	}
	fn load() -> Result<Bindings, ()> {
		let exe = std::env::current_exe().map_err(|_| ())?;
		let contents = exe.parent().and_then(std::path::Path::parent).ok_or(())?;
		let path = contents.join("Frameworks/libDecodexMenuBar.dylib");
		if !std::fs::symlink_metadata(&path).map_err(|_| ())?.file_type().is_file() {
			return Err(());
		}
		use std::os::unix::ffi::OsStrExt as _;
		let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| ())?;
		// SAFETY: fixed library in this signed application's Contents/Frameworks directory.
		// Keep it loaded because URLSession completes cancellation asynchronously.
		unsafe {
			let library = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
			if library.is_null() {
				return Err(());
			}
			let create = libc::dlsym(library, c"decodex_dictation_create".as_ptr());
			let command = libc::dlsym(library, c"decodex_dictation_command".as_ptr());
			let poll = libc::dlsym(library, c"decodex_dictation_poll".as_ptr());
			let destroy = libc::dlsym(library, c"decodex_dictation_destroy".as_ptr());
			if [create, command, poll, destroy].iter().any(|p| p.is_null()) {
				return Err(());
			}
			Ok(Bindings {
				create: std::mem::transmute::<*mut c_void, Create>(create),
				command: std::mem::transmute::<*mut c_void, Command>(command),
				poll: std::mem::transmute::<*mut c_void, Poll>(poll),
				destroy: std::mem::transmute::<*mut c_void, Destroy>(destroy),
			})
		}
	}
}
#[cfg(target_os = "macos")] pub(crate) use macos::Stream;
#[cfg(not(target_os = "macos"))]
pub(crate) struct Stream;
#[cfg(not(target_os = "macos"))]
impl Stream {
	pub(crate) fn new(_: &str) -> Result<Self, ()> {
		Err(())
	}

	pub(crate) fn command(&mut self, _: Value) -> bool {
		false
	}

	pub(crate) fn poll(&mut self) -> Option<Value> {
		None
	}
}
