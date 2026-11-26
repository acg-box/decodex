//! AppKit context menus with GPUI actions applied after native tracking.
use std::ffi::c_void;

use gpui::{App, AsyncApp, Window};
use objc2::{
	MainThreadMarker,
	rc::Retained,
	runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
};
use objc2_app_kit::{NSEventModifierFlags, NSMenu, NSMenuItem, NSView};
use objc2_foundation::{NSPoint, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(crate) enum Item<T> {
	Command {
		title: String,
		key: String,
		modifiers: NSEventModifierFlags,
		enabled: bool,
		action: T,
	},
	Separator,
}
impl<T> Item<T> {
	pub(crate) fn command(
		title: impl Into<String>,
		key: &'static str,
		enabled: bool,
		action: T,
	) -> Self {
		Self::Command {
			title: title.into(),
			key: key.into(),
			modifiers: NSEventModifierFlags::Command,
			enabled,
			action,
		}
	}

	pub(crate) fn bound(
		title: &'static str,
		binding: &dyn gpui::Action,
		cx: &App,
		enabled: bool,
		action: T,
	) -> Self {
		let mut modifiers = NSEventModifierFlags::empty();
		let key = crate::keyboard_shortcuts::key(binding, cx).map_or_else(String::new, |stroke| {
			let m = stroke.modifiers;
			for (on, flag) in [
				(m.platform, NSEventModifierFlags::Command),
				(m.control, NSEventModifierFlags::Control),
				(m.alt, NSEventModifierFlags::Option),
				(m.shift, NSEventModifierFlags::Shift),
				(m.function, NSEventModifierFlags::Function),
			] {
				if on {
					modifiers |= flag;
				}
			}
			match stroke.key.as_str() {
				"enter" => "\r".into(),
				"tab" => "\t".into(),
				"space" => " ".into(),
				"escape" => "\u{1b}".into(),
				"backspace" => "\u{8}".into(),
				"delete" => "\u{f728}".into(),
				"up" => "\u{f700}".into(),
				"down" => "\u{f701}".into(),
				"left" => "\u{f702}".into(),
				"right" => "\u{f703}".into(),
				k if k.len() == 1 => k.into(),
				k => k
					.strip_prefix('f')
					.and_then(|n| n.parse::<u32>().ok())
					.filter(|n| (1..=35).contains(n))
					.and_then(|n| char::from_u32(0xf703 + n))
					.map(|c| c.to_string())
					.unwrap_or_default(),
			}
		});
		Self::Command { title: title.into(), key, modifiers, enabled, action }
	}
}

struct Request<T, F> {
	view: Retained<NSView>,
	position: NSPoint,
	items: Vec<Item<T>>,
	apply: F,
	app: AsyncApp,
}

pub(crate) fn show<T: 'static, F: FnOnce(T, &mut App) + 'static>(
	window: &Window,
	cx: &App,
	items: Vec<Item<T>>,
	apply: F,
) {
	let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
	let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
	// The retained GPUI view and menu snapshot live until native tracking ends.
	let Some(view) = (unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }) else {
		return;
	};
	let mouse = window.mouse_position();
	let position = NSPoint::new(
		f64::from(f32::from(mouse.x)),
		if view.isFlipped() {
			f64::from(f32::from(mouse.y))
		} else {
			view.bounds().size.height - f64::from(f32::from(mouse.y))
		},
	);
	let request = Request { view, position, items, apply, app: cx.to_async() };

	// AppKit tracks a menu in a nested event loop. Do not enter it inside a GPUI callback.
	unsafe {
		dispatch_async_f(
			std::ptr::addr_of!(_dispatch_main_q),
			Box::into_raw(Box::new(request)).cast(),
			present::<T, F>,
		);
	}
}

extern "C" fn present<T, F: FnOnce(T, &mut App)>(context: *mut c_void) {
	// The main queue exclusively owns this snapshot and all AppKit calls stay on main.
	let Request { view, position, items, apply, app } =
		*unsafe { Box::from_raw(context.cast::<Request<T, F>>()) };

	if view.window().is_none() {
		return;
	}

	let chosen = menu(&view, position, &items);
	if chosen > 0
		&& let Some(Item::Command { action, enabled: true, .. }) =
			items.into_iter().nth(chosen as usize - 1)
	{
		app.update(|cx| apply(action, cx));
	}
}

fn menu<T>(view: &NSView, position: NSPoint, items: &[Item<T>]) -> isize {
	let mtm = MainThreadMarker::new().expect("native menu main queue");
	let menu = NSMenu::new(mtm);
	// The target is retained for the duration of synchronous menu tracking.
	let target: Retained<AnyObject> = unsafe { objc2::msg_send![target_class(), new] };
	menu.setAutoenablesItems(false);

	for (index, item) in items.iter().enumerate() {
		let Item::Command { title, key, modifiers, enabled, .. } = item else {
			menu.addItem(&NSMenuItem::separatorItem(mtm));
			continue;
		};
		let item = NSMenuItem::new(mtm);
		item.setTitle(&NSString::from_str(title));
		item.setKeyEquivalent(&NSString::from_str(key));
		item.setKeyEquivalentModifierMask(*modifiers);
		item.setEnabled(*enabled);
		item.setTag(index as isize + 1);
		// The selector belongs to the live, retained target above.
		unsafe {
			item.setAction(Some(objc2::sel!(choose:)));
			item.setTarget(Some(&target));
		}
		menu.addItem(&item);
	}

	menu.popUpMenuPositioningItem_atLocation_inView(None, position, Some(view));
	unsafe { objc2::msg_send![&*target, tag] }
}

fn target_class() -> &'static AnyClass {
	if let Some(class) = AnyClass::get(c"DecodexContextMenuTarget") {
		return class;
	}

	let mut class = ClassBuilder::new(
		c"DecodexContextMenuTarget",
		AnyClass::get(c"NSMenuItem").expect("AppKit menu item"),
	)
	.expect("unique menu target");

	// A menu-local tag records the choice. App state is updated after tracking ends.
	unsafe {
		class.add_method(objc2::sel!(choose:), choose as extern "C-unwind" fn(_, _, _));
	}

	class.register()
}

extern "C-unwind" fn choose(target: &AnyObject, _: Sel, sender: &AnyObject) {
	// Both arguments are the retained NSMenuItems created by menu().
	unsafe {
		let tag: isize = objc2::msg_send![sender, tag];
		let _: () = objc2::msg_send![target, setTag: tag];
	}
}

unsafe extern "C" {
	static _dispatch_main_q: c_void;
	fn dispatch_async_f(
		queue: *const c_void,
		context: *mut c_void,
		work: extern "C" fn(*mut c_void),
	);
}
