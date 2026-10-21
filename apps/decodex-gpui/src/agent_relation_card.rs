//! Unframed agent footprints retain readable type throughout the supported camera range.
use crate::ui_motion::{self, Tween};
use gpui::{
	AnyElement, App, Div, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
	SharedString, Stateful, Styled as _, Window, prelude::FluentBuilder as _,
};

// Geometry stays in world space. Text stops shrinking before it becomes illegible.
// At 100%, graph type uses the same sizes as the native shell.
// Reserve the longest compact counters, including the native average-rate caption,
// at the full-detail threshold and the one-times display font floor.
// Values never resize a node.
pub(super) const WIDTH: f32 = 328.;
pub(super) const INSET_X: f32 = 10.;
pub(super) const INSET_Y: f32 = 8.;
pub(super) const PORT_Y: f32 = INSET_Y + crate::ui_theme::LABEL_LINE_HEIGHT / 2.;
pub(super) const SELECTED: u32 = crate::ui_theme::BLUE;
pub(super) const SECONDARY_TEXT: u32 = 0xc7c2cd;
pub(super) const TASK_TEXT: u32 = 0xe0d8ea;
pub(super) const ACTIVITY_TEXT: u32 = 0xd3cbdc;
pub(super) const ALLOCATION_TEXT: u32 = 0xc1b4dd;
pub(super) const MIN_ZOOM: f32 = 0.05;
pub(super) const MAX_ZOOM: f32 = 1.;
pub(super) const FULL_DETAIL_ZOOM: f32 = 0.7;
const POINT_ZOOM: f32 = 0.2;
const MORPH_MARGIN: f32 = 0.04;
const SUMMARY_ZOOM: f32 = 0.45;
pub(super) const OVERVIEW_HEIGHT: f32 = 64.;

#[derive(Clone, Copy)]
pub(super) struct Layout {
	pub width: f32,
	pub zoom: f32,
	pub moving: bool,
	display_scale: f32,
	presence: f32,
}
impl Layout {
	pub(super) fn new(zoom: f32) -> Self {
		let t = ((zoom - POINT_ZOOM + MORPH_MARGIN) / (2. * MORPH_MARGIN)).clamp(0., 1.);
		Self {
			width: WIDTH * zoom,
			zoom,
			moving: false,
			display_scale: 1.,
			presence: t * t * (3. - 2. * t),
		}
	}

	pub(super) fn with_presence(mut self, presence: f32, moving: bool) -> Self {
		self.presence = presence;
		self.moving = moving;
		self
	}

	pub(super) fn card_presence(self) -> f32 {
		self.presence
	}

	pub(super) fn on_display(mut self, scale: f32) -> Self {
		self.display_scale = scale.max(1.);
		self.width = self.pixel(self.width);
		self
	}

	pub(super) fn pixel(self, points: f32) -> f32 {
		(points * self.display_scale).round() / self.display_scale
	}

	/// Shape glyphs at the target display size, on its physical pixel grid.
	fn font(self, size: f32, minimum: f32) -> f32 {
		(size * self.zoom * self.display_scale).round().max((minimum * self.display_scale).ceil())
			/ self.display_scale
	}

	pub(super) fn title_size(self) -> f32 {
		self.font(crate::ui_theme::BODY_SIZE, if self.overview() { 9. } else { 10.5 })
	}

	pub(super) fn task_size(self) -> f32 {
		self.font(crate::ui_theme::BODY_SIZE - 1., 10.)
	}

	pub(super) fn detail_size(self) -> f32 {
		self.font(crate::ui_theme::CAPTION_SIZE, if self.overview() { 9. } else { 9.5 })
	}

	pub(super) fn full_detail(self) -> bool {
		self.zoom + 4. * f32::EPSILON >= FULL_DETAIL_ZOOM
	}

	pub(super) fn overview(self) -> bool {
		self.zoom + 4. * f32::EPSILON < SUMMARY_ZOOM
	}

	pub(super) fn points(self) -> bool {
		self.presence == 0.
	}

	pub(super) fn point_size(self) -> f32 {
		(self.zoom * 40.).clamp(3., 7.) / self.zoom
	}

	pub(super) fn node_size(self, height: f32) -> (f32, f32) {
		if !self.overview() {
			return (WIDTH, height);
		}
		let dot = self.point_size();
		(dot + (WIDTH - dot) * self.presence, dot + (height - dot) * self.presence)
	}

