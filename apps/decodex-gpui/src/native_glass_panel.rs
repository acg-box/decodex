//! A foreground-owning native material for small GPUI child windows.
//! UI state and event handlers remain in GPUI; AppKit owns only composition.
use std::{env, ptr};

use gpui::{Bounds, Pixels, Window, WindowBackgroundAppearance};
use objc2::{
	self,
	rc::Retained,
	runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
};
use objc2_app_kit::{NSView, NSWindow, NSWindowCollectionBehavior};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

thread_local! {
	// Non-owning handles; the corresponding GlassPanel registers and removes each entry.
	static ATTACHED_WINDOWS: std::cell::RefCell<std::collections::HashMap<usize, gpui::AnyWindowHandle>> = Default::default();
}

/// Read the native material for opt-in frame diagnostics.
pub(crate) fn observed_style(window: &Window) -> Option<isize> {
	let content = native(window)?.contentView()?;
	let class = AnyClass::get(c"NSGlassEffectView")?;
	content.subviews().iter().find_map(|view| unsafe {
		let glass: bool = objc2::msg_send![&*view, isKindOfClass: class];
		glass.then(|| objc2::msg_send![&*view, style])
	})
}

/// Retains the installed glass so style updates cannot mistake it for a window backdrop.
pub(crate) struct GlassPanel {
	glass: Retained<NSView>,
	foreground: Retained<NSView>,
	native: Retained<NSWindow>,
	parent: Retained<NSWindow>,
	frame: Option<NSRect>,
	visible: bool,
	window_observer: Retained<AnyObject>,
}
impl GlassPanel {
	pub(crate) fn install(parent: &Window, window: &mut Window, radius: f64) -> Option<Self> {
		Self::install_surface(parent, window, Some(radius))
	}

	pub(crate) fn install_overlay(parent: &Window, window: &mut Window) -> Option<Self> {
		Self::install_surface(parent, window, None)
	}

	fn install_surface(parent: &Window, window: &mut Window, radius: Option<f64>) -> Option<Self> {
		if radius.is_some() && !available() {
			return None;
		}

		let parent = native(parent)?;
		let native = native(window)?;
		let gpu = view(window)?;
		let content = native.contentView()?;

		native.setCollectionBehavior(
			NSWindowCollectionBehavior::Transient
				| NSWindowCollectionBehavior::IgnoresCycle
				| NSWindowCollectionBehavior::FullScreenAuxiliary,
		);
		native.setTitle(&NSString::from_str(if radius.is_some() {
			"Decodex Composer"
		} else {
			"Decodex Status"
		}));

		let class = AnyClass::get(if radius.is_some() { c"NSGlassEffectView" } else { c"NSView" })?;

		window.set_background_appearance(WindowBackgroundAppearance::Transparent);

		unsafe {
			let glass: Retained<NSView> = objc2::msg_send![class, new];

			glass.setFrame(content.bounds());

			let _: () = objc2::msg_send![&*glass, setAutoresizingMask: 18_usize];

			gpu.removeFromSuperview();
			// GPUI owns Metal drawable sizing. Keep AppKit autoresizing off this
			// view; otherwise a parent resize can scale it a second time.
			let _: () = objc2::msg_send![&*gpu, setAutoresizingMask: 0_usize];
			// This child often stays non-key while history owns keyboard focus. Match
			// GPUI's popup tracking so hover, cursor and tooltips still receive movement.
			let tracking: Retained<AnyObject> = objc2::msg_send![
				objc2::msg_send![AnyClass::get(c"NSTrackingArea")?, alloc],
				initWithRect: NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)),
				options: 0x283_usize,
				owner: &*gpu,
				userInfo: ptr::null::<AnyObject>()
			];
			let _: () = objc2::msg_send![&*gpu, addTrackingArea: &*tracking];

			if let Some(radius) = radius {
				let _: () = objc2::msg_send![&*glass, setCornerRadius: radius];
				// Glass may lay out its content view itself. Give it a plain host,
				// so its layout never changes the Metal view behind GPUI's back.
				let host: Retained<NSView> = objc2::msg_send![AnyClass::get(c"NSView")?, new];
				host.setFrame(content.bounds());
				let _: () = objc2::msg_send![&*host, setAutoresizingMask: 18_usize];
				host.addSubview(&gpu);
				let _: () = objc2::msg_send![&*glass, setContentView: &*host];
			} else {
				glass.addSubview(&gpu);

				let _: () = objc2::msg_send![&*native, setHasShadow: false];
				let _: () = objc2::msg_send![&*native, setAlphaValue: 0.0_f64];
			}

