//! Native material views and foreground-owning GPUI child windows.
//! GPUI owns UI state and navigation; AppKit composes surfaces and relays side buttons.
use std::{cell::Cell, env, ptr, time::Instant};

use block2::RcBlock;
use gpui::{AnyWindowHandle, Bounds, Pixels, Window, WindowBackgroundAppearance};
use objc2::{
	self,
	rc::{Allocated, Retained},
	runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
};
use objc2_app_kit::{
	NSEventMask, NSEventType, NSResponder, NSView, NSWindow, NSWindowCollectionBehavior,
};
use objc2_core_foundation::CGAffineTransform;
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::Value;

use crate::ui_theme::{
	self,
	material::{GlassScope, GlassStyle, Material, Surface},
};

/// Reduced transparency keeps selection menus readable with the flat material.
fn panel_material(surface: Surface) -> Material {
	let requested = surface.material();
	if surface == Surface::Popover && material_class(requested).is_none() {
		Material::Thin
	} else {
		requested
	}
}

thread_local! {
	// Non-owning handles; each MaterialPanel registers and removes its entry.
	static ATTACHED_WINDOWS: std::cell::RefCell<std::collections::HashMap<usize, Attachment>> = Default::default();
}

/// Owns composition separately from the editor and its window lifecycle.
pub(crate) struct MaterialPanel {
	surface: Surface,
	material: Material,
	radius: f64,
	host: Retained<NSView>,
	material_view: Retained<NSView>,
	foreground: Retained<NSView>,
	native: Retained<NSWindow>,
	parent: Retained<NSWindow>,
	frame: Option<NSRect>,
	visible: bool,
	presented: bool,
	shadow_dirty: bool,
	opacity: Cell<f32>,
	window_observer: Retained<AnyObject>,
	side_buttons: Retained<AnyObject>,
}
impl MaterialPanel {
	pub(crate) fn install_input_bar(
		parent: &Window,
		window: &mut Window,
		radius: f64,
	) -> Option<Self> {
		Self::install_surface(parent, window, Surface::InputBar, radius)
	}

	pub(crate) fn install_popover(parent: &Window, window: &mut Window) -> Option<Self> {
		Self::install_surface(parent, window, Surface::Popover, f64::from(ui_theme::POPOVER_RADIUS))
	}

	pub(crate) fn set_radius(&mut self, radius: f64) {
		if self.radius == radius {
			return;
		}
		self.radius = radius;
		unsafe {
			let _: () = objc2::msg_send![&*self.material_view, setCornerRadius: radius];
		}
		self.shadow_dirty = true;
	}

	fn install_surface(
		parent: &Window,
		window: &mut Window,
		surface: Surface,
		radius: f64,
	) -> Option<Self> {
		let material = panel_material(surface);
		let parent = native(parent)?;
		let native = native(window)?;
		let gpu = view(window)?;
		let content = native.contentView()?;

		native.setCollectionBehavior(
			NSWindowCollectionBehavior::Transient
				| NSWindowCollectionBehavior::IgnoresCycle
				| NSWindowCollectionBehavior::FullScreenAuxiliary,
		);
		native.setTitle(&NSString::from_str(if surface == Surface::InputBar {
			"Decodex Composer"
		} else {
			"Decodex Popover"
		}));
		window.set_background_appearance(WindowBackgroundAppearance::Transparent);

		unsafe {
			let material_view = create_surface(material, radius)?;
			// Transfer material ownership. A window backdrop under a Clear panel
			// would blur or tint the same pixels twice.
			if surface == Surface::Popover {
				ui_theme::window_material::remove_backdrop(window);
			}

			material_view.setFrame(content.bounds());

			let _: () = objc2::msg_send![&*material_view, setAutoresizingMask: 18_usize];

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
			let host: Retained<NSView> = objc2::msg_send![AnyClass::get(c"NSView")?, new];

			host.setFrame(content.bounds());

			let _: () = objc2::msg_send![&*host, setAutoresizingMask: 18_usize];

			host.addSubview(&gpu);

			// The material owns a plain host, while GPUI owns Metal sizing.
			let _: () = objc2::msg_send![&*material_view, setContentView: &*host];

			content.addSubview(&material_view);

			let _: () = objc2::msg_send![&*native, setStyleMask: 0_usize];
			if surface == Surface::Popover {
				let _: () = objc2::msg_send![&*native, setHasShadow: true];
			}
			let _: () = objc2::msg_send![&*native, setLevel: 0_isize];
			let _: () = objc2::msg_send![&*native, setMovable: false];
			let _: () = objc2::msg_send![&*native, setExcludedFromWindowsMenu: true];
			// Attachment precedes the owner's first placement. Start transparent so
			// native ordering cannot expose a frame before bounds and arrival are ready.
			let _: () = objc2::msg_send![&*native, setAlphaValue: 0.0_f64];
			let _: () = objc2::msg_send![&*parent, addChildWindow: &*native, ordered: 1_isize];
			let window_observer = observe_panel_window(&native)?;
			let side_buttons = route_side_buttons(&native, &gpu)?;

			ATTACHED_WINDOWS.with(|windows| {
				windows.borrow_mut().insert(
					&*native as *const NSWindow as usize,
					Attachment { handle: Window::window_handle(window), surface },
				);
			});

			if material == Material::LiquidGlass {
				apply_glass_style(&material_view, surface, native.isKeyWindow());
			}
			apply_popover_tint(&material_view, surface, material);

			Some(Self {
				surface,
				material,
				radius,
				host,
				material_view,
				foreground: gpu,
				native,
				parent,
				frame: None,
				visible: false,
				presented: false,
				shadow_dirty: true,
				opacity: Cell::new(0.),
				window_observer,
				side_buttons,
			})
		}
	}

