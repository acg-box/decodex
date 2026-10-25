//! Unframed agent footprints retain readable type throughout the supported camera range.
use crate::{
	ui_motion::{self, Tween},
	ui_theme::foreground,
};
use gpui::{
	AnyElement, App, Div, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
	SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Window,
	prelude::FluentBuilder as _,
};

// Geometry stays in world space. Text stops shrinking before it becomes illegible.
// At 100%, graph type uses the same sizes as the native shell.
// A compact three-row HUD stays centered under the portrait, with readable
// counters at the full-detail threshold and the one-times display font floor.
// At 100%, a 256-point reading column leaves two complete agents within a
// normal Graph pane. Keep the portrait, native type and numeric floors intact.
pub(super) const WIDTH: f32 = 368.;
pub(super) const INSET_X: f32 = 10.;
pub(super) const INSET_Y: f32 = 8.;
pub(super) const PORTRAIT_WIDTH: f32 = 80.;
pub(super) const PORTRAIT_HEIGHT: f32 = 44.;
const TEXT_SIZE: f32 = 10.5;
const MIN_TEXT_SIZE: f32 = 10.;
pub(super) const SELECTED: u32 = crate::ui_theme::BLUE;
pub(super) const SECONDARY_TEXT: u32 = foreground::SECONDARY;
pub(super) const MIN_ZOOM: f32 = 0.05;
pub(super) const MAX_ZOOM: f32 = 1.;
pub(super) const FULL_DETAIL_ZOOM: f32 = 0.7;
const POINT_ZOOM: f32 = 0.2;
const MORPH_MARGIN: f32 = 0.04;
const SUMMARY_ZOOM: f32 = 0.45;
const LABEL_ZOOM: f32 = 0.30;
pub(super) const OVERVIEW_HEIGHT: f32 = 112.;

