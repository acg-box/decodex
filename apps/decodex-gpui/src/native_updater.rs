//! Main-thread bridge to Sparkle in the signed native framework.

#[cfg(all(target_os = "macos", not(test)))]
mod platform {
	use std::ffi::{CStr, CString, c_void};
	type Create = unsafe extern "C" fn() -> *mut c_void;
	type Check = unsafe extern "C" fn(*mut c_void) -> bool;
	type Destroy = unsafe extern "C" fn(*mut c_void);

	pub(super) struct Bridge {
		_image: *mut c_void,
		host: *mut c_void,
		check: Check,
		destroy: Destroy,
	}
	impl Bridge {
		pub(super) fn load() -> Result<Self, String> {
			let executable = std::env::current_exe().map_err(|e| e.to_string())?;
			let contents = executable
				.parent()
				.and_then(std::path::Path::parent)
				.filter(|p| p.file_name().is_some_and(|n| n == "Contents"))
				.ok_or("Application installation is available from Decodex.app.")?;
			let path = CString::new(
				contents.join("Frameworks/libDecodexMenuBar.dylib").to_string_lossy().as_bytes(),
			)
			.map_err(|e| e.to_string())?;
			// SAFETY: the library belongs to the running signed app. It remains loaded.
			let image = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
			if image.is_null() {
				return Err("The native update framework is unavailable.".into());
			}
			// SAFETY: the checked non-null symbols have these exact C ABIs in UpdateHost.swift.
			let (create, check, destroy): (Create, Check, Destroy) = unsafe {
				(
					std::mem::transmute::<*mut c_void, Create>(symbol(
						image,
						c"decodex_update_create",
					)?),
					std::mem::transmute::<*mut c_void, Check>(symbol(
						image,
						c"decodex_update_check",
					)?),
					std::mem::transmute::<*mut c_void, Destroy>(symbol(
						image,
						c"decodex_update_destroy",
					)?),
				)
			};
			// SAFETY: creation occurs on the GPUI main thread.
			let host = unsafe { create() };
			if host.is_null() {
				return Err("App updates are not configured for this build. Codex components can still be prepared and repaired.".into());
			}
			Ok(Self { _image: image, host, check, destroy })
		}

		pub(super) fn check(&mut self) -> Result<(), String> {
			// SAFETY: host is retained and calls stay on the main thread.
			if unsafe { (self.check)(self.host) } {
				Ok(())
			} else {
				Err("An update check is already in progress.".into())
			}
		}
	}
	unsafe fn symbol(image: *mut c_void, name: &CStr) -> Result<*mut c_void, String> {
		// SAFETY: image is a retained dlopen handle; the symbol name is nul-terminated.
		let address = unsafe { libc::dlsym(image, name.as_ptr()) };
		if address.is_null() {
			Err("The native update framework has an incompatible interface.".into())
		} else {
			Ok(address)
		}
	}
	impl Drop for Bridge {
		fn drop(&mut self) {
			// SAFETY: exactly one main-thread owner releases the retained Swift object.
			unsafe { (self.destroy)(self.host) };
		}
	}
}

pub(crate) struct NativeUpdater {
	#[cfg(all(target_os = "macos", not(test)))]
	bridge: Option<platform::Bridge>,
}
impl NativeUpdater {
	pub(crate) fn new() -> Self {
		Self {
			#[cfg(all(target_os = "macos", not(test)))]
			bridge: None,
		}
	}

	pub(crate) fn check(&mut self) -> Result<(), String> {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			if self.bridge.is_none() {
				self.bridge = Some(platform::Bridge::load()?);
			}
			self.bridge.as_mut().ok_or("The native updater is unavailable.")?.check()
		}
		#[cfg(any(not(target_os = "macos"), test))]
		{
			Err("Application installation is available from Decodex.app on macOS.".into())
		}
	}
}
