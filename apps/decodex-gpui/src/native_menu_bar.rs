//! Checked same-process bridge to the original native Swift menu-bar panel.

#[cfg(all(target_os = "macos", not(test)))]
use std::{
	ffi::{CString, c_void},
	marker::PhantomData,
	os::unix::ffi::OsStrExt as _,
	path::{Path, PathBuf},
	rc::Rc,
};

#[cfg(all(target_os = "macos", not(test)))]
const MENU_BAR_ABI_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
	dead_code,
	reason = "failure variants are selected by the production platform and staged-bundle path"
)]
pub(crate) enum NativeMenuBarFailure {
	NotBundled,
	LibraryUnavailable,
	Incompatible,
	HostUnavailable,
	ApplyFailed,
	LoginItemFailed,
	UnsupportedPlatform,
}

impl NativeMenuBarFailure {
	pub(crate) const fn detail(self) -> &'static str {
		match self {
			Self::NotBundled => "The native menu bar is available from staged Decodex.app builds.",
			Self::LibraryUnavailable | Self::Incompatible | Self::HostUnavailable =>
				"Restart Decodex.",
			Self::ApplyFailed => "The native Swift menu bar refused the requested visibility.",
			Self::LoginItemFailed => "macOS refused the requested login-item operation.",
			Self::UnsupportedPlatform => "The menu-bar surface is available only on macOS.",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LaunchAtLoginState {
	NotRegistered,
	Enabled,
	RequiresApproval,
	NotFound,
	OperationFailed,
}

impl LaunchAtLoginState {
	pub(crate) const fn is_requested(self) -> bool {
		matches!(self, Self::Enabled | Self::RequiresApproval)
	}

	#[cfg(all(target_os = "macos", not(test)))]
	const fn from_raw(raw: i32) -> Self {
		match raw {
			0 => Self::NotRegistered,
			1 => Self::Enabled,
			2 => Self::RequiresApproval,
			3 => Self::NotFound,
			_ => Self::OperationFailed,
		}
	}
}

pub(crate) struct NativeMenuBarHost {
	#[cfg(all(target_os = "macos", not(test)))]
	bridge: Result<SwiftMenuBarBridge, NativeMenuBarFailure>,
	#[cfg(any(test, not(target_os = "macos")))]
	visible: bool,
	#[cfg(any(test, not(target_os = "macos")))]
	launch_at_login: LaunchAtLoginState,
	#[cfg(any(test, not(target_os = "macos")))]
	launched_as_login_item: bool,
}

impl NativeMenuBarHost {
	pub(crate) fn new() -> Self {
		Self {
			#[cfg(all(target_os = "macos", not(test)))]
			bridge: SwiftMenuBarBridge::load(),
			#[cfg(any(test, not(target_os = "macos")))]
			visible: false,
			#[cfg(any(test, not(target_os = "macos")))]
			launch_at_login: LaunchAtLoginState::NotRegistered,
			#[cfg(any(test, not(target_os = "macos")))]
			launched_as_login_item: false,
		}
	}

	pub(crate) fn apply(&mut self, enabled: bool) -> Result<bool, NativeMenuBarFailure> {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			self.bridge.as_mut().map_err(|failure| *failure)?.apply(enabled)
		}
		#[cfg(test)]
		{
			self.visible = enabled;
			Ok(self.visible)
		}
		#[cfg(all(not(test), not(target_os = "macos")))]
		{
			let _ = enabled;
			Err(NativeMenuBarFailure::UnsupportedPlatform)
		}
	}

	pub(crate) fn launch_at_login_state(
		&mut self,
	) -> Result<LaunchAtLoginState, NativeMenuBarFailure> {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			Ok(self.bridge.as_mut().map_err(|failure| *failure)?.launch_at_login_state())
		}
		#[cfg(test)]
		{
			Ok(self.launch_at_login)
		}
		#[cfg(all(not(test), not(target_os = "macos")))]
		{
			Err(NativeMenuBarFailure::UnsupportedPlatform)
		}
	}

	pub(crate) fn set_launch_at_login(
		&mut self,
		enabled: bool,
	) -> Result<LaunchAtLoginState, NativeMenuBarFailure> {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			let state =
				self.bridge.as_mut().map_err(|failure| *failure)?.set_launch_at_login(enabled);
			if state == LaunchAtLoginState::OperationFailed {
				Err(NativeMenuBarFailure::LoginItemFailed)
			} else {
				Ok(state)
			}
		}
		#[cfg(test)]
		{
			self.launch_at_login = if enabled {
				LaunchAtLoginState::Enabled
			} else {
				LaunchAtLoginState::NotRegistered
			};
			Ok(self.launch_at_login)
		}
		#[cfg(all(not(test), not(target_os = "macos")))]
		{
			let _ = enabled;
			Err(NativeMenuBarFailure::UnsupportedPlatform)
		}
	}

	pub(crate) fn open_login_items_settings(&mut self) -> Result<(), NativeMenuBarFailure> {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			if self.bridge.as_mut().map_err(|failure| *failure)?.open_login_items_settings() {
				Ok(())
			} else {
				Err(NativeMenuBarFailure::LoginItemFailed)
			}
		}
		#[cfg(test)]
		{
			Ok(())
		}
		#[cfg(all(not(test), not(target_os = "macos")))]
		{
			Err(NativeMenuBarFailure::UnsupportedPlatform)
		}
	}

	pub(crate) fn was_launched_as_login_item(&self) -> bool {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			self.bridge.as_ref().is_ok_and(|bridge| bridge.launched_as_login_item)
		}
		#[cfg(any(test, not(target_os = "macos")))]
		{
			self.launched_as_login_item
		}
	}
}

