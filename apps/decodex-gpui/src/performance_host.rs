//! Read-only host counters. GPU counters describe the whole device, not one app.
#[derive(Clone, Copy, Default)]
pub(super) struct HostSample {
	pub gpu_percent: Option<f64>,
	pub cpu_seconds: Option<f64>,
	pub peak_memory_bytes: Option<f64>,
}
impl HostSample {
	pub(super) fn read() -> Self {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			macos::sample()
		}
		#[cfg(not(all(target_os = "macos", not(test))))]
		Self::default()
	}
}
pub(super) fn display_hz(window: &gpui::Window) -> Option<f64> {
	#[cfg(all(target_os = "macos", not(test)))]
	{
		macos::display_hz(window)
	}
	#[cfg(not(all(target_os = "macos", not(test))))]
	{
		let _ = window;
		None
	}
}

#[cfg(all(target_os = "macos", not(test)))]
mod macos {
	use super::HostSample;
	use objc2::{rc::Retained, runtime::AnyObject};
	use objc2_foundation::NSString;
	use raw_window_handle::RawWindowHandle;
	use std::{
		ffi::{c_char, c_void},
		ptr,
	};

	#[link(name = "IOKit", kind = "framework")]
	unsafe extern "C" {
		fn IOServiceMatching(name: *const c_char) -> *const c_void;
		fn IOServiceGetMatchingServices(
			port: u32,
			matching: *const c_void,
			iterator: *mut u32,
		) -> i32;
		fn IOIteratorNext(iterator: u32) -> u32;
		fn IORegistryEntryCreateCFProperty(
			entry: u32,
			key: *const c_void,
			allocator: *const c_void,
			options: u32,
		) -> *const c_void;
		fn IOObjectRelease(object: u32) -> i32;
	}
	#[link(name = "CoreFoundation", kind = "framework")]
	unsafe extern "C" {
		fn CFGetTypeID(value: *const c_void) -> usize;
		fn CFDictionaryGetTypeID() -> usize;
		fn CFNumberGetTypeID() -> usize;
		fn CFDictionaryGetValue(dictionary: *const c_void, key: *const c_void) -> *const c_void;
		fn CFNumberGetValue(number: *const c_void, kind: isize, value: *mut c_void) -> u8;
		fn CFRelease(value: *const c_void);
	}
	struct Io(u32);
	impl Drop for Io {
		fn drop(&mut self) {
			unsafe {
				IOObjectRelease(self.0);
			}
		}
	}
	struct Property(*const c_void);
	impl Drop for Property {
		fn drop(&mut self) {
			unsafe {
				CFRelease(self.0);
			}
		}
	}

	fn number(dictionary: &Property, key: &str) -> Option<f64> {
		let key = NSString::from_str(key);
		unsafe {
			let value = CFDictionaryGetValue(dictionary.0, (&*key as *const NSString).cast());
			if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
				return None;
			}
			let mut result = 0_f64;
			// kCFNumberDoubleType; convert the driver's integer counter without assumptions about
			// its width.
			(CFNumberGetValue(value, 13, (&mut result as *mut f64).cast()) != 0
				&& result.is_finite())
			.then_some(result)
		}
	}
	fn gpu() -> Option<f64> {
		unsafe {
			let matching = IOServiceMatching(c"IOAccelerator".as_ptr());
			if matching.is_null() {
				return None;
			}
			let mut iterator = 0;
			// IOKit consumes the matching dictionary on both success and failure.
			if IOServiceGetMatchingServices(0, matching, &mut iterator) != 0 || iterator == 0 {
				return None;
			}
			let iterator = Io(iterator);
			let key = NSString::from_str("PerformanceStatistics");
			let mut result = None;
			loop {
				let entry = IOIteratorNext(iterator.0);
				if entry == 0 {
					break;
				}
				let entry = Io(entry);
				let property = IORegistryEntryCreateCFProperty(
					entry.0,
					(&*key as *const NSString).cast(),
					ptr::null(),
					0,
				);
				if property.is_null() {
					continue;
				}
				let property = Property(property);
				if CFGetTypeID(property.0) != CFDictionaryGetTypeID() {
					continue;
				}
				let Some(percent) =
					number(&property, "Device Utilization %").filter(|p| (0. ..=100.).contains(p))
				else {
					continue;
				};
				if result.is_none_or(|old| percent >= old) {
					result = Some(percent);
				}
			}
			result
		}
	}
	pub(super) fn sample() -> HostSample {
		let mut result = HostSample { gpu_percent: gpu(), ..HostSample::default() };
		unsafe {
			let mut usage: libc::rusage = std::mem::zeroed();
			if libc::getrusage(libc::RUSAGE_SELF, &mut usage) == 0 {
				result.cpu_seconds = Some(
					usage.ru_utime.tv_sec as f64
						+ usage.ru_utime.tv_usec as f64 / 1_000_000.
						+ usage.ru_stime.tv_sec as f64
						+ usage.ru_stime.tv_usec as f64 / 1_000_000.,
				);
				result.peak_memory_bytes = Some(usage.ru_maxrss as f64);
			}
		}
		result
	}
	pub(super) fn display_hz(window: &gpui::Window) -> Option<f64> {
		let RawWindowHandle::AppKit(handle) =
			raw_window_handle::HasWindowHandle::window_handle(window).ok()?.as_raw()
		else {
			return None;
		};
		unsafe {
			let view = Retained::<AnyObject>::retain(handle.ns_view.as_ptr().cast())?;
			let native: Option<Retained<AnyObject>> = objc2::msg_send![&*view, window];
			let screen: Option<Retained<AnyObject>> = objc2::msg_send![&*native?, screen];
			let rate: isize = objc2::msg_send![&*screen?, maximumFramesPerSecond];
			(rate > 0).then_some(rate as f64)
		}
	}
}
