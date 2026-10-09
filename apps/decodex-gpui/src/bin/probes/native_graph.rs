//! Isolated GPUI/native-glass composition and frame-cadence probe; synthetic nodes only.
use super::probe::{native_view, native_window};
use gpui::{
	App, AppContext, Bounds, Context, FocusHandle, IntoElement, Render, Window,
	WindowBackgroundAppearance, WindowBounds, WindowOptions, div, point, prelude::*, px, rgb, rgba,
	size,
};
use objc2::{msg_send, rc::Retained, runtime::AnyClass};
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::time::Instant;

struct Material {
	backdrop: Retained<NSView>,
	container: Retained<NSView>,
	host: Retained<NSView>,
	cards: Vec<Retained<NSView>>,
}
impl Material {
	fn install(window: &mut Window) -> Self {
		let gpu = native_view(window);
		let root = native_window(window).contentView().expect("content view");
		window.set_background_appearance(WindowBackgroundAppearance::Transparent);
		unsafe {
			let container: Retained<NSView> = msg_send![
				AnyClass::get(c"NSGlassEffectContainerView").expect("macOS 26 glass"),
				new
			];
			let host: Retained<NSView> = msg_send![AnyClass::get(c"NSView").expect("NSView"), new];
			let backdrop: Retained<NSView> =
				msg_send![AnyClass::get(c"NSView").expect("NSView"), new];
			backdrop.setWantsLayer(true);
			let color: Retained<objc2::runtime::AnyObject> = msg_send![AnyClass::get(c"NSColor").expect("NSColor"), colorWithSRGBRed: 0.16_f64, green: 0.17_f64, blue: 0.20_f64, alpha: 1.0_f64];
			let cg: *const std::ffi::c_void = msg_send![&*color, CGColor];
			let layer: Retained<objc2::runtime::AnyObject> = msg_send![&*backdrop, layer];
			let _: () = msg_send![&*layer, setBackgroundColor: cg];
			let _: () =
				msg_send![&*root, addSubview: &*backdrop, positioned: -1_isize, relativeTo: &*gpu];
			container.setWantsLayer(true);
			let clip: Retained<objc2::runtime::AnyObject> = msg_send![&*container, layer];
			let _: () = msg_send![&*clip, setMasksToBounds: true];
			let _: () = msg_send![&*container, setContentView: &*host];
			let _: () = msg_send![&*container, setSpacing: 0_f64];
			let _: () =
				msg_send![&*root, addSubview: &*container, positioned: -1_isize, relativeTo: &*gpu];
			Self { backdrop, container, host, cards: vec![] }
		}
	}