#[cfg(all(target_os = "macos", not(test)))]
type VersionFn = unsafe extern "C" fn() -> u32;
#[cfg(all(target_os = "macos", not(test)))]
type CreateFn = unsafe extern "C" fn() -> *mut c_void;
#[cfg(all(target_os = "macos", not(test)))]
type SetVisibleFn = unsafe extern "C" fn(*mut c_void, bool) -> bool;
#[cfg(all(target_os = "macos", not(test)))]
type DestroyFn = unsafe extern "C" fn(*mut c_void);
#[cfg(all(target_os = "macos", not(test)))]
type LoginItemStatusFn = unsafe extern "C" fn(*mut c_void) -> i32;
#[cfg(all(target_os = "macos", not(test)))]
type SetLaunchAtLoginFn = unsafe extern "C" fn(*mut c_void, bool) -> i32;
#[cfg(all(target_os = "macos", not(test)))]
type OpenLoginItemsSettingsFn = unsafe extern "C" fn(*mut c_void) -> bool;
#[cfg(all(target_os = "macos", not(test)))]
type WasLaunchedAsLoginItemFn = unsafe extern "C" fn() -> bool;

#[cfg(all(target_os = "macos", not(test)))]
struct SwiftMenuBarBridge {
	// The image intentionally stays loaded until process exit because Swift termination uses a
	// Task.
	_image: *mut c_void,
	host: *mut c_void,
	set_visible: SetVisibleFn,
	launch_at_login_status: LoginItemStatusFn,
	set_launch_at_login: SetLaunchAtLoginFn,
	open_login_items_settings: OpenLoginItemsSettingsFn,
	destroy: DestroyFn,
	launched_as_login_item: bool,
	_not_send_or_sync: PhantomData<Rc<()>>,
}

