//! Unframed agent footprints retain readable type throughout the supported camera range.
use crate::{
	ui_motion::{self, Tween},
	ui_theme::foreground,
};
use gpui::{
	AnyElement, App, Div, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
	SharedString, Stateful, Styled as _, Window, prelude::FluentBuilder as _,
};

// Geometry stays in world space. Text stops shrinking before it becomes illegible.
// At 100%, graph type uses the same sizes as the native shell.
// A compact three-row HUD stays centered under the portrait, with readable
// counters at the full-detail threshold and the one-times display font floor.
pub(super) const WIDTH: f32 = 288.;
pub(super) const INSET_X: f32 = 10.;
pub(super) const INSET_Y: f32 = 8.;
pub(super) const PORTRAIT_WIDTH: f32 = 80.;
pub(super) const PORTRAIT_HEIGHT: f32 = 44.;
pub(super) const PORT_Y: f32 = INSET_Y + crate::ui_theme::LABEL_LINE_HEIGHT / 2.;
pub(super) const SELECTED: u32 = crate::ui_theme::BLUE;
pub(super) const SECONDARY_TEXT: u32 = foreground::SECONDARY;
pub(super) const ACTIVITY_TEXT: u32 = foreground::BODY;
pub(super) const MIN_ZOOM: f32 = 0.05;
pub(super) const MAX_ZOOM: f32 = 1.;
pub(super) const FULL_DETAIL_ZOOM: f32 = 0.7;
const POINT_ZOOM: f32 = 0.2;
const MORPH_MARGIN: f32 = 0.04;
const SUMMARY_ZOOM: f32 = 0.45;
pub(super) const OVERVIEW_HEIGHT: f32 = 112.;

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
		self.font(foreground::BODY_SIZE, if self.labels() { 10.5 } else { 9. })
	}

	pub(super) fn detail_size(self) -> f32 {
		self.font(foreground::CAPTION_SIZE, if self.overview() { 9. } else { 9.5 })
	}

	pub(super) fn metric_size(self) -> f32 {
		self.font(foreground::METRIC_SIZE, 10.)
	}

	pub(super) fn activity_size(self) -> f32 {
		self.font(foreground::CAPTION_SIZE, 10.5)
	}

	pub(super) fn full_detail(self) -> bool {
		self.zoom + 4. * f32::EPSILON >= FULL_DETAIL_ZOOM
	}

	pub(super) fn overview(self) -> bool {
		self.zoom + 4. * f32::EPSILON < SUMMARY_ZOOM
	}

	pub(super) fn labels(self) -> bool {
		self.zoom + 4. * f32::EPSILON >= 0.30
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
		((crate::ui_theme::CAPTION_LINE_HEIGHT * self.zoom)
			.max(self.metric_size().max(self.activity_size()) + 3.)
			* self.display_scale)
			.ceil()
			/ self.display_scale
	}

	pub(super) fn activity_line(self) -> f32 {
		((self.activity_size() + 2.) * self.display_scale).ceil() / self.display_scale
	}

	pub(super) fn spacing(self, points: f32) -> f32 {
		self.pixel(points * self.zoom)
	}

	pub(super) fn gap(self) -> f32 {
		self.spacing(2.)
	}

	/// Three compact status tracks form one block under the numbered portrait.
	pub(super) fn metrics_height(self) -> f32 {
		3. * self.detail_line() + 2. * self.spacing(1.)
	}

	pub(super) fn header_height(self, actions: f32) -> f32 {
		let portrait = self.spacing(PORTRAIT_HEIGHT)
			+ if self.labels() { self.title_line() + self.spacing(2.) } else { 0. }
			+ if !self.overview() { self.gap() + self.metrics_height() } else { 0. };
		let activity = self.spacing(8.)
			+ if actions > 0. { actions * self.activity_line() } else { self.title_line() };
		portrait.max(activity)
	}

	/// The avatar and current activity determine the footprint, without reserving
	/// blank rows for future updates. Values do not resize the agent.
	pub(super) fn world_height(self, actions: f32) -> f32 {
		if self.overview() {
			return OVERVIEW_HEIGHT;
		}
		// Keep one camera footprint per detail tier. Content stays top-aligned;
		// readable font floors must not move the agent under the zoom pointer.
		let (min, max) = self.zoom_range();
		let range = [min, max];
		range.into_iter().fold(0., |height, zoom| {
			let layout = Self::new(zoom);
			height
				.max(((layout.header_height(actions) + 2. * layout.spacing(INSET_Y)) / zoom).ceil())
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
		.bg(crate::ui_theme::rgb(color))
		.when(selected, |dot| {
			dot.child(
				gpui::div()
					.absolute()
					.inset(gpui::px(-2.))
					.rounded_full()
					.border_1()
					.border_color(crate::ui_theme::rgb(SELECTED)),
			)
		})
		.into_any_element()
}

/// Routed endpoints remain visible without framing the character and its activity.
/// Ports are local world coordinates; the router alone chooses their side.
pub(super) struct Ports {
	pub zoom: f32,
	pub selected: bool,
	pub presence: f32,
	pub ports: Vec<(f32, f32)>,
}
impl Ports {
	fn element(self) -> impl IntoElement {
		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window, _| {
				let density = window.scale_factor();
				let snap = |v: f32| (v * density).round() / density;
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
							crate::ui_theme::rgb(if self.selected {
								SELECTED
							} else {
								SECONDARY_TEXT
							})
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
	pub ports: Option<Ports>,
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
			.when_some(self.ports, |surface, ports| surface.child(ports.element()))
			.child(gpui::div().size_full().opacity(1. - 0.08 * dim).child(self.content))
	}
}
