//! Solid graph cards sit above the transparent workspace.
use crate::{
	ui_motion::{self, Tween},
	ui_theme,
};
use gpui::{
	AnyElement, App, Div, IntoElement, ParentElement as _, RenderOnce, SharedString, Stateful,
	Styled as _, Window,
};

// Card geometry stays in world space. The camera scales every part together.
pub(super) const WIDTH: f32 = 176.;
pub(super) const HEIGHT: f32 = 112.;
pub(super) const MIN_ZOOM: f32 = 0.05;
pub(super) const MAX_ZOOM: f32 = 1.;

#[derive(Clone, Copy)]
pub(super) struct Layout {
	pub width: f32,
	pub height: f32,
	pub text_scale: f32,
	pub chrome_scale: f32,
	pub inset_x: f32,
	pub inset_y: f32,
	pub gap: f32,
	pub task_lines: usize,
	pub preview_lines: usize,
}
impl Layout {
	pub(super) fn new(zoom: f32) -> Self {
		Self {
			width: WIDTH * zoom,
			height: HEIGHT * zoom,
			text_scale: zoom,
			chrome_scale: zoom,
			inset_x: 10. * zoom,
			inset_y: 8. * zoom,
			gap: 4. * zoom,
			task_lines: 1,
			preview_lines: 2,
		}
	}

	pub(super) fn metrics_height(self) -> f32 {
		28. * self.text_scale
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
	pub emphasis: f32,
	pub faded: bool,
	pub visibility: f32,
}
impl RenderOnce for Card {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(
			SharedString::from(format!("graph-card-motion-{}", self.key)),
			cx,
			|_, _| (Tween::new(0.), Tween::new(0.), Tween::new(0.)),
		);
		let now = std::time::Instant::now();
		let (arrival, emphasis, dim, moving) = state.update(cx, |state, _| {
			state.0.target(1., now);
			state.1.target(self.emphasis, now);
			state.2.target(if self.faded { 1. } else { 0. }, now);
			(
				state.0.sample(now),
				state.1.sample(now),
				state.2.sample(now),
				state.0.moving(now) || state.1.moving(now) || state.2.moving(now),
			)
		});
		if moving {
			ui_motion::request_frame(window, cx);
		}
		self.surface
			.bg(gpui::rgb(0x222228).blend(gpui::rgb(0xffffff).alpha(0.025 * emphasis)))
			.border_color(gpui::rgb(ui_theme::LINE_STRONG).alpha(0.23 + 0.32 * emphasis))
			.opacity(self.visibility * arrival)
			.child(gpui::div().size_full().opacity(1. - 0.16 * dim).child(self.content))
	}
}
