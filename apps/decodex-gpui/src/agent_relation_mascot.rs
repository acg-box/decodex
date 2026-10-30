//! Native Cloudlets geometry from the approved catalog, colored by the active theme.
#[path = "agent_relation_mascot_motion.rs"] mod motion;

use std::{
	cell::{Cell, RefCell},
	collections::BTreeMap,
	rc::Rc,
	sync::{Arc, LazyLock},
	time::{Duration, Instant},
};

use gpui::{App, ContentMask, IntoElement, Pixels, Styled as _, Window};
use serde::Deserialize;

use crate::{
	shell::agent_surface::work_board::relations::card::{Layout, PORTRAIT_HEIGHT, PORTRAIT_WIDTH},
	ui_motion,
	ui_theme::ColorScheme,
};
use motion::State;

const IDLE_INTERVAL: Duration = Duration::from_millis(50);
const ACTION_INTERVAL: Duration = Duration::from_millis(16);

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../../assets/mascots/cloudlets/catalog.json"
	)))
	.expect("checked-in Cloudlets catalog")
});

/// Poll poses without drawing identical pixels. Other motion keeps display cadence.
#[derive(Clone, Default)]
pub(super) struct Clock(Rc<Cell<u64>>);
impl Clock {
	fn cancel(&self) {
		if self.0.get() % 2 == 1 {
			self.0.set(self.0.get().wrapping_add(1));
		}
	}

	fn request(
		&self,
		window: &Window,
		cx: &mut App,
		interval: Duration,
		mut changed: impl FnMut() -> Option<bool> + 'static,
	) {
		if !ui_motion::animations_active() || self.0.get() % 2 == 1 {
			return;
		}

		let token = self.0.get().wrapping_add(1);

		self.0.set(token);

		let pending = self.0.clone();
		let target = window.window_handle();
		let view = window.current_view();

		cx.spawn(async move |cx| {
			while pending.get() == token {
				cx.background_executor().timer(interval).await;

				if pending.get() != token {
					break;
				}

				let result = target.update(cx, |_, window, cx| {
					if !window.is_visible() {
						// Keep one dirty pose ready for the next visible frame.
						cx.notify(view);

						return None;
					}
					if ui_motion::reduced() || !ui_motion::animations_active() {
						return None;
					}

					let redraw = changed();

					if redraw == Some(true) {
						cx.notify(view);
					}

					redraw
				});

				if !matches!(result, Ok(Some(false))) {
					break;
				}
			}

			if pending.get() == token {
				pending.set(token.wrapping_add(1));
			}
		})
		.detach();
	}
}

/// Layout publishes visible actor slots; only this sibling layer advances their poses.
#[derive(Clone, Default)]
pub(super) struct Scene {
	placements: Rc<RefCell<Vec<Placement>>>,
	painted: Rc<RefCell<Painted>>,
	clock: Clock,
}
impl Scene {
	pub(super) fn clear(&self) {
		self.clock.cancel();
		self.placements.borrow_mut().clear();
		self.painted.borrow_mut().pixels.clear();
	}

	fn pixels(
		placements: &[Placement],
		now: Instant,
		density: f32,
		mask: ContentMask<Pixels>,
		reduced: bool,
	) -> Vec<PaintedPixel> {
		let mut pixels = Vec::new();

		for placement in placements {
			let opacity = placement.presence * placement.figure.opacity.get();
			let clip = placement.mask.bounds.intersect(&mask.bounds);

			if opacity <= 0.001 || placement.bounds.intersect(&clip).is_empty() {
				continue;
			}

			let start = pixels.len();

			placement.figure.pixels(placement.bounds, now, density, opacity, reduced, &mut pixels);

			for pixel in &mut pixels[start..] {
				pixel.bounds = pixel.bounds.intersect(&clip);
			}
		}

		pixels.retain(|pixel| !pixel.bounds.is_empty());

		pixels
	}

