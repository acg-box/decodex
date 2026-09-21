//! A foreground-owning native material for small GPUI child windows.
//! UI state and event handlers remain in GPUI; AppKit owns only composition.
use gpui::{Bounds, Pixels, Window, WindowBackgroundAppearance};
use objc2::{
	msg_send,
	rc::Retained,
	runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
	sel,
};
use objc2_app_kit::{NSView, NSWindow, NSWindowCollectionBehavior};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(crate) fn available() -> bool {
	AnyClass::get(c"NSGlassEffectView").is_some()
		&& std::env::var_os("DECODEX_DISABLE_LIQUID_GLASS").is_none()
		&& !reduced_transparency()
}

fn reduced_transparency() -> bool {
	unsafe {
		let workspace: Retained<objc2::runtime::AnyObject> =
			msg_send![AnyClass::get(c"NSWorkspace").expect("AppKit"), sharedWorkspace];
		msg_send![&*workspace, accessibilityDisplayShouldReduceTransparency]
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

/// Keep the workspace main while an attached control receives keyboard input.
fn observe_main_window(window: &NSWindow) -> Option<Retained<AnyObject>> {
	unsafe extern "C-unwind" fn restore_main(_: &AnyObject, _: Sel, notification: &AnyObject) {
		unsafe {
			let child: Option<Retained<NSWindow>> = msg_send![notification, object];
			if let Some(parent) = child.and_then(|child| child.parentWindow())
				&& parent.isVisible()
			{
				parent.makeMainWindow();
			}
		}
	}
	let class = if let Some(class) = AnyClass::get(c"DecodexMainWindowObserver") {
		class
	} else {
		let mut builder =
			ClassBuilder::new(c"DecodexMainWindowObserver", AnyClass::get(c"NSObject")?)?;
		unsafe {
			builder.add_method(
				sel!(restoreMain:),
				restore_main as unsafe extern "C-unwind" fn(_, _, _),
			);
		}
		builder.register()
	};
	unsafe {
		let observer: Retained<AnyObject> = msg_send![class, new];
		let center: Retained<AnyObject> =
			msg_send![AnyClass::get(c"NSNotificationCenter")?, defaultCenter];
		let name = objc2_foundation::NSString::from_str("NSWindowDidBecomeMainNotification");
		let _: () = msg_send![&*center, addObserver: &*observer, selector: sel!(restoreMain:), name: &*name, object: window];
		Some(observer)
	}
}

/// Retains the installed glass so style updates cannot mistake it for a window backdrop.
pub(crate) struct GlassPanel {
	glass: Retained<NSView>,
	foreground: Retained<NSView>,
	native: Retained<NSWindow>,
	parent: Retained<NSWindow>,
	frame: Option<NSRect>,
	visible: bool,
	clear_style: Option<bool>,
	main_observer: Retained<AnyObject>,
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
		native.setTitle(&objc2_foundation::NSString::from_str(if radius.is_some() {
			"Decodex Composer"
		} else {
			"Decodex Status"
		}));
		let class = AnyClass::get(if radius.is_some() { c"NSGlassEffectView" } else { c"NSView" })?;
		window.set_background_appearance(WindowBackgroundAppearance::Transparent);
		unsafe {
			let glass: Retained<NSView> = msg_send![class, new];
			glass.setFrame(content.bounds());
			let _: () = msg_send![&*glass, setAutoresizingMask: 18usize];
			gpu.removeFromSuperview();
			if let Some(radius) = radius {
				let _: () = msg_send![&*glass, setCornerRadius: radius];
				let _: () = msg_send![&*glass, setContentView: &*gpu];
			} else {
				glass.addSubview(&gpu);
				let _: () = msg_send![&*native, setHasShadow: false];
				let _: () = msg_send![&*native, setAlphaValue: 0.0f64];
			}
			content.addSubview(&glass);
			let _: () = msg_send![&*native, setStyleMask: 0usize];
			let _: () = msg_send![&*native, setLevel: 0isize];
			let _: () = msg_send![&*native, setMovable: false];
			let _: () = msg_send![&*native, setExcludedFromWindowsMenu: true];
			let _: () = msg_send![&*parent, addChildWindow: &*native, ordered: 1isize];
			let main_observer = observe_main_window(&native)?;
			Some(Self {
				glass,
				foreground: gpu,
				native,
				parent,
				frame: None,
				visible: false,
				clear_style: None,
				main_observer,
			})
		}
	}

	pub(crate) fn set_style(&mut self, clear: bool) {
		if self.clear_style == Some(clear) {
			return;
		}
		self.clear_style = Some(clear);
		unsafe {
			let _: () = msg_send![&*self.glass, setStyle: isize::from(clear)];
			// Keep Clear readable against the dark workspace without covering
			// the system material's blur and reflections with an opaque fill.
			let tint: Option<Retained<objc2::runtime::AnyObject>> = clear.then(|| {
				msg_send![AnyClass::get(c"NSColor").expect("AppKit"),
					colorWithSRGBRed: 0.10f64, green: 0.11f64, blue: 0.13f64, alpha: 0.18f64]
			});
			let _: () = msg_send![&*self.glass, setTintColor: tint.as_deref()];
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
			self.native.setFrame_display(frame, true);
			// AppKit does not guarantee autoresizing a reparented Metal view.
			// Explicit sizing also delivers GPUI's setFrameSize resize callback.
			if let Some(content) = self.native.contentView() {
				self.glass.setFrame(content.bounds());
				self.foreground.setFrame(NSRect::new(NSPoint::new(0., 0.), content.bounds().size));
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
			let _: () = msg_send![&*self.native, setAlphaValue: f64::from(opacity)];
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
					.is_some_and(|parent| std::ptr::eq(parent, &*self.parent));
				if self.visible && self.native.isVisible() && attached {
					return;
				}
				// orderOut and parent activation can change native ordering without
				// changing GPUI state. Restore the relationship, not global frontmost.
				if !attached {
					let _: () =
						msg_send![&*self.parent, addChildWindow: &*self.native, ordered: 1isize];
				}
				let _: () = msg_send![&*self.native, orderWindow: 1isize, relativeTo: self.parent.windowNumber()];
			} else if self.visible || self.native.isVisible() {
				let _: () = msg_send![&*self.native, orderOut: std::ptr::null::<NSWindow>()];
			}
		}
		self.visible = visible;
	}
}
impl Drop for GlassPanel {
	fn drop(&mut self) {
		unsafe {
			let center: Retained<AnyObject> = msg_send![
				AnyClass::get(c"NSNotificationCenter").expect("Foundation"),
				defaultCenter
			];
			let _: () = msg_send![&*center, removeObserver: &*self.main_observer];
		}
		self.set_visible(false);
		unsafe {
			let _: () = msg_send![&*self.parent, removeChildWindow: &*self.native];
		}
	}
}
