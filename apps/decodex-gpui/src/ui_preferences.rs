//! Cached host-local presentation preferences.
pub(crate) fn boolean(
	key: &str,
	cache: &std::sync::atomic::AtomicU8,
	value: Option<bool>,
	default: bool,
) -> bool {
	use std::sync::atomic::Ordering;

	if let Some(value) = value {
		stored_boolean(key, Some(value), default);

		cache.store(u8::from(value), Ordering::Relaxed);

		return value;
	}

	let cached = cache.load(Ordering::Relaxed);

	if cached != u8::MAX {
		return cached != 0;
	}

	let value = stored_boolean(key, None, default);

	cache.store(u8::from(value), Ordering::Relaxed);

	value
}

// Host-local presentation settings never change service state.
#[cfg(all(target_os = "macos", not(test)))]
fn stored_boolean(key: &str, value: Option<bool>, default: bool) -> bool {
	use objc2::{
		msg_send,
		rc::Retained,
		runtime::{AnyClass, AnyObject},
	};

	unsafe {
		let defaults: Retained<AnyObject> =
			msg_send![AnyClass::get(c"NSUserDefaults").expect("Foundation"), standardUserDefaults];
		let key = objc2_foundation::NSString::from_str(key);

		if let Some(value) = value {
			let _: () = msg_send![&*defaults, setBool: value, forKey: &*key];
		}

		let stored: Option<Retained<AnyObject>> = msg_send![&*defaults, objectForKey: &*key];

		if stored.is_none() {
			return default;
		}

		msg_send![&*defaults, boolForKey: &*key]
	}
}
#[cfg(not(all(target_os = "macos", not(test))))]
fn stored_boolean(_: &str, value: Option<bool>, default: bool) -> bool {
	value.unwrap_or(default)
}

#[cfg(test)]
mod tests {
	#[test]
	fn boolean_defaults_to_enabled_and_retains_an_explicit_disable() {
		let cache = std::sync::atomic::AtomicU8::new(u8::MAX);

		assert!(super::boolean("test-smooth-scroll", &cache, None, true));
		assert!(!super::boolean("test-smooth-scroll", &cache, Some(false), true));
		assert!(!super::boolean("test-smooth-scroll", &cache, None, true));
		assert!(super::boolean("test-smooth-scroll", &cache, Some(true), true));
	}
}