#[derive(Clone, Copy)]
pub(super) struct Layout {
	pub width: f32,
	pub zoom: f32,
	pub moving: bool,
	world_width: f32,
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
			world_width: WIDTH,
			display_scale: 1.,
			presence: t * t * (3. - 2. * t),
		}
	}

	pub(super) fn with_width(mut self, width: f32) -> Self {
		self.world_width = width;
		self.width = width * self.zoom;
		self
	}

	pub(super) fn world_width(self) -> f32 {
		self.world_width
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
		self.font(TEXT_SIZE, MIN_TEXT_SIZE)
	}

	pub(super) fn metric_size(self) -> f32 {
		self.title_size()
	}

	pub(super) fn activity_size(self) -> f32 {
		self.title_size()
	}

	pub(super) fn activity_visible(self) -> bool {
		self.labels() && self.zoom > 0.5 + f32::EPSILON
	}

	pub(super) fn text_width(self) -> f32 {
		(self.details_width() - 2. * self.bubble_padding() - self.title_size() - 5.).max(1.)
	}

	pub(super) fn details_width(self) -> f32 {
		self.spacing(256.).min(self.width - 2. * self.spacing(INSET_X)).max(1.)
	}

	pub(super) fn speech_left(self) -> f32 {
		(self.width - 2. * self.spacing(INSET_X) - self.details_width()) / 2.
	}

	pub(super) fn caption_width(self) -> f32 {
		self.spacing(220.).min(self.width - 2. * self.spacing(INSET_X)).max(1.)
	}

	pub(super) fn bubble_padding(self) -> f32 {
		self.spacing(8.).max(6.)
	}

	pub(super) fn speech_offset(self, rows: f32) -> f32 {
		if self.activity_visible() && rows > 0. {
			rows * self.activity_line() - self.spacing(2.)
		} else {
			0.
		}
	}

	/// The short tail stays centered over the character as the bubble resizes.
	pub(super) fn speech_tip(self, _height: f32) -> (f32, f32) {
		(self.details_width() / 2., self.spacing(5.).max(4.))
	}

	pub(super) fn full_detail(self) -> bool {
		self.zoom + 4. * f32::EPSILON >= FULL_DETAIL_ZOOM
	}

	pub(super) fn overview(self) -> bool {
		self.zoom + 4. * f32::EPSILON < SUMMARY_ZOOM
	}

	pub(super) fn labels(self) -> bool {
		self.zoom + 4. * f32::EPSILON >= LABEL_ZOOM
	}

	pub(super) fn points(self) -> bool {
		self.presence == 0.
	}

	pub(super) fn point_size(self) -> f32 {
		(self.zoom * 40.).clamp(3., 7.) / self.zoom
	}

	pub(super) fn node_size(self, height: f32) -> (f32, f32) {
		let width = self.world_width;
		if !self.overview() {
			return (width, height);
		}
		let dot = self.point_size();
		(dot + (width - dot) * self.presence, dot + (height - dot) * self.presence)
	}

	/// Both representations retain one world-space center and one manual position.
	pub(super) fn node_inset(self) -> (f32, f32) {
		let (width, height) = self.node_size(OVERVIEW_HEIGHT);
		((self.world_width - width) / 2., (OVERVIEW_HEIGHT - height) / 2.)
	}

	fn header_origin(self, actions: f32) -> (f32, f32) {
		let portrait = self.portrait_spacing(PORTRAIT_WIDTH) / self.zoom;
		let left = (self.world_width - portrait) / 2.;
		let top = if self.overview() {
			(OVERVIEW_HEIGHT - self.header_height(actions) / self.zoom) / 2.
		} else {
			INSET_Y + self.speech_offset(actions) / self.zoom
		};
		let inset = if self.overview() { self.node_inset() } else { (0., 0.) };
		(left - inset.0, top - inset.1)
	}

	pub(super) fn identity_bounds(self, actions: f32) -> Option<[f32; 4]> {
		self.labels().then(|| {
			let (left, top) = self.header_origin(actions);
			let top = top
				+ (self.portrait_spacing(PORTRAIT_HEIGHT)
					+ if self.overview() { 0. } else { self.gap() + self.metrics_height() }
					+ self.metadata_gap())
					/ self.zoom;
			[
				left,
				top,
				left + self.portrait_spacing(PORTRAIT_WIDTH) / self.zoom,
				top + self.title_line() / self.zoom,
			]
		})
	}

	/// Route around the character and its HUD, independently of the activity column.
	/// During the point morph the same attachment contracts into the visible dot.
	pub(super) fn character_bounds(self, actions: f32) -> [f32; 4] {
		let z = self.zoom;
		let portrait = self.portrait_spacing(PORTRAIT_WIDTH) / z;
		let height = (self.portrait_spacing(PORTRAIT_HEIGHT)
			+ if self.overview() { 0. } else { self.gap() + self.metrics_height() }
			+ self.metadata_height())
			/ z;
		let (left, top) = self.header_origin(actions);
		let (width, node_height) = self.node_size(self.world_height(actions));
		let center = (width / 2., node_height / 2.);
		let r = self.point_size() / 2.;
		let point = [center.0 - r, center.1 - r, center.0 + r, center.1 + r];
		let character = [left - 3., top - 3., left + portrait + 3., top + height + 3.];
		std::array::from_fn(|i| point[i] + (character[i] - point[i]) * self.presence)
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

	/// Hidden activity must not determine the height of a readable team.
	pub(super) fn activity_range(self) -> Option<(f32, f32)> {
		let (min, max) = self.zoom_range();
		(max > 0.5).then_some((min.max(0.5 + 2. * f32::EPSILON), max))
	}

	pub(super) fn at_zoom(self, zoom: f32) -> Self {
		Self::new(zoom).with_width(self.world_width).on_display(self.display_scale)
	}

	pub(super) fn metadata_gap(self) -> f32 {
		self.spacing(3.).max(2.)
	}

	pub(super) fn metadata_height(self) -> f32 {
		if self.labels() { self.title_line() + self.metadata_gap() } else { 0. }
	}

	pub(super) fn title_line(self) -> f32 {
		self.pixel(self.title_size() + 2.)
	}

	pub(super) fn metric_line(self) -> f32 {
		((self.metric_size() + 0.5) * self.display_scale).ceil() / self.display_scale
	}

	pub(super) fn activity_line(self) -> f32 {
		((self.activity_size() + 2.) * self.display_scale).ceil() / self.display_scale
	}

	pub(super) fn spacing(self, points: f32) -> f32 {
		self.pixel(points * self.zoom)
	}

	/// Balance the figure with readable counters without enlarging its 100% size.
	/// The floor grows gradually before the HUD appears, preserving the point morph.
	pub(super) fn portrait_spacing(self, points: f32) -> f32 {
		let presence = ((self.zoom - LABEL_ZOOM) / (SUMMARY_ZOOM - LABEL_ZOOM)).clamp(0., 1.);
		let width = self.pixel((PORTRAIT_WIDTH * self.zoom).max(56. * presence));
		self.pixel(points * width / PORTRAIT_WIDTH)
	}

	pub(super) fn gap(self) -> f32 {
		self.pixel(0.5).max(0.5)
	}

	/// Three compact status tracks form one block under the numbered portrait.
	pub(super) fn metrics_height(self) -> f32 {
		3. * self.metric_line()
	}

	pub(super) fn header_height(self, actions: f32) -> f32 {
		let portrait = self.portrait_spacing(PORTRAIT_HEIGHT)
			+ if self.overview() { 0. } else { self.gap() + self.metrics_height() };
		let details = if self.activity_visible() { actions * self.activity_line() } else { 0. };
		self.metadata_height() + portrait + details
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
		let range = [min, max].into_iter().chain(self.activity_range().map(|(min, _)| min));
		range.fold(0., |height, zoom| {
			let layout = self.at_zoom(zoom);
			height
				.max(((layout.header_height(actions) + 2. * layout.spacing(INSET_Y)) / zoom).ceil())
		})
	}
}

