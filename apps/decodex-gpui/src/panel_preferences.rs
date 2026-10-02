//! Host-local default dimensions. Runtime resizing does not change these defaults.
use std::sync::atomic::{AtomicU32, Ordering};

use gpui::App;
#[cfg(all(target_os = "macos", not(test)))]
use objc2::{
	rc::Retained,
	runtime::{AnyClass, AnyObject},
};

static DEFAULTS: AtomicU32 = AtomicU32::new(0);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PanelDefaults {
	pub sidebar: u16,
	pub dock: u16,
}
impl PanelDefaults {
	pub fn configured() -> Self {
		let mut value = DEFAULTS.load(Ordering::Relaxed);

		if value == 0 {
			value = saved().unwrap_or(240 | (240 << 16));

			DEFAULTS.store(value, Ordering::Relaxed);
		}

		Self {
			sidebar: (value as u16).clamp(160, 480),
			dock: ((value >> 16) as u16).clamp(120, 480),
		}
	}

	pub fn select(self, cx: &mut App) {
		let value =
			u32::from(self.sidebar.clamp(160, 480)) | (u32::from(self.dock.clamp(120, 480)) << 16);

		DEFAULTS.store(value, Ordering::Relaxed);

		save(value);

		cx.refresh_windows();
	}
}
#[cfg(all(target_os = "macos", not(test)))]
fn saved() -> Option<u32> {
	unsafe {
		let defaults: Retained<AnyObject> = objc2::msg_send![
			AnyClass::get(c"NSUserDefaults").expect("Foundation"),
			standardUserDefaults
		];
		let key = objc2_foundation::NSString::from_str("DecodexPanelDefaults");
		let object: Option<Retained<AnyObject>> = objc2::msg_send![&*defaults, objectForKey: &*key];

		object.map(|_| {
			let value: isize = objc2::msg_send![&*defaults, integerForKey: &*key];

			value as u32
		})
	}
}
#[cfg(all(target_os = "macos", not(test)))]
fn save(value: u32) {
	unsafe {
		let defaults: Retained<AnyObject> = objc2::msg_send![
			AnyClass::get(c"NSUserDefaults").expect("Foundation"),
			standardUserDefaults
		];
		let key = objc2_foundation::NSString::from_str("DecodexPanelDefaults");
		let _: () = objc2::msg_send![&*defaults, setInteger: value as isize, forKey: &*key];
	}
}
#[cfg(not(all(target_os = "macos", not(test))))]
fn saved() -> Option<u32> {
	None
}
#[cfg(not(all(target_os = "macos", not(test))))]
fn save(_: u32) {}