	pub(super) fn element(&self) -> impl IntoElement {
		let scene = self.clone();

		gpui::canvas(
			|_, _, _| (),
			move |_, _, window, cx| {
				let density = window.scale_factor();
				let mask = window.content_mask();
				let reduced = ui_motion::reduced();
				let pixels = Self::pixels(
					&scene.placements.borrow(),
					Instant::now(),
					density,
					mask,
					reduced,
				);

				// This sibling layer has a fixed paint order. One layer bound avoids
				// inserting every artwork rectangle into GPUI's overlap tree.
				window.paint_layer(mask.bounds, |window| {
					for pixel in &pixels {
						window.paint_quad(gpui::fill(
							pixel.bounds,
							gpui::rgb(pixel.color).opacity(pixel.opacity),
						));
					}
				});

				let visible = !pixels.is_empty();

				scene.painted.replace(Painted { pixels, density, mask });

				if visible && !reduced {
					// Keep the existing idle artwork cadence. Active poses can advance
					// at display speed, independently of scrolling and camera motion.
					let interval = if scene.placements.borrow().iter().any(|placement| {
						placement.figure.sample(Instant::now(), false).0 != State::Idle
					}) {
						ACTION_INTERVAL
					} else {
						IDLE_INTERVAL
					};
					let placements = Rc::downgrade(&scene.placements);
					let painted = Rc::downgrade(&scene.painted);

					scene.clock.request(window, cx, interval, move || {
						let placements = placements.upgrade()?;
						let painted = painted.upgrade()?;
						let painted = painted.borrow();

						Some(
							Self::pixels(
								&placements.borrow(),
								Instant::now(),
								painted.density,
								painted.mask,
								false,
							) != painted.pixels,
						)
					});
				} else {
					scene.clock.cancel();
				}
			},
		)
		.size_full()
	}
}

/// One actor per stable agent identity; repeated reads do not restart its motion.
pub(super) struct Actor {
	seed: String,
	theme: ColorScheme,
	drawing: Arc<Drawing>,
	scene: Scene,
	opacity: Rc<Cell<f32>>,
	state: State,
	changed: Instant,
	arrival: bool,
	phase: f32,
}
impl Actor {
	pub(super) fn new(seed: &str, status: &str, now: Instant, scene: Scene) -> Self {
		let theme = ColorScheme::current();

		Self {
			seed: seed.into(),
			theme,
			drawing: Arc::new(Drawing::new(seed, theme)),
			scene,
			opacity: Rc::new(Cell::new(1.)),
			state: State::for_status(status),
			changed: now,
			arrival: false,
			phase: (hash(seed) % 5_800) as f32 / 1_000.,
		}
	}

	pub(super) fn opacity(&self) -> Rc<Cell<f32>> {
		self.opacity.clone()
	}

	pub(super) fn frame(&mut self, status: &str, now: Instant) -> Avatar {
		let theme = ColorScheme::current();

		if theme != self.theme {
			self.drawing = Arc::new(Drawing::new(&self.seed, theme));
			self.theme = theme;
		}

		let state = State::for_status(status);

		if self.state == State::Done && state == State::Idle {
			if now.duration_since(self.changed).as_secs_f32() >= 2.2 {
				self.state = State::Idle;
			}
		} else if state != self.state {
			self.arrival = state == State::Working;

			let completed =
				self.state == State::Working && matches!(status, "Idle" | "Marked complete");

			self.state = if completed { State::Done } else { state };
			self.changed = now;
		}

		let seconds = now.saturating_duration_since(self.changed).as_secs_f32();
		let animation = if self.arrival && self.state == State::Working && seconds < 0.9 {
			State::Hopping
		} else {
			self.state
		};

		Avatar {
			scene: self.scene.clone(),
			figure: Figure {
				drawing: self.drawing.clone(),
				opacity: self.opacity.clone(),
				state: animation,
				seconds,
				at: now,
				settles: match animation {
					State::Hopping if self.arrival => Some(State::Working),
					State::Done => Some(State::Idle),
					_ => None,
				},
				phase: self.phase,
			},
		}
	}
}

pub(super) struct Avatar {
	scene: Scene,
	figure: Figure,
}
impl Avatar {
	/// Travel is a presentation pose; it never changes the native work state.
	pub(super) fn travelling(mut self, seconds: f32) -> Self {
		self.figure.state = State::Hopping;
		self.figure.seconds = seconds;
		self.figure.settles = None;

		self
	}

	pub(super) fn element(self, layout: Layout) -> impl IntoElement {
		let presence = if layout.overview() { layout.card_presence() } else { 1. };

		gpui::canvas(
			move |bounds, window, _| {
				let mask = window.content_mask();

				if !bounds.intersect(&mask.bounds).is_empty() {
					self.scene.placements.borrow_mut().push(Placement {
						figure: self.figure,
						bounds,
						mask,
						presence,
					});
				}
			},
			|_, _, _, _| {},
		)
		.w(gpui::px(layout.portrait_spacing(PORTRAIT_WIDTH)))
		.h(gpui::px(layout.portrait_spacing(PORTRAIT_HEIGHT)))
	}
}