	/// Replace only the material host; the editor view and its focus stay intact.
	pub(crate) fn refresh_material(&mut self) {
		let surface = self.surface;
		let material = panel_material(surface);

		if self.material == material {
			if material == Material::LiquidGlass {
				self.shadow_dirty |=
					apply_glass_style(&self.material_view, surface, self.native.isKeyWindow());
			}
			apply_popover_tint(&self.material_view, surface, material);
			return;
		}

		let Some(replacement) = create_surface(material, self.radius) else { return };
		let Some(content) = self.native.contentView() else { return };

		replacement.setFrame(content.bounds());
		replacement.setAlphaValue(self.material_view.alphaValue());

		let had_focus = self.native.firstResponder().is_some_and(|first| {
			ptr::eq(
				(&*first as *const NSResponder).cast::<AnyObject>(),
				(&*self.foreground as *const NSView).cast::<AnyObject>(),
			)
		});

		unsafe {
			let _: () = objc2::msg_send![&*self.material_view, setContentView: None::<&NSView>];
			let _: () = objc2::msg_send![&*replacement, setContentView: &*self.host];
		}

		content.addSubview(&replacement);
		self.material_view.removeFromSuperview();

		self.material_view = replacement;
		self.material = material;
		self.shadow_dirty = true;

		if had_focus {
			self.focus_text();
		}
		if material == Material::LiquidGlass {
			apply_glass_style(&self.material_view, surface, self.native.isKeyWindow());
		}
		apply_popover_tint(&self.material_view, surface, material);
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
			let started = Instant::now();
			// AppKit moves attached children with their parent. Do not submit the
			// same screen position again or resize Metal during a parent drag.
			let place = self.native.frame() != frame;
			let resize = self.frame.is_none_or(|previous| previous.size != frame.size);

			if place {
				self.native.setFrame_display(frame, false);
			}
			if resize && let Some(content) = self.native.contentView() {
				self.shadow_dirty = true;
				self.material_view.setFrame(content.bounds());
				// Call the resize hook, not setFrame:, so the drawable and GPUI
				// viewport use the same size. The foreground has one layout owner.
				self.foreground.setFrameSize(content.bounds().size);
				self.foreground.setFrameOrigin(NSPoint::new(0., 0.));
			}

			self.frame = Some(frame);
			crate::frame_trace::native_panel_place(started, resize);

			return place || resize;
		}

