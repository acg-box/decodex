//! Window material policy. Platform details stay behind one application interface.
#[cfg(all(target_os = "macos", not(test)))]
mod macos {
	use crate::ui_theme::{
		material::{Material, Surface},
		window_material::{Window, WindowBackgroundAppearance},
	};

	use objc2::{msg_send, rc::Retained};

	use objc2_app_kit::{
		NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
		NSVisualEffectView,
	};

	use raw_window_handle::{HasWindowHandle, RawWindowHandle};

	pub(super) fn apply(window: &mut Window, material: Material) -> bool {
		let Some(content) = content(window) else { return false };
		let identifier = objc2_foundation::NSString::from_str("decodex-window-material");
		let existing = content.subviews().iter().find(|view| unsafe {
			let tag: Option<Retained<objc2_foundation::NSString>> = msg_send![&**view, identifier];

			tag.is_some_and(|tag| tag == identifier)
		});
		let Some(class) = super::super::native_material_panel::material_class(material) else {
			if let Some(existing) = existing {
				existing.removeFromSuperview();
			}

			return false;
		};
		let surface = if let Some(existing) =
			existing.filter(|view| unsafe { msg_send![&**view, isKindOfClass: class] })
		{
			existing
		} else {
			// Only the backdrop changes. GPUI keeps its view, drawable and layout.
			for view in content.subviews().iter() {
				let tag: Option<Retained<objc2_foundation::NSString>> =
					unsafe { msg_send![&*view, identifier] };

				if tag.is_some_and(|tag| tag == identifier) {
					view.removeFromSuperview();
				}
			}

			let Some(surface) = super::super::native_material_panel::create_surface(material, 0.)
			else {
				return false;
			};

			unsafe {
				let _: () = msg_send![&*surface, setIdentifier: &*identifier];
				let _: () = msg_send![&*content, addSubview: &*surface, positioned: -1_isize, relativeTo: std::ptr::null::<NSView>()];
			}

			surface
		};

		surface.setFrame(content.bounds());

		unsafe {
			let _: () = msg_send![&*surface, setAutoresizingMask: 18_usize];
		}

		if material == Material::LiquidGlass {
			super::super::native_material_panel::apply_glass_style(
				&surface,
				Surface::Window,
				false,
			);
		}

		window.set_background_appearance(WindowBackgroundAppearance::Transparent);

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

	pub(super) fn remove_backdrop(window: &Window) {
		let Some(content) = content(window) else { return };
		let identifier = objc2_foundation::NSString::from_str("decodex-window-material");
		for view in content.subviews().iter() {
			let tag: Option<Retained<objc2_foundation::NSString>> =
				unsafe { msg_send![&*view, identifier] };
			if tag.is_some_and(|tag| tag == identifier) {
				view.removeFromSuperview();
			}
		}
	}

	fn content(window: &Window) -> Option<Retained<NSView>> {
		let handle = HasWindowHandle::window_handle(window).ok()?;
		let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
		// The live GPUI window owns this view; all calls run on the main thread.
		let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };

		view.window()?.contentView()
	}
}

use gpui::{Window, WindowBackgroundAppearance};

use crate::ui_theme::material::{Material, Surface};

#[cfg(all(target_os = "macos", not(test)))]
pub(crate) fn remove_backdrop(window: &Window) {
	macos::remove_backdrop(window);
}

pub(crate) fn configure(window: &mut Window) {
	apply(window, Surface::Window.material());
}

/// Prefer the selected system material; use platform blur if it is unavailable.
pub(crate) fn apply(window: &mut Window, material: Material) {
	#[cfg(all(target_os = "macos", not(test)))]
	if super::native_material_panel::owns_material(window) {
		return;
	}
	#[cfg(all(target_os = "macos", not(test)))]
	if macos::apply(window, material) {
		return;
	}

	let _ = material;

	window.set_background_appearance(WindowBackgroundAppearance::Blurred);

	#[cfg(all(target_os = "macos", not(test)))]
	macos::configure_vibrancy(window);
}