#[cfg(all(target_os = "macos", not(test)))]
impl SwiftMenuBarBridge {
	fn load() -> Result<Self, NativeMenuBarFailure> {
		let executable = std::env::current_exe().map_err(|_| NativeMenuBarFailure::NotBundled)?;
		let library = bundled_library_path(&executable)?;
		let metadata = std::fs::symlink_metadata(&library)
			.map_err(|_| NativeMenuBarFailure::LibraryUnavailable)?;
		if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
			return Err(NativeMenuBarFailure::LibraryUnavailable);
		}
		let encoded = CString::new(library.as_os_str().as_bytes())
			.map_err(|_| NativeMenuBarFailure::LibraryUnavailable)?;
		// SAFETY: the path is a checked regular file in this app's Frameworks directory.
		let image = unsafe { libc::dlopen(encoded.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
		if image.is_null() {
			return Err(NativeMenuBarFailure::LibraryUnavailable);
		}

		// SAFETY: every symbol is verified non-null before it is copied to its exact C ABI type.
		let abi: VersionFn = unsafe { symbol(image, c"decodex_menu_bar_abi_version")? };
		// SAFETY: the version function has no arguments and no side effects outside the loaded
		// image.
		if unsafe { abi() } != MENU_BAR_ABI_VERSION {
			return Err(NativeMenuBarFailure::Incompatible);
		}
		// SAFETY: each exported symbol has a fixed ABI that is covered by Swift and Rust tests.
		let create: CreateFn = unsafe { symbol(image, c"decodex_menu_bar_create")? };
		// SAFETY: same exact checked ABI boundary.
		let set_visible: SetVisibleFn = unsafe { symbol(image, c"decodex_menu_bar_set_visible")? };
		// SAFETY: same exact checked ABI boundary.
		let launch_at_login_status: LoginItemStatusFn =
			unsafe { symbol(image, c"decodex_menu_bar_launch_at_login_status")? };
		// SAFETY: same exact checked ABI boundary.
		let set_launch_at_login: SetLaunchAtLoginFn =
			unsafe { symbol(image, c"decodex_menu_bar_set_launch_at_login")? };
		// SAFETY: same exact checked ABI boundary.
		let open_login_items_settings: OpenLoginItemsSettingsFn =
			unsafe { symbol(image, c"decodex_menu_bar_open_login_items_settings")? };
		// SAFETY: same exact checked ABI boundary.
		let was_launched_as_login_item: WasLaunchedAsLoginItemFn =
			unsafe { symbol(image, c"decodex_app_was_launched_as_login_item")? };
		// SAFETY: same exact checked ABI boundary.
		let destroy: DestroyFn = unsafe { symbol(image, c"decodex_menu_bar_destroy")? };
		// SAFETY: the function has no arguments and reads only the current launch Apple event.
		let launched_as_login_item = unsafe { was_launched_as_login_item() };
		// SAFETY: loading happens inside GPUI's macOS main-thread application callback.
		let host = unsafe { create() };
		if host.is_null() {
			return Err(NativeMenuBarFailure::HostUnavailable);
		}

		Ok(Self {
			_image: image,
			host,
			set_visible,
			launch_at_login_status,
			set_launch_at_login,
			open_login_items_settings,
			destroy,
			launched_as_login_item,
			_not_send_or_sync: PhantomData,
		})
	}

	fn apply(&mut self, enabled: bool) -> Result<bool, NativeMenuBarFailure> {
		// SAFETY: `host` is retained by Swift and all calls occur on the GPUI main thread.
		let visible = unsafe { (self.set_visible)(self.host, enabled) };
		if visible == enabled { Ok(visible) } else { Err(NativeMenuBarFailure::ApplyFailed) }
	}

	fn launch_at_login_state(&mut self) -> LaunchAtLoginState {
		// SAFETY: `host` is retained by Swift and the call remains on the main thread.
		LaunchAtLoginState::from_raw(unsafe { (self.launch_at_login_status)(self.host) })
	}

	fn set_launch_at_login(&mut self, enabled: bool) -> LaunchAtLoginState {
		// SAFETY: same retained main-thread host and exact checked ABI boundary.
		LaunchAtLoginState::from_raw(unsafe { (self.set_launch_at_login)(self.host, enabled) })
	}

	fn open_login_items_settings(&mut self) -> bool {
		// SAFETY: same retained main-thread host and exact checked ABI boundary.
		unsafe { (self.open_login_items_settings)(self.host) }
	}
}

#[cfg(all(target_os = "macos", not(test)))]
impl Drop for SwiftMenuBarBridge {
	fn drop(&mut self) {
		if !self.host.is_null() {
			// SAFETY: this object is main-thread confined and destroys its one retained Swift host
			// once.
			unsafe { (self.destroy)(self.host) };
			self.host = std::ptr::null_mut();
		}
	}
}

#[cfg(all(target_os = "macos", not(test)))]
fn bundled_library_path(executable: &Path) -> Result<PathBuf, NativeMenuBarFailure> {
	let macos = executable.parent().ok_or(NativeMenuBarFailure::NotBundled)?;
	if macos.file_name().and_then(|name| name.to_str()) != Some("MacOS") {
		return Err(NativeMenuBarFailure::NotBundled);
	}
	let contents = macos.parent().ok_or(NativeMenuBarFailure::NotBundled)?;
	if contents.file_name().and_then(|name| name.to_str()) != Some("Contents") {
		return Err(NativeMenuBarFailure::NotBundled);
	}
	Ok(contents.join("Frameworks/libDecodexMenuBar.dylib"))
}

#[cfg(all(target_os = "macos", not(test)))]
unsafe fn symbol<T: Copy>(
	image: *mut c_void,
	name: &std::ffi::CStr,
) -> Result<T, NativeMenuBarFailure> {
	// SAFETY: `image` comes from `dlopen`, and `name` is a static nul-terminated symbol name.
	let address = unsafe { libc::dlsym(image, name.as_ptr()) };
	if address.is_null() || std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
		return Err(NativeMenuBarFailure::Incompatible);
	}
	// SAFETY: the caller selects the exact exported C function type after the size check.
	Ok(unsafe { std::mem::transmute_copy(&address) })
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn simulated_bridge_applies_visibility_idempotently() {
		let mut bridge = NativeMenuBarHost::new();
		assert_eq!(bridge.apply(true), Ok(true));
		assert_eq!(bridge.apply(true), Ok(true));
		assert_eq!(bridge.apply(false), Ok(false));
	}

	#[test]
	fn simulated_bridge_keeps_login_item_state_separate_from_menu_visibility() {
		let mut bridge = NativeMenuBarHost::new();
		assert_eq!(bridge.launch_at_login_state(), Ok(LaunchAtLoginState::NotRegistered));
		assert_eq!(bridge.set_launch_at_login(true), Ok(LaunchAtLoginState::Enabled));
		assert_eq!(bridge.apply(false), Ok(false));
		assert_eq!(bridge.launch_at_login_state(), Ok(LaunchAtLoginState::Enabled));
		assert!(!bridge.was_launched_as_login_item());
		assert!(LaunchAtLoginState::RequiresApproval.is_requested());
		assert!(!LaunchAtLoginState::NotFound.is_requested());
	}
}