			content.addSubview(&glass);

			let _: () = objc2::msg_send![&*native, setStyleMask: 0_usize];
			let _: () = objc2::msg_send![&*native, setLevel: 0_isize];
			let _: () = objc2::msg_send![&*native, setMovable: false];
			let _: () = objc2::msg_send![&*native, setExcludedFromWindowsMenu: true];
			let _: () = objc2::msg_send![&*parent, addChildWindow: &*native, ordered: 1_isize];
			let window_observer = observe_panel_window(&native)?;
			if radius.is_some() {
				update_focus_material(&native);
			}

			ATTACHED_WINDOWS.with(|windows| {
				windows
					.borrow_mut()
					.insert(&*native as *const NSWindow as usize, Window::window_handle(window));
			});

			Some(Self {
				glass,
				foreground: gpu,
				native,
				parent,
				frame: None,
				visible: false,
				window_observer,
			})
		}
	}

	/// GPUI bounds use a top-left origin; AppKit screen conversion uses bottom-left.
	pub(crate) fn place(&mut self, bounds: Bounds<Pixels>) -> bool {
		let Some(content) = self.parent.contentView() else { return false };
		let frame = self.parent.convertRectToScreen(NSRect::new(
			NSPoint::new(
				f64::from(f32::from(bounds.origin.x)),
				content.bounds().size.height
					- f64::from(f32::from(bounds.origin.y + bounds.size.height)),
			),
			NSSize::new(
				f64::from(f32::from(bounds.size.width)),
				f64::from(f32::from(bounds.size.height)),
			),
		));

		if self.frame != Some(frame) {
			self.native.setFrame_display(frame, false);
			if let Some(content) = self.native.contentView() {
				self.glass.setFrame(content.bounds());
				// Call the resize hook, not setFrame:, so the drawable and GPUI
				// viewport use the same size. The foreground has one layout owner.
				self.foreground.setFrameSize(content.bounds().size);
				self.foreground.setFrameOrigin(NSPoint::new(0., 0.));
			}

			self.frame = Some(frame);

			return true;
		}

		false
	}

	pub(crate) fn focus_text(&self) {
		self.native.makeFirstResponder(Some(&self.foreground));
	}

	pub(crate) fn set_opacity(&self, opacity: f32) {
		unsafe {
			let _: () = objc2::msg_send![&*self.native, setAlphaValue: f64::from(opacity)];
		}
	}

	pub(crate) fn set_visible(&mut self, visible: bool) {
		let visible = visible && self.parent.isVisible() && !self.parent.isMiniaturized();

		unsafe {
			if visible {
				let attached = self
					.native
					.parentWindow()
					.as_deref()
					.is_some_and(|parent| ptr::eq(parent, &*self.parent));

				if self.visible && self.native.isVisible() && attached {
					return;
				}
				// orderOut and parent activation can change native ordering without
				// changing GPUI state. Restore the relationship, not global frontmost.
				if !attached {
					let _: () = objc2::msg_send![&*self.parent, addChildWindow: &*self.native, ordered: 1_isize];
				}

				let _: () = objc2::msg_send![&*self.native, orderWindow: 1_isize, relativeTo: self.parent.windowNumber()];
			} else if self.visible || self.native.isVisible() {
				let _: () = objc2::msg_send![&*self.native, orderOut: ptr::null::<NSWindow>()];
			}
		}
		self.visible = visible;
	}
}

impl Drop for GlassPanel {
	fn drop(&mut self) {
		ATTACHED_WINDOWS.with(|windows| {
			windows.borrow_mut().remove(&(&*self.native as *const NSWindow as usize));
		});

		unsafe {
			let center: Retained<AnyObject> = objc2::msg_send![
				AnyClass::get(c"NSNotificationCenter").expect("Foundation"),
				defaultCenter
			];
			let _: () = objc2::msg_send![&*center, removeObserver: &*self.window_observer];
		}

		self.set_visible(false);

		unsafe {
			let _: () = objc2::msg_send![&*self.parent, removeChildWindow: &*self.native];
		}
	}
}

/// Attached panels own their material independently of window backdrops.
pub(crate) fn owns_material(window: &Window) -> bool {
	ATTACHED_WINDOWS
		.with(|windows| windows.borrow().values().any(|handle| *handle == window.window_handle()))
}