	/// Both representations retain one world-space center and one manual position.
	pub(super) fn node_inset(self) -> (f32, f32) {
		let (width, height) = self.node_size(OVERVIEW_HEIGHT);
		((WIDTH - width) / 2., (OVERVIEW_HEIGHT - height) / 2.)
	}

	pub(super) fn zoom_range(self) -> (f32, f32) {
		if self.full_detail() {
			(FULL_DETAIL_ZOOM, MAX_ZOOM)
		} else if self.zoom + 4. * f32::EPSILON < POINT_ZOOM {
			(MIN_ZOOM, POINT_ZOOM - 0.01)
		} else if self.overview() {
			(POINT_ZOOM, SUMMARY_ZOOM - 0.01)
		} else {
			(SUMMARY_ZOOM, FULL_DETAIL_ZOOM - 0.01)
		}
	}

	pub(super) fn title_line(self) -> f32 {
		((crate::ui_theme::LABEL_LINE_HEIGHT * self.zoom).max(self.title_size() + 3.)
			* self.display_scale)
			.ceil()
			/ self.display_scale
	}

	pub(super) fn detail_line(self) -> f32 {
		((crate::ui_theme::CAPTION_LINE_HEIGHT * self.zoom).max(self.detail_size() + 3.)
			* self.display_scale)
			.ceil()
			/ self.display_scale
	}

	pub(super) fn task_line(self) -> f32 {
		((crate::ui_theme::LABEL_LINE_HEIGHT * self.zoom).max(self.task_size() + 3.)
			* self.display_scale)
			.ceil()
			/ self.display_scale
	}

	pub(super) fn spacing(self, points: f32) -> f32 {
		self.pixel(points * self.zoom)
	}

	pub(super) fn gap(self) -> f32 {
		self.spacing(4.)
	}

	/// Values and their optional token allocation share one baseline.
	pub(super) fn metrics_height(self) -> f32 {
		self.detail_line()
	}

	/// Reserve the current content, not the maximum number of future action rows.
	/// Each detail tier keeps a stable footprint across its camera range and displays.
	pub(super) fn world_height(self, title_rows: usize, sections: usize, actions: f32) -> f32 {
		if self.overview() {
			return OVERVIEW_HEIGHT;
		}
		let (range, title_rows, sections, actions) = if self.full_detail() {
			([FULL_DETAIL_ZOOM, MAX_ZOOM], title_rows, sections, actions)
		} else {
			([SUMMARY_ZOOM, FULL_DETAIL_ZOOM - 0.01], title_rows, sections, 0.)
		};
		range.into_iter().fold(0., |height, zoom| {
			let layout = Self::new(zoom);
			let gaps = sections.saturating_sub(1) as f32 + if actions > 0. { 1. } else { 0. };
			let tasks = sections.saturating_sub(1);
			let header = title_rows.saturating_sub(tasks) as f32 * layout.title_line()
				+ tasks as f32 * layout.task_line()
				+ gaps * layout.spacing(2.)
				+ if actions > 0. { layout.spacing(4.) } else { 0. }
				+ actions * layout.detail_line();
			height.max(
				((header + layout.gap() + layout.metrics_height() + 2. * layout.spacing(INSET_Y))
					/ zoom)
					.ceil(),
			)
		})
	}
}

/// Keep native text at its display size while the footprint opens around its center.
pub(super) fn overview(
	key: &str,
	color: u32,
	selected: bool,
	layout: Layout,
	body: Option<AnyElement>,
) -> AnyElement {
	let presence = layout.card_presence();
	let inset = layout.node_inset();
	gpui::div()
		.relative()
		.size_full()
		.when_some(body, |view, body| {
			view.child(
				gpui::div()
					.absolute()
					.left(gpui::px(-layout.spacing(inset.0)))
					.top(gpui::px(-layout.spacing(inset.1)))
					.opacity(presence)
					.child(body),
			)
		})
		.when(presence < 1., |view| {
			view.child(
				gpui::div()
					.absolute()
					.inset_0()
					.flex()
					.items_center()
					.justify_center()
					// Retire the dot before it can overprint the centered agent number.
					.opacity((1. - 2. * presence).max(0.))
					.child(
						gpui::div()
							.size(gpui::px(layout.spacing(layout.point_size())))
							.child(point(key, color, selected)),
					),
			)
		})
		.into_any_element()
}