		false
	}

	pub(crate) fn focus_text(&self) {
		self.native.makeFirstResponder(Some(&self.foreground));
	}

	pub(crate) fn set_title(&self, title: &str) {
		self.native.setTitle(&NSString::from_str(title));
	}

	pub(crate) fn set_opacity(&self, opacity: f32) {
		// Only a changed alpha needs an AppKit write. Transcript frames must not
		// repeatedly update the native window's composition state.
		if self.opacity.replace(opacity) == opacity {
			return;
		}

		unsafe {
			let _: () = objc2::msg_send![&*self.native, setAlphaValue: f64::from(opacity)];
		}
	}

	/// Order the native window before asking GPUI for a frame. AppKit stops the
	/// display link of hidden windows; only the content stays transparent here.
	pub(crate) fn prepare_open(&mut self) {
		if !self.visible || !self.native.isVisible() {
			unsafe {
				let layer: Option<Retained<AnyObject>> =
					objc2::msg_send![&*self.material_view, layer];
				if let Some(layer) = layer {
					for key in ["opacity", "transform.translation.y"] {
						let _: () = objc2::msg_send![&*layer, removeAnimationForKey: &*NSString::from_str(key)];
					}
				}
			}
			self.material_view.setAlphaValue(0.);
			self.material_view.setFrameOrigin(NSPoint::new(0., 0.));
			self.presented = false;
			self.set_opacity(1.);
		}
		self.set_visible(true);
	}

	/// Reveal the prepared content as one surface, without resizing its texture.
	pub(crate) fn set_open(&mut self, open: bool) {
		if !open {
			self.set_visible(false);
			self.presented = false;
			return;
		}
		self.prepare_open();
		if !self.presented {
			self.presented = true;
			self.material_view.setAlphaValue(1.);
			self.refresh_shadow();
			if crate::ui_frame::reduced() {
				return;
			}
			unsafe {
				let layer: Option<Retained<AnyObject>> =
					objc2::msg_send![&*self.material_view, layer];
				let Some(layer) = layer else { return };
				let timing: Retained<AnyObject> = objc2::msg_send![AnyClass::get(c"CAMediaTimingFunction").expect("QuartzCore"), functionWithName: &*NSString::from_str("easeOut")];
				// Keep arrival in the composite layer. Layout and Metal redraws
				// use the final geometry throughout the presentation animation.
				for (key, from, to) in [
					("opacity", 0., 1.),
					("transform.translation.y", -f64::from(ui_theme::POPOVER_OFFSET), 0.),
				] {
					let key = NSString::from_str(key);
					let animation: Retained<AnyObject> = objc2::msg_send![AnyClass::get(c"CABasicAnimation").expect("QuartzCore"), animationWithKeyPath: &*key];
					let number = AnyClass::get(c"NSNumber").expect("Foundation");
					let from: Retained<AnyObject> =
						objc2::msg_send![number, numberWithDouble: from];
					let to: Retained<AnyObject> = objc2::msg_send![number, numberWithDouble: to];
					let _: () = objc2::msg_send![&*animation, setFromValue: &*from];
					let _: () = objc2::msg_send![&*animation, setToValue: &*to];
					let _: () = objc2::msg_send![&*animation, setDuration: ui_theme::POPOVER_ARRIVAL.as_secs_f64()];
					let _: () = objc2::msg_send![&*animation, setTimingFunction: &*timing];
					let _: () = objc2::msg_send![&*layer, addAnimation: &*animation, forKey: &*key];
				}
			}
		}
		self.refresh_shadow();
	}

	fn refresh_shadow(&mut self) {
		if self.surface == Surface::Popover && self.shadow_dirty {
			// Reused popup windows change their rounded contour after Metal resizes.
			// Rebuild the system shadow once the final foreground has been painted.
			self.native.invalidateShadow();
			self.shadow_dirty = false;
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

impl Drop for MaterialPanel {
	fn drop(&mut self) {
		ATTACHED_WINDOWS.with(|windows| {
			windows.borrow_mut().remove(&(&*self.native as *const NSWindow as usize));
		});

		unsafe {
			let _: () = objc2::msg_send![
				AnyClass::get(c"NSEvent").expect("AppKit"), removeMonitor: &*self.side_buttons
			];
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

fn route_side_buttons(
	window: &NSWindow,
	foreground: &Retained<NSView>,
) -> Option<Retained<AnyObject>> {
	let number = window.windowNumber();
	let foreground = foreground.clone();
	let handler = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
		unsafe {
			let target: isize = objc2::msg_send![event, windowNumber];
			let button: isize = objc2::msg_send![event, buttonNumber];
			if target != number || ![3, 4].contains(&button) {
				return event;
			}
			let kind: NSEventType = objc2::msg_send![event, type];
			// Deliver to GPUI before the material's native responder chain. GPUI
			// keeps the navigation policy; consuming here prevents a second delivery.
			if kind == NSEventType::OtherMouseDown {
				let _: () = objc2::msg_send![&*foreground, otherMouseDown: event];
			} else {
				let _: () = objc2::msg_send![&*foreground, otherMouseUp: event];
			}
			ptr::null_mut()
		}
	});
	unsafe {
		Some(objc2::msg_send![
			AnyClass::get(c"NSEvent")?,
			addLocalMonitorForEventsMatchingMask: (NSEventMask::OtherMouseDown | NSEventMask::OtherMouseUp),
			handler: &*handler
		])
	}
}

struct Attachment {
	handle: AnyWindowHandle,
	surface: Surface,
}

pub(crate) fn material_class(material: Material) -> Option<&'static AnyClass> {
	match material {
		Material::Thin => AnyClass::get(c"DecodexThinMaterialView"),
		Material::LiquidGlass
			if env::var_os("DECODEX_DISABLE_LIQUID_GLASS").is_none() && !reduced_transparency() =>
			AnyClass::get(c"NSGlassEffectView"),
		Material::LiquidGlass => None,
	}
}

pub(crate) fn create_surface(material: Material, radius: f64) -> Option<Retained<NSView>> {
	let class = material_class(material)?;

	unsafe {
		let allocated: Allocated<NSView> = objc2::msg_send![class, alloc];
		let surface: Retained<NSView> = objc2::msg_send![allocated, initWithFrame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.))];
		let _: () = objc2::msg_send![&*surface, setCornerRadius: radius];
		let _: () = objc2::msg_send![&*surface, setAutoresizingMask: 18_usize];

		Some(surface)
	}
}