#[derive(Default)]
struct Painted {
	pixels: Vec<PaintedPixel>,
	density: f32,
	mask: ContentMask<Pixels>,
}

#[derive(Debug, PartialEq)]
struct PaintedPixel {
	bounds: gpui::Bounds<Pixels>,
	color: u32,
	opacity: f32,
}

struct Placement {
	figure: Figure,
	bounds: gpui::Bounds<Pixels>,
	mask: ContentMask<Pixels>,
	presence: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
	bodies: BTreeMap<String, Body>,
	palettes: BTreeMap<String, BTreeMap<String, String>>,
	expressions: BTreeMap<String, Part>,
	accessories: BTreeMap<String, Part>,
	props: BTreeMap<String, Part>,
	hand_styles: BTreeMap<String, Part>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Body {
	first_row: usize,
	rows: Vec<[f32; 2]>,
	shade: Vec<[f32; 4]>,
	highlight: Vec<[f32; 4]>,
	particles: Vec<[f32; 4]>,
	anchors: BTreeMap<String, [f32; 2]>,
}

#[derive(Deserialize)]
struct Part {
	rects: Vec<(f32, f32, f32, f32, String)>,
	anchor: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct Traits {
	palette: String,
	expression: String,
	accessory: String,
	prop: String,
	held_right: bool,
}
impl Traits {
	fn for_seed(seed: &str) -> Self {
		Self {
			palette: pick(seed, "palette", &CATALOG.palettes),
			expression: pick(seed, "expression", &CATALOG.expressions),
			accessory: pick(seed, "accessory", &CATALOG.accessories),
			prop: pick(seed, "prop", &CATALOG.props),
			held_right: hash(&format!("{seed}:held-side")) % 2 == 1,
		}
	}
}

struct Pixel {
	rect: [f32; 4],
	color: u32,
}

struct Drawing {
	cloud: Vec<Pixel>,
	eyes: Vec<Pixel>,
	left: Vec<Pixel>,
	right: Vec<Pixel>,
	particles: Vec<Pixel>,
	held_right: bool,
}
impl Drawing {
	fn new(seed: &str, theme: ColorScheme) -> Self {
		let traits = Traits::for_seed(seed);
		let body = &CATALOG.bodies["original"];
		let palette = theme.mascot_colors(&traits.palette);
		let color = |fill: &str| match fill {
			"base" => palette.base,
			"shade" => palette.shade,
			"light" => palette.light,
			"ink" => palette.ink,
			_ => palette.accessory(
				u32::from_str_radix(fill.trim_start_matches('#'), 16).expect("catalog color"),
			),
		};
		let pixels = |part: &Part, origin: [f32; 2]| -> Vec<Pixel> {
			part.rects
				.iter()
				.map(|(x, y, w, h, fill)| Pixel {
					rect: [origin[0] + x, origin[1] + y, *w, *h],
					color: color(fill),
				})
				.collect()
		};
		let mut cloud: Vec<_> = body
			.rows
			.iter()
			.enumerate()
			.map(|(row, [x, width])| Pixel {
				rect: [*x, (body.first_row + row) as f32, *width, 1.],
				color: color("base"),
			})
			.collect();

		// Clip the shade and highlight to the same silhouette as the source SVG.
		for (rects, fill) in [(&body.shade, "shade"), (&body.highlight, "light")] {
			for [x, y, width, height] in rects {
				for (row, [body_x, body_width]) in body.rows.iter().enumerate() {
					let row_y = (body.first_row + row) as f32;
					let left = x.max(*body_x);
					let top = y.max(row_y);
					let right = (x + width).min(body_x + body_width);
					let bottom = (y + height).min(row_y + 1.);

					if right > left && bottom > top {
						cloud.push(Pixel {
							rect: [left, top, right - left, bottom - top],
							color: color(fill),
						});
					}
				}
			}
		}

		let eyes = pixels(&CATALOG.expressions[&traits.expression], body.anchors["face"]);
		let accessory = &CATALOG.accessories[&traits.accessory];

		cloud.extend(pixels(
			accessory,
			body.anchors[accessory.anchor.as_deref().unwrap_or("accessory")],
		));

		let hand = |side: &str, held: bool| {
			let origin = body.anchors[side];
			let mut result = pixels(&CATALOG.hand_styles["floating"], origin);

			if held {
				result.extend(pixels(&CATALOG.props[&traits.prop], [origin[0], origin[1] - 1.]));
			}

			result
		};

		Self {
			cloud,
			eyes,
			left: hand("leftHand", !traits.held_right),
			right: hand("rightHand", traits.held_right),
			particles: body
				.particles
				.iter()
				.map(|rect| Pixel { rect: *rect, color: color("light") })
				.collect(),
			held_right: traits.held_right,
		}
	}
}

struct Figure {
	drawing: Arc<Drawing>,
	opacity: Rc<Cell<f32>>,
	state: State,
	seconds: f32,
	at: Instant,
	settles: Option<State>,
	phase: f32,
}
impl Figure {
	fn sample(&self, now: Instant, reduced: bool) -> (State, f32) {
		if reduced {
			return (State::Idle, 0.);
		}

		let mut seconds = self.seconds + now.saturating_duration_since(self.at).as_secs_f32();
		let mut state = self.state;

		if let Some(next) = self.settles
			&& seconds >= if state == State::Done { 2.2 } else { 0.9 }
		{
			state = next;
		}
		// A snapshot can introduce many actors in the same frame. Offset their
		// recurring gestures without shifting arrival or completion timing.
		if matches!(state, State::Idle | State::Working | State::Waiting | State::Attention) {
			seconds += self.phase;
		}

		(state, seconds)
	}