pub(crate) fn available() -> bool {
	AnyClass::get(c"NSGlassEffectView").is_some()
		&& env::var_os("DECODEX_DISABLE_LIQUID_GLASS").is_none()
		&& !reduced_transparency()
}

fn reduced_transparency() -> bool {
	unsafe {
		let workspace: Retained<AnyObject> =
			objc2::msg_send![AnyClass::get(c"NSWorkspace").expect("AppKit"), sharedWorkspace];

		objc2::msg_send![&*workspace, accessibilityDisplayShouldReduceTransparency]
	}
}

fn view(window: &Window) -> Option<Retained<NSView>> {
	let handle = HasWindowHandle::window_handle(window).ok()?;
	let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };

	// Called on the main thread while the GPUI window owns the native view.
	unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
}

fn native(window: &Window) -> Option<Retained<NSWindow>> {
	view(window)?.window()
}

// GPUI delivers its activation state asynchronously, after AppKit's focus frame.
// Update the material in that native transaction, without waiting for GPUI render.
fn update_focus_material(window: &NSWindow) {
	let Some(content) = window.contentView() else { return };
	let Some(class) = AnyClass::get(c"NSGlassEffectView") else { return };
	let Some(glass) = content
		.subviews()
		.iter()
		.find(|view| unsafe { objc2::msg_send![&**view, isKindOfClass: class] })
	else {
		return;
	};
	let clear = window.isKeyWindow();
	unsafe {
		let style: isize = objc2::msg_send![&*glass, style];
		if style == isize::from(clear) {
			return;
		}
		let _: () = objc2::msg_send![&*glass, setStyle: isize::from(clear)];
		// Keep Clear readable without an opaque fill over the native material.
		let tint: Option<Retained<AnyObject>> = clear.then(|| {
			objc2::msg_send![AnyClass::get(c"NSColor").expect("AppKit"),
				colorWithSRGBRed: 0.10_f64, green: 0.11_f64, blue: 0.13_f64, alpha: 0.18_f64]
		});
		let _: () = objc2::msg_send![&*glass, setTintColor: tint.as_deref()];
	}
}

/// Keep native focus, material, and the workspace's main-window status in sync.
fn observe_panel_window(window: &NSWindow) -> Option<Retained<AnyObject>> {
	unsafe extern "C-unwind" fn focus_changed(_: &AnyObject, _: Sel, notification: &AnyObject) {
		let child: Option<Retained<NSWindow>> = unsafe { objc2::msg_send![notification, object] };
		if let Some(child) = child {
			update_focus_material(&child);
		}
	}

	unsafe extern "C-unwind" fn restore_main(_: &AnyObject, _: Sel, notification: &AnyObject) {
		unsafe {
			let child: Option<Retained<NSWindow>> = objc2::msg_send![notification, object];

			if let Some(parent) = child.and_then(|child| child.parentWindow())
				&& parent.isVisible()
			{
				parent.makeMainWindow();
			}
		}
	}

	let class = if let Some(class) = AnyClass::get(c"DecodexPanelWindowObserver") {
		class
	} else {
		let mut builder =
			ClassBuilder::new(c"DecodexPanelWindowObserver", AnyClass::get(c"NSObject")?)?;

		unsafe {
			builder.add_method(
				objc2::sel!(restoreMain:),
				restore_main as unsafe extern "C-unwind" fn(_, _, _),
			);
			builder.add_method(
				objc2::sel!(focusChanged:),
				focus_changed as unsafe extern "C-unwind" fn(_, _, _),
			);
		}

		builder.register()
	};

	unsafe {
		let observer: Retained<AnyObject> = objc2::msg_send![class, new];
		let center: Retained<AnyObject> =
			objc2::msg_send![AnyClass::get(c"NSNotificationCenter")?, defaultCenter];
		let name = NSString::from_str("NSWindowDidBecomeMainNotification");
		let _: () = objc2::msg_send![&*center, addObserver: &*observer, selector: objc2::sel!(restoreMain:), name: &*name, object: window];
		for name in ["NSWindowDidBecomeKeyNotification", "NSWindowDidResignKeyNotification"] {
			let name = NSString::from_str(name);
			let _: () = objc2::msg_send![&*center, addObserver: &*observer, selector: objc2::sel!(focusChanged:), name: &*name, object: window];
		}

		Some(observer)
	}
}