pub(crate) fn observed_material(window: &Window) -> Option<&'static str> {
	let content = native(window)?.contentView()?;

	for view in content.subviews().iter() {
		for (name, label) in
			[(c"DecodexThinMaterialView", "thin"), (c"NSGlassEffectView", "liquid-glass")]
		{
			if let Some(class) = AnyClass::get(name) {
				let matches: bool = unsafe { objc2::msg_send![&*view, isKindOfClass: class] };

				if matches {
					return Some(label);
				}
			}
		}
	}

	None
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

/// Read the applied tint for opt-in material qualification.
pub(crate) fn observed_tint(window: &Window) -> Option<bool> {
	let content = native(window)?.contentView()?;
	let class = AnyClass::get(c"NSGlassEffectView")?;
	content.subviews().iter().find_map(|view| unsafe {
		let glass: bool = objc2::msg_send![&*view, isKindOfClass: class];
		glass.then(|| {
			let tint: Option<Retained<AnyObject>> = objc2::msg_send![&*view, tintColor];
			tint.is_some()
		})
	})
}

/// Inspect this window's render backing only while the user enables frame diagnostics.
pub(crate) fn observed_rendering(window: &Window) -> Option<Value> {
	let gpu = view(window)?;
	let native = gpu.window()?;
	let viewport = window.viewport_size();
	let bounds = gpu.bounds();
	let backing = gpu.convertRectToBacking(bounds);

	unsafe {
		let layer: Option<Retained<AnyObject>> = objc2::msg_send![&*gpu, layer];
		let layer = layer?;
		let contents_scale: f64 = objc2::msg_send![&*layer, contentsScale];
		let rasterization_scale: f64 = objc2::msg_send![&*layer, rasterizationScale];
		let rasterized: bool = objc2::msg_send![&*layer, shouldRasterize];
		let mut ancestry = Vec::new();
		let mut ancestor = Some(gpu.clone());

		while let Some(view) = ancestor {
			let view_layer: Option<Retained<AnyObject>> = objc2::msg_send![&*view, layer];
			let composition = view_layer.map(|layer| {
				let frame: NSRect = objc2::msg_send![&*layer, frame];
				let bounds: NSRect = objc2::msg_send![&*layer, bounds];
				let affine: CGAffineTransform = objc2::msg_send![&*layer, affineTransform];
				let opacity: f32 = objc2::msg_send![&*layer, opacity];
				let rasterized: bool = objc2::msg_send![&*layer, shouldRasterize];
				let filters: Option<Retained<NSArray<AnyObject>>> = objc2::msg_send![&*layer, filters];

				serde_json::json!({
					"class": layer.class().name().to_string_lossy(),
					"frame": [frame.origin.x, frame.origin.y, frame.size.width, frame.size.height],
					"bounds": [bounds.origin.x, bounds.origin.y, bounds.size.width, bounds.size.height],
					"affine": [affine.a, affine.b, affine.c, affine.d, affine.tx, affine.ty],
					"opacity": opacity,
					"rasterized": rasterized,
					"filters": filters.map(|filters| filters.iter().map(|filter| filter.class().name().to_string_lossy().into_owned()).collect::<Vec<_>>()).unwrap_or_default(),
				})
			});
			let frame = view.frame();
			let bounds = view.bounds();

			ancestry.push(serde_json::json!({
				"class": view.class().name().to_string_lossy(),
				"frame": [frame.origin.x, frame.origin.y, frame.size.width, frame.size.height],
				"bounds": [bounds.origin.x, bounds.origin.y, bounds.size.width, bounds.size.height],
				"alpha": view.alphaValue(),
				"layer": composition,
			}));

			ancestor = view.superview();
		}

		let siblings = gpu
			.superview()
			.map(|parent| {
				parent
					.subviews()
					.iter()
					.map(|view| view.class().name().to_string_lossy().into_owned())
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();
		let drawable = AnyClass::get(c"CAMetalLayer").and_then(|class| {
			let metal: bool = objc2::msg_send![&*layer, isKindOfClass: class];

			metal.then(|| {
				let size: NSSize = objc2::msg_send![&*layer, drawableSize];

				[size.width, size.height]
			})
		});

		Some(serde_json::json!({
			"gpui_scale": window.scale_factor(),
			"gpui_viewport": [f32::from(viewport.width), f32::from(viewport.height)],
			"window_backing_scale": native.backingScaleFactor(),
			"view_bounds": [bounds.size.width, bounds.size.height],
			"view_backing_size": [backing.size.width, backing.size.height],
			"layer_contents_scale": contents_scale,
			"layer_rasterization_scale": rasterization_scale,
			"layer_rasterized": rasterized,
			"metal_drawable_size": drawable,
			"foreground_ancestry": ancestry,
			"sibling_paint_order": siblings,
		}))
	}
}

/// Attached panels own their material independently of window backdrops.
pub(crate) fn owns_material(window: &Window) -> bool {
	ATTACHED_WINDOWS.with(|windows| {
		windows.borrow().values().any(|attachment| attachment.handle == window.window_handle())
	})
}

pub(crate) fn available(surface: Surface) -> bool {
	material_class(surface.material()).is_some()
}

pub(crate) fn apply_glass_style(glass: &NSView, surface: Surface, key: bool) -> bool {
	let clear = match surface {
		Surface::Window => GlassScope::Window.configured() == GlassStyle::Clear,
		Surface::InputBar => key,
		Surface::Popover => GlassScope::Popover.configured() == GlassStyle::Clear,
	};

	set_glass_style(glass, clear, surface == Surface::InputBar)
}

fn set_glass_style(glass: &NSView, clear: bool, input_bar: bool) -> bool {
	unsafe {
		let style: isize = objc2::msg_send![glass, style];

		if style == isize::from(clear) {
			return false;
		}

		let _: () = objc2::msg_send![glass, setStyle: isize::from(clear)];
		let tint: Option<Retained<AnyObject>> = (clear && input_bar).then(|| {
			objc2::msg_send![AnyClass::get(c"NSColor").expect("AppKit"), colorWithSRGBRed: 0.10_f64, green: 0.11_f64, blue: 0.13_f64, alpha: 0.18_f64]
		});
		let _: () = objc2::msg_send![glass, setTintColor: tint.as_deref()];
		true
	}
}

fn apply_popover_tint(view: &NSView, surface: Surface, material: Material) {
	if surface != Surface::Popover {
		return;
	}
	let color = ui_theme::popover_tint(material, GlassScope::Popover.configured());
	unsafe {
		let tint: Option<Retained<AnyObject>> = color.map(|color| objc2::msg_send![AnyClass::get(c"NSColor").expect("AppKit"), colorWithSRGBRed: f64::from(color.r), green: f64::from(color.g), blue: f64::from(color.b), alpha: f64::from(color.a)]);
		let previous: Option<Retained<AnyObject>> = objc2::msg_send![view, tintColor];
		let changed = match (previous.as_deref(), tint.as_deref()) {
			(Some(previous), Some(tint)) => {
				let equal: bool = objc2::msg_send![previous, isEqual: tint];
				!equal
			},
			(None, None) => false,
			_ => true,
		};
		if changed {
			let _: () = objc2::msg_send![view, setTintColor: tint.as_deref()];
		}
	}
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
	let surface = ATTACHED_WINDOWS.with(|windows| {
		windows
			.borrow()
			.get(&(window as *const NSWindow as usize))
			.map(|attachment| attachment.surface)
	});
	let Some(surface) = surface else { return };
	let Some(content) = window.contentView() else { return };
	let Some(class) = AnyClass::get(c"NSGlassEffectView") else { return };
	let Some(glass) = content
		.subviews()
		.iter()
		.find(|view| unsafe { objc2::msg_send![&**view, isKindOfClass: class] })
	else {
		return;
	};

	apply_glass_style(&glass, surface, window.isKeyWindow());
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