	fn pixels(
		&self,
		bounds: gpui::Bounds<Pixels>,
		now: Instant,
		density: f32,
		alpha: f32,
		reduced: bool,
		pixels: &mut Vec<PaintedPixel>,
	) {
		let (state, seconds) = self.sample(now, reduced);
		let pose = state.pose(seconds, self.drawing.held_right);
		let scale = f32::from(bounds.size.width) / 40.;
		let snap = |value: f32| (value * density).round() / density;
		let mut paint = |pixel: &Pixel, offset: [f32; 2], size: f32, opacity: f32| {
			let [x, y, w, h] = pixel.rect;
			// Move each part as one physical-pixel translation. Rounding every
			// rectangle after adding a subpixel pose deforms the outline and
			// creates redraws even when the part has not moved by a pixel.
			let origin = [
				snap(f32::from(bounds.left())) + snap(offset[0] * scale),
				snap(f32::from(bounds.top())) + snap(offset[1] * scale),
			];
			let left = origin[0] + snap((x + w * (1. - size) / 2.) * scale);
			let top = origin[1] + snap((y + h * (1. - size) / 2.) * scale);
			let right = origin[0] + snap((x + w * (1. + size) / 2.) * scale);
			let bottom = origin[1] + snap((y + h * (1. + size) / 2.) * scale);

			pixels.push(PaintedPixel {
				bounds: gpui::Bounds::new(
					gpui::point(gpui::px(left), gpui::px(top)),
					gpui::size(gpui::px(right - left), gpui::px(bottom - top)),
				),
				color: pixel.color,
				opacity: opacity * alpha,
			});
		};

		for (index, pixel) in self.drawing.particles.iter().enumerate() {
			let particle = pose.particle(state, seconds, index);

			paint(pixel, particle.offset, particle.size, particle.opacity);
		}
		for (pixels, offset) in [
			(&self.drawing.cloud, pose.cloud),
			(&self.drawing.left, pose.left),
			(&self.drawing.right, pose.right),
		] {
			for pixel in pixels {
				paint(pixel, offset, 1., 1.);
			}
		}
		for eye in &self.drawing.eyes {
			if pose.blink {
				let [x, _, w, _] = eye.rect;

				paint(&Pixel { rect: [x, 17.3, w, 0.45], color: eye.color }, pose.cloud, 1., 1.);
			} else {
				paint(eye, pose.cloud, 1., 1.);
			}
		}
	}
}

fn hash(value: &str) -> u32 {
	value.bytes().fold(0x811c_9dc5, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193))
}

fn pick<T>(seed: &str, category: &str, choices: &BTreeMap<String, T>) -> String {
	choices
		.keys()
		.max_by_key(|id| hash(&format!("{seed}:{category}:{id}")))
		.expect("catalog choices")
		.clone()
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use crate::{
		shell::agent_surface::work_board::relations::mascot::{
			self, Actor, Arc, CATALOG, Cell, Clock, ColorScheme, Drawing, IDLE_INTERVAL, Instant,
			IntoElement, Rc, Scene, State, Traits, Window,
		},
		ui_theme::{
			ColorTheme,
			appearance::{self, Theme},
		},
	};

	struct AnimationFixture {
		clock: Clock,
		visible: bool,
	}
	impl gpui::Render for AnimationFixture {
		fn render(
			&mut self,
			window: &mut Window,
			cx: &mut gpui::Context<Self>,
		) -> impl IntoElement {
			if self.visible {
				self.clock.request(window, cx, IDLE_INTERVAL, || Some(true));
				self.clock.request(window, cx, IDLE_INTERVAL, || Some(true));
			}

			gpui::div()
		}
	}

	struct PixelFixture {
		clock: Clock,
		changed: Rc<Cell<bool>>,
		checks: Rc<Cell<usize>>,
	}
	impl gpui::Render for PixelFixture {
		fn render(
			&mut self,
			window: &mut Window,
			cx: &mut gpui::Context<Self>,
		) -> impl IntoElement {
			let changed = self.changed.clone();
			let checks = self.checks.clone();

			self.clock.request(window, cx, IDLE_INTERVAL, move || {
				checks.set(checks.get() + 1);

				Some(changed.get())
			});

			gpui::div()
		}
	}

	#[gpui::test]
	fn visible_actors_share_one_tick_and_hidden_actors_stop(cx: &mut gpui::TestAppContext) {
		let (view, visual) =
			cx.add_window_view(|_, _| AnimationFixture { clock: Clock::default(), visible: true });

		visual.update(crate::ui_frame::draw_current_frame);
		visual.run_until_parked();

		let notifications = Rc::new(Cell::new(0));

		visual.update(|_, cx| {
			let notifications = notifications.clone();

			cx.observe(&view, move |_, _| notifications.set(notifications.get() + 1)).detach();
		});

		visual.executor().advance_clock(Duration::from_millis(49));
		visual.run_until_parked();

		assert_eq!(notifications.get(), 0);

		visual.executor().advance_clock(Duration::from_millis(1));
		visual.run_until_parked();

		assert_eq!(notifications.get(), 1, "visible actors must share one redraw notification");

		view.update(visual, |view, cx| {
			view.visible = false;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);
		visual.run_until_parked();
		notifications.set(0);
		// A tick that was queued while visible may finish once after hiding.
		visual.executor().advance_clock(Duration::from_millis(50));
		visual.run_until_parked();

		assert!(notifications.get() <= 1);

		notifications.set(0);
		visual.executor().advance_clock(Duration::from_millis(150));
		visual.run_until_parked();

		assert_eq!(notifications.get(), 0, "a hidden graph must not retain an animation loop");
	}

	#[gpui::test]
	fn unchanged_pixels_do_not_request_frames_and_cancellation_stops_polling(
		cx: &mut gpui::TestAppContext,
	) {
		let notifications = Rc::new(Cell::new(0));
		let changed = Rc::new(Cell::new(false));
		let checks = Rc::new(Cell::new(0));
		let (view, visual) = cx.add_window_view(|_, _| PixelFixture {
			clock: Clock::default(),
			changed: changed.clone(),
			checks: checks.clone(),
		});

		visual.update(|window, cx| {
			let notifications = notifications.clone();

			cx.observe(&view, move |_, _| notifications.set(notifications.get() + 1)).detach();
			window.draw(cx).clear(cx);
		});

		for _ in 0..4 {
			visual.executor().advance_clock(IDLE_INTERVAL);
			visual.run_until_parked();
		}

		assert!(checks.get() > 0);
		assert_eq!(notifications.get(), 0);

		changed.set(true);
		visual.executor().advance_clock(IDLE_INTERVAL);
		visual.run_until_parked();

		assert_eq!(notifications.get(), 1);

		visual.update(|window, cx| {
			changed.set(false);
			window.draw(cx).clear(cx);

			let clock = view.read(cx).clock.clone();

			clock.cancel();
		});

		let before = checks.get();

		visual.executor().advance_clock(Duration::from_millis(100));
		visual.run_until_parked();

		assert_eq!(checks.get(), before);
	}

	#[test]
	fn pixel_comparison_keeps_crisp_geometry_and_ignores_subpixel_pose_changes() {
		let now = Instant::now();
		let mut actor = Actor::new("main", "Idle", now, Scene::default());
		let figure = actor.frame("Idle", now).figure;
		let bounds = gpui::Bounds::new(
			gpui::point(gpui::px(20.25), gpui::px(30.25)),
			gpui::size(gpui::px(13.2), gpui::px(10.56)),
		);
		let pixels = |time| {
			let mut result = Vec::new();

			figure.pixels(bounds, time, 2., 0.8, false, &mut result);

			result
		};
		let first = pixels(now);

		assert_eq!(pixels(now + Duration::from_micros(1)), first);
		assert!((1..625).any(|step| pixels(now + Duration::from_millis(step * 16)) != first));

		let frames: Vec<_> =
			(0..120).map(|step| pixels(now + Duration::from_millis(step * 16))).collect();
		let changed = frames.windows(2).filter(|pair| pair[0] != pair[1]).count();

		assert!(changed < 60, "small actors must not draw every display tick: {changed}");
		assert!(first.iter().all(|pixel| {
			[pixel.bounds.left(), pixel.bounds.top(), pixel.bounds.right(), pixel.bounds.bottom()]
				.into_iter()
				.all(|edge| (f32::from(edge) * 2.).fract() == 0.)
		}));
	}

	#[test]
	fn moving_a_pixel_part_keeps_its_outline_rigid_at_thumbnail_zoom() {
		let now = Instant::now();
		let mut actor = Actor::new("main", "Waiting", now, Scene::default());
		let mut figure = actor.frame("Waiting", now).figure;

		// Exercise a known downward step, independently of the actor's phase.
		figure.phase = 0.;

		let bounds = gpui::Bounds::new(
			gpui::point(gpui::px(20.25), gpui::px(30.25)),
			gpui::size(gpui::px(13.2), gpui::px(10.56)),
		);
		let mut before = Vec::new();
		let mut after = Vec::new();

		figure.pixels(bounds, now, 2., 1., false, &mut before);
		figure.pixels(bounds, now + Duration::from_millis(1_600), 2., 1., false, &mut after);

		let start = figure.drawing.particles.len();

		for index in start..start + figure.drawing.cloud.len() {
			assert_eq!(before[index].bounds.size, after[index].bounds.size);
			assert_eq!(
				after[index].bounds.origin - before[index].bounds.origin,
				gpui::point(gpui::px(0.), gpui::px(0.5))
			);
		}
	}

	#[test]
	fn artwork_is_finite_and_identity_is_stable() {
		for seed in ["main", "release", "native:thread-1", "专项负责人", "A1234"] {
			assert_eq!(Traits::for_seed(seed), Traits::for_seed(seed));

			let drawing = Drawing::new(seed, ColorScheme::current());

			for pixel in drawing
				.cloud
				.iter()
				.chain(&drawing.left)
				.chain(&drawing.right)
				.chain(&drawing.particles)
			{
				assert!(pixel.rect.iter().all(|value| value.is_finite()));
				assert!(pixel.rect[2] > 0. && pixel.rect[3] > 0.);
			}
		}

		assert_ne!(Traits::for_seed("main"), Traits::for_seed("release"));
	}
	#[test]
	fn source_seed_mapping_matches_the_asset_generator() {
		assert_eq!(mascot::hash("hello"), 0x4f9f2cab);
		assert_eq!(
			Traits::for_seed("main"),
			Traits {
				palette: "butter".into(),
				expression: "wink".into(),
				accessory: "leaf-pin".into(),
				prop: "paintbrush".into(),
				held_right: true
			}
		);
		assert_eq!(
			Traits::for_seed("专项负责人"),
			Traits {
				palette: "lilac".into(),
				expression: "dot".into(),
				accessory: "headset".into(),
				prop: "camera".into(),
				held_right: false
			}
		);
	}

	#[gpui::test]
	fn theme_changes_recolor_cached_artwork_without_restarting_motion(
		cx: &mut gpui::TestAppContext,
	) {
		let original = ColorTheme::configured();

		appearance::with_theme(Theme::Dark, || {
			cx.update(|cx| ColorTheme::Decodex.apply(cx));

			let now = Instant::now();
			let mut actor = Actor::new("main", "Running", now, Scene::default());
			let first = actor.frame("Running", now).figure.drawing;
			let repeated = actor.frame("Running", now + Duration::from_millis(10)).figure.drawing;

			assert!(Arc::ptr_eq(&first, &repeated));

			cx.update(|cx| ColorTheme::Nightfox.apply(cx));

			let themed = actor.frame("Running", now + Duration::from_millis(20)).figure.drawing;

			assert!(!Arc::ptr_eq(&first, &themed));
			assert_eq!(actor.changed, now);
			assert_eq!(actor.state, State::Working);
			assert_eq!(
				first.cloud.iter().map(|p| p.rect).collect::<Vec<_>>(),
				themed.cloud.iter().map(|p| p.rect).collect::<Vec<_>>()
			);
			assert_ne!(first.cloud[0].color, themed.cloud[0].color);
			assert!(Arc::ptr_eq(
				&themed,
				&actor.frame("Running", now + Duration::from_millis(30)).figure.drawing
			));
		});

		cx.update(|cx| original.apply(cx));

		assert_eq!(CATALOG.palettes.len(), crate::ui_theme::MASCOT_FAMILIES.len());

		for family in CATALOG.palettes.keys() {
			assert!(crate::ui_theme::MASCOT_FAMILIES.contains(&family.as_str()));
		}
	}
	#[test]
	fn completion_plays_once_and_repeated_reads_do_not_restart_it() {
		let now = Instant::now();
		let later = |ms| now + Duration::from_millis(ms);
		let mut actor = Actor::new("main", "Running", now, Scene::default());

		assert_eq!(actor.frame("Running", later(200)).figure.state, State::Working);
		assert_eq!(actor.frame("Idle", later(300)).figure.state, State::Done);
		assert_eq!(actor.frame("Idle", later(900)).figure.state, State::Done);
		assert_eq!(actor.frame("Idle", later(2_600)).figure.state, State::Idle);
		assert_eq!(actor.frame("Idle", later(2_800)).figure.state, State::Idle);
		assert_eq!(actor.frame("Running", later(3_000)).figure.state, State::Hopping);
		assert_eq!(actor.frame("Running", later(4_000)).figure.state, State::Working);
	}

	#[test]
	fn lost_execution_status_does_not_celebrate_a_completion() {
		let now = Instant::now();

		for status in ["Not running", "Status unavailable", "Unknown", "Archived", "Error"] {
			let mut actor = Actor::new("main", "Running", now, Scene::default());

			assert_ne!(actor.frame(status, now).figure.state, State::Done);
		}
	}

	#[test]
	fn cached_figures_advance_and_finish_without_a_model_refresh() {
		let now = Instant::now();
		let mut actor = Actor::new("main", "Idle", now, Scene::default());
		let started = now + Duration::from_millis(100);
		let hopping = actor.frame("Running", started).figure;

		assert_eq!(hopping.sample(started, false).0, State::Hopping);
		assert_eq!(hopping.sample(started + Duration::from_secs(1), false).0, State::Working);

		let completed = started + Duration::from_secs(2);
		let done = actor.frame("Idle", completed).figure;

		assert_eq!(done.sample(completed, true), (State::Idle, 0.));
		assert_eq!(done.sample(completed, false).0, State::Done);
		assert_eq!(done.sample(completed + Duration::from_secs(3), false).0, State::Idle);
		assert!(
			done.sample(completed + Duration::from_secs(4), false).1
				> done.sample(completed + Duration::from_secs(3), false).1
		);
	}

	#[test]
	fn simultaneous_workers_keep_individual_phases_across_model_refreshes() {
		let now = Instant::now();
		let mut first = Actor::new("mira", "Running", now, Scene::default());
		let mut second = Actor::new("sage", "Running", now, Scene::default());
		let a = first.frame("Running", now).figure;
		let b = second.frame("Running", now).figure;
		let later = now + Duration::from_secs(1);

		assert_eq!(
			a.sample(later, false),
			first.frame("Running", later).figure.sample(later, false)
		);
		assert!((0..30).any(|frame| {
			let at = now + Duration::from_millis(frame * 40);
			let pose = |figure: &super::Figure| {
				let (state, seconds) = figure.sample(at, false);

				state.pose(seconds, figure.drawing.held_right).cloud
			};

			pose(&a) != pose(&b)
		}));
	}
}