/// Distant agents retain only their real status and selection, with no card chrome.
fn point(key: &str, color: u32, selected: bool) -> AnyElement {
	let selector = format!("relation-point-{key}");
	gpui::div()
		.debug_selector(move || selector.clone())
		.relative()
		.size_full()
		.rounded_full()
		.bg(gpui::rgb(color))
		.when(selected, |dot| {
			dot.child(
				gpui::div()
					.absolute()
					.inset(gpui::px(-2.))
					.rounded_full()
					.border_1()
					.border_color(gpui::rgb(SELECTED)),
			)
		})
		.into_any_element()
}

/// Corner marks and actual routed endpoints share the card's vector surface.
/// Ports are local world coordinates; the router alone chooses their side.
pub(super) struct Frame {
	pub zoom: f32,
	pub selected: bool,
	pub presence: f32,
	pub ports: Vec<(f32, f32)>,
}
impl Frame {
	fn element(self) -> impl IntoElement {
		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, _| {
				let density = window.scale_factor();
				let snap = |v: f32| (v * density).round() / density;
				let left = snap(f32::from(bounds.left()));
				let top = snap(f32::from(bounds.top()));
				let right = snap(f32::from(bounds.right())) - 1.;
				let bottom = snap(f32::from(bounds.bottom())) - 1.;
				let length = snap((8. * self.zoom).max(5.));
				let color = gpui::rgb(if self.selected { SELECTED } else { 0xc2bbc9 })
					.opacity(self.presence * if self.selected { 0.8 } else { 0.4 });
				for (x, y, dx, dy) in [
					(left, top, 1., 1.),
					(right, top, -1., 1.),
					(left, bottom, 1., -1.),
					(right, bottom, -1., -1.),
				] {
					for (x, y, width, height) in [
						(x + if dx < 0. { 1. - length } else { 0. }, y, length, 1.),
						(x, y + if dy < 0. { 1. - length } else { 0. }, 1., length),
					] {
						window.paint_quad(gpui::fill(
							gpui::Bounds::new(
								gpui::point(gpui::px(x), gpui::px(y)),
								gpui::size(gpui::px(width), gpui::px(height)),
							),
							color,
						));
					}
				}
				for &(x, y) in &self.ports {
					let center = (
						snap(f32::from(bounds.left()) + x * self.zoom),
						snap(f32::from(bounds.top()) + y * self.zoom),
					);
					let point = |x, y| gpui::point(gpui::px(center.0 + x), gpui::px(center.1 + y));
					let (radius, control) = (2.5, 2.5 * 0.552_284_8);
					let mut ring = gpui::PathBuilder::stroke(gpui::px(1.));
					ring.move_to(point(radius, 0.));
					ring.cubic_bezier_to(
						point(0., radius),
						point(radius, control),
						point(control, radius),
					);
					ring.cubic_bezier_to(
						point(-radius, 0.),
						point(-control, radius),
						point(-radius, control),
					);
					ring.cubic_bezier_to(
						point(0., -radius),
						point(-radius, -control),
						point(-control, -radius),
					);
					ring.cubic_bezier_to(
						point(radius, 0.),
						point(control, -radius),
						point(radius, -control),
					);
					if let Ok(path) = ring.build() {
						window.paint_path(
							path,
							gpui::rgb(if self.selected { SELECTED } else { SECONDARY_TEXT })
								.opacity(self.presence),
						);
					}
				}
			},
		)
		.absolute()
		.inset_0()
	}
}

/// Pointer hit bounds follow the camera. Hover does not change node geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
	pub x: f32,
	pub y: f32,
	pub width: f32,
	pub height: f32,
}
impl Rect {
	pub(super) fn contains(self, x: f32, y: f32) -> bool {
		x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
	}
}

#[derive(IntoElement)]
pub(super) struct Card {
	pub key: String,
	pub surface: Stateful<Div>,
	pub content: AnyElement,
	pub faded: bool,
	pub visibility: f32,
	pub frame: Option<Frame>,
}
impl RenderOnce for Card {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(
			SharedString::from(format!("graph-card-motion-{}", self.key)),
			cx,
			|_, _| (Tween::new(0.), Tween::new(0.)),
		);
		let now = std::time::Instant::now();
		let (arrival, dim, moving) = state.update(cx, |state, _| {
			state.0.target(1., now);
			state.1.target(if self.faded { 1. } else { 0. }, now);
			(state.0.sample(now), state.1.sample(now), state.0.moving(now) || state.1.moving(now))
		});
		if moving {
			ui_motion::request_frame(window, cx);
		}
		self.surface
			.opacity(self.visibility * arrival)
			.when_some(self.frame, |surface, frame| surface.child(frame.element()))
			.child(gpui::div().size_full().opacity(1. - 0.08 * dim).child(self.content))
	}
}