pub(super) fn number_features() -> gpui::FontFeatures {
	static FEATURES: std::sync::OnceLock<gpui::FontFeatures> = std::sync::OnceLock::new();
	FEATURES
		.get_or_init(|| gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])))
		.clone()
}

type PointerAction<E> = std::rc::Rc<dyn Fn(&E, &mut Window, &mut App)>;

/// The camera footprint is not an input target. Only visible content selects an
/// agent; only its portrait or overview point starts an agent drag.
#[derive(Clone)]
pub(super) struct Interaction {
	pub click: PointerAction<gpui::ClickEvent>,
	pub drag: PointerAction<gpui::MouseDownEvent>,
	pub team: PointerAction<gpui::ClickEvent>,
}
impl Interaction {
	pub(super) fn keyboard(&self, view: Stateful<Div>) -> Stateful<Div> {
		Self::keyboard_action(view, &self.click)
	}

	fn keyboard_action(
		view: Stateful<Div>,
		action: &PointerAction<gpui::ClickEvent>,
	) -> Stateful<Div> {
		let keyboard = action.clone();
		let accessible = action.clone();
		view.on_key_down(move |event, window, cx| {
			if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				keyboard(&gpui::ClickEvent::default(), window, cx);
				cx.stop_propagation();
			}
		})
		.on_a11y_action(gpui::AccessibleAction::Click, move |_, window, cx| {
			accessible(&gpui::ClickEvent::default(), window, cx);
		})
	}

	pub(super) fn frame_team(&self, view: Stateful<Div>) -> Stateful<Div> {
		let action = self.team.clone();
		Self::keyboard_action(view, &self.team)
			.cursor_pointer()
			.on_click(move |event, window, cx| action(event, window, cx))
	}

	pub(super) fn apply(&self, view: Stateful<Div>, draggable: bool) -> Stateful<Div> {
		let click = self.click.clone();
		let drag = self.drag.clone();
		view.cursor_pointer().on_click(move |event, window, cx| click(event, window, cx)).when(
			draggable,
			|view| {
				view.on_mouse_down(gpui::MouseButton::Left, move |event, window, cx| {
					drag(event, window, cx);
				})
			},
		)
	}
}

/// Keep native text at its display size while the footprint opens around its center.
pub(super) fn overview(
	key: &str,
	color: u32,
	selected: bool,
	layout: Layout,
	body: Option<AnyElement>,
	interaction: &Interaction,
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
						interaction.apply(
							gpui::div()
								.id(SharedString::from(format!("relation-point-target-{key}")))
								.size(gpui::px(layout.spacing(layout.point_size())))
								.child(point(key, color, selected)),
							true,
						),
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
