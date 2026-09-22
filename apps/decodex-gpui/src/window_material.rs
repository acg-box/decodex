//! Window material policy. Platform details stay behind one application interface.
use gpui::{Window, WindowBackgroundAppearance};
use std::sync::atomic::{AtomicU8, Ordering};
static STYLE: AtomicU8 = AtomicU8::new(2);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum GlassStyle {
	#[default]
	Regular,
	Clear,
}
impl GlassStyle {
	pub(crate) fn configured() -> Self {
		let cached = STYLE.load(Ordering::Relaxed);
		if cached < 2 {
			return if cached == 1 { Self::Clear } else { Self::Regular };
		}
		#[allow(unused_mut)]
		let mut clear = std::env::var("DECODEX_GLASS_STYLE").as_deref() == Ok("clear");
		#[cfg(all(target_os = "macos", not(test)))]
		if std::env::var_os("DECODEX_GLASS_STYLE").is_none() {
			clear = macos::saved_clear();
		}
		STYLE.store(u8::from(clear), Ordering::Relaxed);
		if clear { Self::Clear } else { Self::Regular }
	}

	pub(crate) fn select(self, cx: &mut gpui::App) {
		STYLE.store(u8::from(self == Self::Clear), Ordering::Relaxed);
		#[cfg(all(target_os = "macos", not(test)))]
		macos::save_clear(self == Self::Clear);
		cx.defer(move |cx| {
			for handle in cx.windows() {
				let _ = handle.update(cx, |_, window, _| apply(window, self));
			}
			cx.refresh_windows();
		});
	}
}

pub(crate) fn configure(window: &mut Window) {
	apply(window, GlassStyle::configured());
}

/// Native Liquid Glass where available; GPUI's platform blur everywhere else.
/// GPUI can ignore blur on platforms without compositor support.
pub(crate) fn apply(window: &mut Window, style: GlassStyle) {
	#[cfg(all(target_os = "macos", not(test)))]
	if macos::apply(window, style) {
		return;
	}
	let _ = style;
	window.set_background_appearance(WindowBackgroundAppearance::Blurred);
	#[cfg(all(target_os = "macos", not(test)))]
	macos::configure_vibrancy(window);
}

#[cfg(all(target_os = "macos", not(test)))]
mod macos {
	use super::{GlassStyle, Window, WindowBackgroundAppearance};
	use objc2::{msg_send, rc::Retained, runtime::AnyClass};
	use objc2_app_kit::{
		NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
		NSVisualEffectView,
	};
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};

	// Host-local appearance preferences belong to AppKit, not account or conversation state.
	fn defaults() -> Retained<objc2::runtime::AnyObject> {
		unsafe {
			msg_send![AnyClass::get(c"NSUserDefaults").expect("Foundation"), standardUserDefaults]
		}
	}
	pub(super) fn saved_clear() -> bool {
		unsafe {
			msg_send![&*defaults(), boolForKey: &*objc2_foundation::NSString::from_str("DecodexGlassClear")]
		}
	}
	pub(super) fn save_clear(clear: bool) {
		unsafe {
			let _: () = msg_send![&*defaults(), setBool: clear, forKey: &*objc2_foundation::NSString::from_str("DecodexGlassClear")];
		}
	}

	fn content(window: &Window) -> Option<Retained<NSView>> {
		let handle = HasWindowHandle::window_handle(window).ok()?;
		let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
		// The live GPUI window owns this view; all calls run on the main thread.
		let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
		view.window()?.contentView()
	}

	// Public AppKit bridge; reference longbridge/gpui-component#2440.
	pub(super) fn apply(window: &mut Window, style: GlassStyle) -> bool {
		let Some(content) = content(window) else { return false };
		let Some(class) = AnyClass::get(c"NSGlassEffectView") else { return false };
		let existing = content
			.subviews()
			.iter()
			.find(|child| unsafe { msg_send![&**child, isKindOfClass: class] });
		// Foreground panels own their GPUI view through contentView. Never remove
		// that view when updating the window-backdrop preference.
		if let Some(glass) = existing.as_ref() {
			let foreground: Option<Retained<NSView>> = unsafe { msg_send![&**glass, contentView] };
			if foreground.is_some() {
				unsafe {
					let _: () = msg_send![&**glass, setStyle: match style { GlassStyle::Regular => 0isize, GlassStyle::Clear => 1isize }];
				}
				return true;
			}
		}
		let reduce_transparency: bool = unsafe {
			let workspace: Retained<objc2::runtime::AnyObject> =
				msg_send![AnyClass::get(c"NSWorkspace").expect("AppKit"), sharedWorkspace];
			msg_send![&*workspace, accessibilityDisplayShouldReduceTransparency]
		};
		if reduce_transparency || std::env::var_os("DECODEX_DISABLE_LIQUID_GLASS").is_some() {
			if let Some(glass) = existing {
				glass.removeFromSuperview();
			}
			return false;
		}
		unsafe {
			let glass: Retained<NSView> = existing.unwrap_or_else(|| msg_send![class, new]);
			let _: () = msg_send![&*glass, setStyle: match style { GlassStyle::Regular => 0isize, GlassStyle::Clear => 1isize }];
			glass.setFrame(content.bounds());
			let _: () = msg_send![&*glass, setAutoresizingMask: 18usize];
			window.set_background_appearance(WindowBackgroundAppearance::Transparent);
			if glass.superview().is_none() {
				let _: () = msg_send![&*content, addSubview: &*glass, positioned: -1isize, relativeTo: std::ptr::null::<NSView>()];
			}
		}
		true
	}

	pub(super) fn configure_vibrancy(window: &Window) {
		let Some(content) = content(window) else { return };
		for child in content.subviews().iter() {
			if let Some(effect) = child.downcast_ref::<NSVisualEffectView>() {
				effect.setMaterial(NSVisualEffectMaterial::Sidebar);
				effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
				effect.setState(NSVisualEffectState::Active);
			}
		}
	}
}