	fn sync(&mut self, window: &Window, count: usize, enabled: bool, zoom: f32, pan: (f32, f32)) {
		let root = native_window(window).contentView().expect("content view");
		let frame = root.bounds();
		self.backdrop.setFrame(frame);
		// The clip excludes the GPUI toolbar; foreground remains a sibling above glass.
		self.container.setFrame(NSRect::new(
			NSPoint::new(0., 0.),
			NSSize::new(frame.size.width, (frame.size.height - 52.).max(1.)),
		));
		self.host.setFrame(self.container.bounds());
		self.container.setHidden(!enabled);
		if !enabled {
			return;
		}
		while self.cards.len() > count {
			self.cards.pop().expect("card").removeFromSuperview();
		}
		while self.cards.len() < count {
			unsafe {
				let glass: Retained<NSView> =
					msg_send![AnyClass::get(c"NSGlassEffectView").expect("glass"), new];
				let _: () = msg_send![&*glass, setStyle: 0_isize];
				self.host.addSubview(&glass);
				self.cards.push(glass);
			}
		}
		for (i, card) in self.cards.iter().enumerate() {
			let (x, y, w, h) = geometry(i, count, zoom, pan);
			let f = NSRect::new(
				NSPoint::new(f64::from(x), self.host.bounds().size.height - f64::from(y + h)),
				NSSize::new(f64::from(w), f64::from(h)),
			);
			card.setFrame(f);
			unsafe {
				let _: () = msg_send![&**card, setCornerRadius: f64::from(9.*zoom)];
			}
		}
	}
}
impl Drop for Material {
	fn drop(&mut self) {
		self.container.removeFromSuperview();
		self.backdrop.removeFromSuperview();
	}
}
fn geometry(i: usize, count: usize, zoom: f32, pan: (f32, f32)) -> (f32, f32, f32, f32) {
	let columns = (count as f32).sqrt().ceil() as usize;
	let x = 16. + (i % columns) as f32 * 164.;
	let y = 16. + (i / columns) as f32 * 68.;
	(x * zoom + pan.0, y * zoom + pan.1, 150. * zoom, 54. * zoom)
}
#[derive(Default)]
struct Samples {
	intervals: Vec<f64>,
	updates: Vec<f64>,
	inactive: usize,
}
struct Bench {
	case: usize,
	frame: usize,
	previous: Instant,
	samples: Samples,
	results: Vec<serde_json::Value>,
}
struct Graph {
	material: Option<Material>,
	focus: FocusHandle,
	count: usize,
	glass: bool,
	zoom: f32,
	pan: (f32, f32),
	drag: Option<gpui::Point<gpui::Pixels>>,
	bench: Option<Bench>,
	result: String,
}
fn percentile(values: &[f64], p: f64) -> f64 {
	let mut values = values.to_vec();
	values.sort_by(f64::total_cmp);
	values[((values.len() - 1) as f64 * p).round() as usize]
}
impl Graph {
	fn start(&mut self) {
		self.bench = Some(Bench {
			case: 0,
			frame: 0,
			previous: Instant::now(),
			samples: Samples::default(),
			results: vec![],
		});
		self.result = "Benchmark running · keep this window visible".into();
	}
}
impl Render for Graph {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		if self.material.is_none() {
			self.material = Some(Material::install(window));
		}
		let now = Instant::now();
		if let Some(b) = &mut self.bench {
			self.count = [10, 50, 100, 200][b.case / 2];
			self.glass = b.case % 2 == 1;
			let columns = (self.count as f32).sqrt().ceil();
			self.zoom = ((f32::from(window.viewport_size().width) - 40.) / (columns * 164.))
				.min(1.)
				* (0.90 + 0.05 * (b.frame as f32 * 0.08).sin());
			self.pan =
				(8. + 8. * (b.frame as f32 * 0.07).sin(), 8. + 8. * (b.frame as f32 * 0.09).cos());
			if b.frame >= 30 {
				b.samples.intervals.push(now.duration_since(b.previous).as_secs_f64() * 1000.);
				if !window.is_window_active() {
					b.samples.inactive += 1;
				}
			}
			b.previous = now;
		}
		let start = Instant::now();
		self.material
			.as_mut()
			.expect("installed")
			.sync(window, self.count, self.glass, self.zoom, self.pan);
		let update_ms = start.elapsed().as_secs_f64() * 1000.;
		let mut finished = false;
		if let Some(b) = &mut self.bench {
			if b.frame >= 30 {
				b.samples.updates.push(update_ms);
			}
			b.frame += 1;
			if b.frame >= 150 {
				b.results.push(serde_json::json!({"nodes":self.count,"native_glass":self.glass,"frames":b.samples.intervals.len(),"inactive_frames":b.samples.inactive,"frame_interval_p50_ms":percentile(&b.samples.intervals,0.5),"frame_interval_p95_ms":percentile(&b.samples.intervals,0.95),"native_update_p95_ms":percentile(&b.samples.updates,0.95),"frames_over_25ms":b.samples.intervals.iter().filter(|v| **v>25.).count()}));
				b.case += 1;
				b.frame = 0;
				b.samples = Samples::default();
				if b.case == 8 {
					std::fs::write(
						"/tmp/decodex-native-graph-benchmark.json",
						serde_json::to_vec_pretty(&b.results).expect("JSON"),
					)
					.expect("write local benchmark");
					finished = true;
				}
			}
			window.request_animation_frame();
		}
		if finished {
			self.bench = None;
			self.result = "Benchmark saved: /tmp/decodex-native-graph-benchmark.json".into();
		}
		let zoom = self.zoom;
		let pan = self.pan;
		let count = self.count;
		let mut canvas = div()
			.id("native-graph-canvas")
			.relative()
			.flex_1()
			.min_h_0()
			.overflow_hidden()
			.on_scroll_wheel(cx.listener(move |s, e: &gpui::ScrollWheelEvent, _, cx| {
				s.bench = None;
				s.zoom = (zoom * (f32::from(e.delta.pixel_delta(px(20.)).y) * 0.002).exp())
					.clamp(0.15, 2.);
				cx.notify();
			}))
			.on_mouse_down(
				gpui::MouseButton::Left,
				cx.listener(|s, e: &gpui::MouseDownEvent, _, _| s.drag = Some(e.position)),
			)
			.on_mouse_move(cx.listener(|s, e: &gpui::MouseMoveEvent, _, cx| {
				if e.pressed_button != Some(gpui::MouseButton::Left) {
					s.drag = None;
					return;
				}
				if let Some(last) = s.drag {
					s.bench = None;
					s.pan.0 += f32::from(e.position.x - last.x);
					s.pan.1 += f32::from(e.position.y - last.y);
					s.drag = Some(e.position);
					cx.notify();
				}
			}));
		canvas = canvas.child(
			gpui::canvas(
				|_, _, _| (),
				move |bounds, _, window, _| {
					for i in 1..count {
						let (x, y, _, h) = geometry(i, count, zoom, pan);
						let (a, b, w, k) = geometry(i - 1, count, zoom, pan);
						let mut path = gpui::PathBuilder::stroke(px(zoom.max(0.3)));
						path.move_to(bounds.origin + point(px(a + w), px(b + k / 2.)));
						path.line_to(bounds.origin + point(px(x), px(y + h / 2.)));
						if let Ok(path) = path.build() {
							window.paint_path(path, rgba(0x9aafff80));
						}
					}
				},
			)
			.absolute()
			.size_full(),
		);
		for i in 0..count {
			let (x, y, w, h) = geometry(i, count, zoom, pan);
			canvas = canvas.child(
				div()
					.absolute()
					.left(px(x))
					.top(px(y))
					.w(px(w))
					.h(px(h))
					.rounded(px(9. * zoom))
					.when(!self.glass, |d| {
						d.bg(rgba(0x33333aee)).border_1().border_color(rgba(0xffffff30))
					})
					.px(px(8. * zoom))
					.py(px(5. * zoom))
					.text_size(px(11. * zoom))
					.text_color(rgb(0xffffff))
					.child(format!("Agent {} · GPUI text", i + 1))
					.child(div().text_size(px(9. * zoom)).text_color(rgb(0xcac4f4)).child(format!(
						"{}K tokens · {}s",
						(i + 1) * 7,
						i + 4
					)))
					.child(
						div()
							.mt(px(3. * zoom))
							.h(px(3. * zoom))
							.w(gpui::relative(0.2 + (i % 8) as f32 * 0.1))
							.rounded_full()
							.bg(rgb(0xad9ae8)),
					),
			);
		}
		div()
			.size_full()
			.flex()
			.flex_col()
			.track_focus(&self.focus)
			.on_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, _, cx| {
				match e.keystroke.key.as_str() {
					"b" => s.start(),
					"g" => {
						s.bench = None;
						s.glass = !s.glass;
					},
					"f" => {
						s.bench = None;
						s.zoom = 1.;
						s.pan = (0., 0.);
					},
					"1" => {
						s.bench = None;
						s.count = 10;
					},
					"2" => {
						s.bench = None;
						s.count = 50;
					},
					"3" => {
						s.bench = None;
						s.count = 100;
					},
					"4" => {
						s.bench = None;
						s.count = 200;
					},
					"x" => {
						s.bench = None;
						s.material = None;
						s.count = 0;
					},
					_ => {},
				}
				cx.notify();
			}))
			.child(
				div()
					.h(px(52.))
					.flex_none()
					.px_3()
					.py_2()
					.bg(rgb(0x252830))
					.text_color(rgb(0xffffff))
					.text_size(px(12.))
					.child(format!(
						"GPUI + native glass · {} nodes · {} · {:.0}%",
						count,
						if self.glass { "Native" } else { "Plain" },
						zoom * 100.
					))
					.child(div().text_size(px(10.)).child(if self.result.is_empty() {
						"1–4: nodes · G: material · B: benchmark · F: reset · X: remove nodes · Synthetic data".into()
					} else {
						self.result.clone()
					})),
			)
			.child(canvas)
	}
}
pub(super) fn run() {
	gpui_platform::application().run(|cx: &mut App| {
		cx.open_window(
			WindowOptions {
				window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
					None,
					size(px(1200.), px(800.)),
					cx,
				))),
				window_background: WindowBackgroundAppearance::Transparent,
				titlebar: Some(gpui::TitlebarOptions {
					title: Some("GPUI Native Graph Lab".into()),
					..Default::default()
				}),
				..Default::default()
			},
			|window, cx| {
				let graph = cx.new(|cx| Graph {
					material: None,
					focus: cx.focus_handle(),
					count: 10,
					glass: true,
					zoom: 1.,
					pan: (0., 0.),
					drag: None,
					bench: None,
					result: String::new(),
				});
				let focus = graph.read(cx).focus.clone();
				window.focus(&focus, cx);
				window.on_window_should_close(cx, |_, cx| {
					cx.quit();
					true
				});
				graph
			},
		)
		.expect("open composition lab");
		cx.activate(true);
	});
}
