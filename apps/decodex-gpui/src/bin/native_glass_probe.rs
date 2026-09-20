//! Isolated native compositing experiment. Does not connect to the Decodex daemon.
#![allow(dead_code)]
#[path = "../composer_input.rs"] mod composer_input;
#[path = "../ui_theme.rs"] mod ui_theme;

#[cfg(not(target_os = "macos"))]
fn main() {
	eprintln!("This native compositing probe requires macOS 26 or later.");
}

#[cfg(target_os = "macos")]
fn main() {
	probe::run();
}

#[cfg(target_os = "macos")]
mod probe {
	use crate::composer_input::{self, ComposerInput, SubmitComposer};
	use gpui::{
		App, Bounds, Context, Entity, IntoElement, Render, Window, WindowBackgroundAppearance,
		WindowBounds, WindowHandle, WindowKind, WindowOptions, div, point, prelude::*, px, rgb,
		rgba, size,
	};
	use objc2::{msg_send, rc::Retained, runtime::AnyClass};
	use objc2_app_kit::{NSView, NSWindow};
	use objc2_foundation::{NSPoint, NSRect, NSSize};
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};

	struct Backdrop {
		child: Option<WindowHandle<Composer>>,
		submitted: String,
	}
	struct Composer {
		input: Entity<ComposerInput>,
		parent: Entity<Backdrop>,
		clear: bool,
	}

	pub fn run() {
		if AnyClass::get(c"NSGlassEffectView").is_none() {
			eprintln!("Native glass requires macOS 26 or later.");
			return;
		}
		let app = gpui_platform::application();
		app.on_reopen(|cx| {
			for w in cx.windows() {
				if let Some(w) = w.downcast::<Backdrop>() {
					let _ = w.update(cx, |_, w, _| w.activate_window());
				}
			}
		});
		app.run(|cx: &mut App| {
			cx.set_menus(vec![gpui::Menu {
				name: "Native Glass Probe".into(),
				items: vec![],
				disabled: false,
			}]);
			eprintln!("probe: application ready");
			composer_input::bind_keys(cx);
			let parent = cx
				.open_window(
					WindowOptions {
						window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
							None,
							size(px(940.), px(640.)),
							cx,
						))),
						window_min_size: Some(size(px(440.), px(300.))),
						titlebar: Some(gpui::TitlebarOptions {
							title: Some("Native Glass Probe".into()),
							..Default::default()
						}),
						..Default::default()
					},
					|_, cx| cx.new(|_| Backdrop { child: None, submitted: String::new() }),
				)
				.unwrap();
			if std::env::var_os("DECODEX_PROBE_BASELINE").is_some() {
				cx.activate(true);
				return;
			}
			parent
				.update(cx, |backdrop, parent_window, cx| {
					let native_parent = native_window(parent_window);
					let owner = cx.entity();
					let child = cx
						.open_window(
							WindowOptions {
								kind: WindowKind::Floating,
								titlebar: None,
								window_bounds: Some(WindowBounds::Windowed(Bounds::new(
									point(px(0.), px(0.)),
									size(px(720.), px(64.)),
								))),
								window_background: WindowBackgroundAppearance::Transparent,
								focus: false,
								show: false,
								..Default::default()
							},
							|_, cx| {
								cx.new(|cx| Composer {
									input: cx.new(|cx| {
										ComposerInput::message(
											0,
											"Type here · Enter records locally",
											"Probe message",
											cx,
										)
									}),
									parent: owner,
									clear: false,
								})
							},
						)
						.unwrap();
					child
						.update(cx, |_, window, _| {
							install_glass(window);
							let native_child = native_window(window);
							native_child.setTitle(&objc2_foundation::NSString::from_str(
								"Native Glass Composer",
							));
							unsafe {
								let _: () = msg_send![&*native_child, setStyleMask: 0usize];
								let _: () = msg_send![&*native_child, setLevel: 0isize];
								let _: () = msg_send![&*native_parent, addChildWindow: &*native_child, ordered: 1isize];
							}
							place(&native_parent, &native_child);
							unsafe {
								let _: () = msg_send![&*native_child, orderFront: std::ptr::null::<NSWindow>()];
							}
						})
						.unwrap();
					backdrop.child = Some(child);
					parent_window.on_window_should_close(cx, |_, cx| {
						cx.quit();
						true
					});
					cx.observe_window_bounds(parent_window, move |_, parent_window, cx| {
						let native_parent = native_window(parent_window);
						let _ = child.update(cx, |_, window, _| {
							place(&native_parent, &native_window(window))
						});
					})
					.detach();
				})
				.unwrap();
			parent.update(cx, |_, window, _| window.activate_window()).unwrap();
			cx.refresh_windows();
			eprintln!("probe: native windows ready, count {}", cx.windows().len());
			cx.activate(true);
		});
	}

	fn native_view(window: &Window) -> Retained<NSView> {
		let handle = HasWindowHandle::window_handle(window).unwrap();
		let RawWindowHandle::AppKit(handle) = handle.as_raw() else { unreachable!() };
		unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()).unwrap() }
	}
	fn native_window(window: &Window) -> Retained<NSWindow> {
		native_view(window).window().unwrap()
	}

	fn install_glass(window: &mut Window) {
		window.set_background_appearance(WindowBackgroundAppearance::Transparent);
		let gpu = native_view(window);
		let content = gpu.window().unwrap().contentView().unwrap();
		let class = AnyClass::get(c"NSGlassEffectView").expect("macOS 26 required for this probe");
		unsafe {
			let glass: Retained<NSView> = msg_send![class, new];
			glass.setFrame(content.bounds());
			let _: () = msg_send![&*glass, setAutoresizingMask: 18usize];
			let _: () = msg_send![&*glass, setCornerRadius: 28f64];
			gpu.removeFromSuperview();
			let _: () = msg_send![&*glass, setContentView: &*gpu];
			content.addSubview(&glass);
			let foreground: Retained<NSView> = msg_send![&*glass, contentView];
			assert!(std::ptr::eq(&*foreground, &*gpu), "glass must own the real GPUI foreground");
			eprintln!("probe: native glass owns the GPUI foreground");
		}
	}

	fn place(parent: &NSWindow, child: &NSWindow) {
		let content = parent.contentView().unwrap();
		let bounds = content.bounds();
		let width = (bounds.size.width - 80.).clamp(320., 760.);
		let local = NSRect::new(
			NSPoint::new((bounds.size.width - width) / 2., 32.),
			NSSize::new(width, 64.),
		);
		let frame = parent.convertRectToScreen(local);
		child.setFrame_display(frame, true);
	}

	impl Render for Backdrop {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			div()
				.size_full()
				.relative()
				.bg(rgb(0x141822))
				.text_color(rgb(0xffffff))
				.child(div().absolute().inset_0().id("probe-scroll").overflow_y_scroll().children(
					(0..40).map(|i| {
						div()
							.h(px(78.))
							.px_8()
							.py_4()
							.bg(rgb(if i % 2 == 0 { 0x384d75 } else { 0x734a49 }))
							.child(format!("Backdrop row {i} — scroll behind the native composer"))
					}),
				))
				.child(
					div()
						.absolute()
						.top_4()
						.right_4()
						.p_3()
						.rounded_lg()
						.bg(rgba(0x101014dd))
						.child(format!("Local submission: {}", self.submitted)),
				)
		}
	}
	impl Render for Composer {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			div()
				.size_full()
				.rounded(px(28.))
				.px_4()
				.flex()
				.items_center()
				.gap_3()
				.text_color(rgb(0xffffff))
				.on_action(cx.listener(|s, _: &SubmitComposer, _, cx| {
					let text = s.input.read(cx).content().to_owned();
					s.parent.update(cx, |parent, cx| {
						parent.submitted = text;
						cx.notify();
					});
					s.input.update(cx, |input, cx| input.clear(cx));
				}))
				.child(div().flex_1().min_w_0().child(self.input.clone()))
				.child(
					div()
						.id("glass-style")
						.px_3()
						.py_2()
						.rounded_lg()
						.cursor_pointer()
						.hover(|s| s.bg(rgba(0xffffff18)))
						.on_click(cx.listener(|s, _, window, cx| {
							s.clear = !s.clear;
							let glass = native_window(window)
								.contentView()
								.unwrap()
								.subviews()
								.iter()
								.find(|v| unsafe {
									msg_send![&**v, isKindOfClass: AnyClass::get(c"NSGlassEffectView").unwrap()]
								})
								.unwrap();
							unsafe {
								let _: () = msg_send![&*glass, setStyle: isize::from(s.clear)];
							}
							cx.notify();
						}))
						.child(if self.clear { "Clear" } else { "Regular" }),
				)
		}
	}
}
