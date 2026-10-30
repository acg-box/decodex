//! Qualify the production popover lifecycle without input automation or a daemon.
use std::{
	cell::RefCell,
	rc::Rc,
	time::{Duration, Instant},
};

use gpui::{
	AppContext as _, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions,
	div, point,
	prelude::{ParentElement as _, Styled as _},
	px, size,
};

use crate::{
	probe,
	ui_theme::{
		material::{GlassScope, GlassStyle, Material, Surface},
		native_material_panel::MaterialPanel,
	},
};

#[derive(Default)]
struct Paint {
	opened: Option<Instant>,
	first_ms: Option<f64>,
}

struct Menu {
	panel: Option<MaterialPanel>,
	paint: Rc<RefCell<Paint>>,
	open: bool,
}

impl Render for Menu {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		if self.open {
			cx.defer_in(window, |menu, _, _| {
				menu.panel.as_mut().expect("production panel").set_open(menu.open);
			});
		}
		let paint = self.paint.clone();
		div().size_full().child(
			gpui::canvas(
				|_, _, _| (),
				move |_, _, _, _| {
					let mut paint = paint.borrow_mut();
					if let Some(opened) = paint.opened
						&& paint.first_ms.is_none()
					{
						paint.first_ms = Some(opened.elapsed().as_secs_f64() * 1_000.);
					}
				},
			)
			.size_full(),
		)
	}
}

pub fn run() {
	let library =
		std::env::var("DECODEX_PROBE_NATIVE_LIBRARY").expect("explicit native fixture library");
	let library = std::ffi::CString::new(library).expect("native fixture path");
	assert!(
		!unsafe { libc::dlopen(library.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) }.is_null()
	);
	gpui_platform::application().run(|cx| {
		let paint = Rc::new(RefCell::new(Paint::default()));
		let parent = cx.open_window(WindowOptions {
			window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(700.), px(500.)), cx))),
			..Default::default()
		}, |_, cx| cx.new(|_| Menu { panel: None, paint: Rc::new(RefCell::new(Paint::default())), open: false }))
		.expect("open the isolated parent");
		let child = cx.open_window(WindowOptions {
			titlebar: None,
			show: false,
			focus: false,
			..crate::ui_frame::window_options()
		}, |_, cx| cx.new(|_| Menu { panel: None, paint: paint.clone(), open: false }))
		.expect("open the isolated popover");

		parent.update(cx, |_, parent, cx| {
			child.update(cx, |menu, window, _| {
				menu.panel = MaterialPanel::install_popover(parent, window);
				assert!(menu.panel.is_some());
			}).expect("install the production material panel");
		}).expect("live parent");
		cx.activate(true);
		cx.spawn(async move |cx| {
			let mut samples = Vec::new();
			for (material, style) in [(Material::Thin, GlassStyle::Regular), (Material::LiquidGlass, GlassStyle::Regular), (Material::LiquidGlass, GlassStyle::Clear)] {
				cx.update(|cx| {
					Surface::Popover.select(material, cx);
					GlassScope::Popover.select(style, cx);
				});
				for (index, (width, height)) in [(280., 328.), (248., 230.), (192., 186.)].into_iter().enumerate() {
				cx.background_executor().timer(if index == 0 { Duration::from_secs(2) } else { Duration::from_millis(500) }).await;
				child.update(cx, |menu, window, cx| {
					*menu.paint.borrow_mut() = Paint { opened: Some(Instant::now()), first_ms: None };
					menu.open = true;
					let panel = menu.panel.as_mut().expect("production panel");
					panel.refresh_material();
					if panel.place(Bounds::new(point(px(100.), px(100.)), size(px(width), px(height)))) {
						window.bounds_changed(cx);
					}
					panel.prepare_open();
					assert_eq!(probe::native_window(window).alphaValue(), 1.);
					assert!(probe::native_window(window).isVisible());
					cx.notify();
				}).expect("show the production panel");
				cx.background_executor().timer(Duration::from_millis(160)).await;
				let first_ms = paint.borrow().first_ms.expect("popover paints without pointer or network events");
				samples.push(serde_json::json!({"material":material.label(),"style":format!("{style:?}"),"width":width,"height":height,"first_cpu_paint_ms":first_ms}));
				child.update(cx, |menu, window, _| {
					menu.open = false;
					menu.panel.as_mut().expect("production panel").set_open(false);
					assert!(!probe::native_window(window).isVisible());
				}).expect("hide the production panel");
				}
			}
			eprintln!("popover-first-frame: {}", serde_json::json!({"scope":"Isolated native production MaterialPanel; first CPU paint, excludes GPU and scanout; no input automation, service, or network requests", "samples":samples}));
			cx.update(|cx| cx.quit());
		}).detach();
	});
}
