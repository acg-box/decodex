//! One visual grammar for graph edges. Line patterns denote relation types, not certainty.
use gpui::{KeyDownEvent, Path, PathStyle, Pixels, Point, StrokeOptions, Window};

use crate::{
	shell::{
		agent_surface::work_board::relations::{
			AgentSurface, AnyElement, AppContext, Context, InteractiveElement, IntoElement, Kind,
			MouseButton, ParentElement, PathBuilder, RelationTip, Role, Route, SUCCESS,
			StatefulInteractiveElement, Styled, TEXT_MUTED, WARNING,
		},
		workspace_symbols,
		workspace_symbols::Symbol,
	},
	ui_motion::SmoothControl,
	ui_theme::{self, ERROR, INFO},
};

/// Annotations share the same world coordinates and zoom as cards and routes.
pub(super) struct ConnectionLabel {
	pub text: String,
	pub width: f32,
}
impl ConnectionLabel {
	pub const FONT_SIZE: f32 = 10.;
	pub const HEIGHT: f32 = 18.;

	pub fn new(text: String) -> Self {
		Self { width: (text.chars().count() as f32 * 6. + 10.).max(22.), text }
	}

	/// World-space bounds used by both the rendered control and camera fitting.
	pub fn bounds(&self, anchor: (f32, f32)) -> [f32; 4] {
		let (x, y) = (self.width / 2., Self::HEIGHT / 2.);

		[anchor.0 - x, anchor.1 - y, anchor.0 + x, anchor.1 + y]
	}
}

/// Keep adjacent solid strokes together without changing their triangle order.
/// Separate paths let unrelated node backgrounds split the edge rendering into
/// repeated full-window intermediate passes.
#[derive(Default)]
pub(super) struct StrokeBatch(Vec<(u32, Path<Pixels>)>);
impl StrokeBatch {
	pub(super) fn push(&mut self, path: Path<Pixels>, color: u32) {
		if let Some((_, previous)) = self.0.last_mut().filter(|(previous, _)| *previous == color) {
			previous.bounds = previous.bounds.union(&path.bounds);

			previous.vertices.extend(path.vertices);
		} else {
			self.0.push((color, path));
		}
	}

	pub(super) fn paint(&mut self, window: &mut Window) {
		for (color, path) in self.0.drain(..) {
			window.paint_path(path, ui_theme::rgba(color));
		}
	}
}

impl AgentSurface {
	pub(super) fn relation_legend_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
		let hidden = !self.work_board.view.legend_visible;
		let label = if hidden { "Show legend" } else { "Hide legend" };

		gpui::div()
			.id("graph-legend-toggle")
			.debug_selector(|| "graph-legend-toggle".into())
			.role(Role::Button)
			.aria_label(label)
			.tab_index(0)
			.size(gpui::px(24.))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(5.))
			.cursor_pointer()
			.occlude()
			.hover(|d| d.bg(ui_theme::rgba(0xffffff10)))
			.tooltip(move |_, cx| cx.new(|_| RelationTip(label.into())).into())
			.child(workspace_symbols::icon_sized(
				if hidden { Symbol::Eye } else { Symbol::EyeSlash },
				14.,
			))
			.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
			.on_click(cx.listener(|s, _, _, cx| {
				s.work_board.view.legend_visible = !s.work_board.view.legend_visible;

				cx.notify();
			}))
			.on_key_down(cx.listener(|s, event: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.work_board.view.legend_visible = !s.work_board.view.legend_visible;

					cx.stop_propagation();
					cx.notify();
				}
			}))
			.smooth()
			.into_any_element()
	}

	pub(in super::super) fn relation_legend(&self) -> AnyElement {
		let mut links = gpui::div().flex().flex_wrap().items_center().gap(gpui::px(14.));

		for (kind, label) in [
			(Kind::Assigned, "Ownership"),
			(Kind::Message, "Collaboration"),
			(Kind::Resource, "Reference"),
			(Kind::Wait, "Dependency"),
		] {
			links = links.child(
				gpui::div()
					.flex()
					.items_center()
					.flex_none()
					.whitespace_nowrap()
					.gap(gpui::px(5.))
					.child(
						gpui::canvas(
							|_, _, _| (),
							move |bounds, _, window, _| {
								let route = Route::curve((1., 5.), (7., 5.), (14., 5.), (21., 5.));

								if let Ok(path) = route
									.stroke(kind, bounds.origin, 1., (0., 0.), 1.2, false)
									.build()
								{
									window.paint_path(path, ui_theme::rgb(kind.color()));
								}
							},
						)
						.w(gpui::px(22.))
						.h(gpui::px(10.)),
					)
					.child(label),
			);
		}

		let mut lights = gpui::div().flex().flex_wrap().items_center().gap(gpui::px(12.));

		for (color, label) in [
			(INFO, "Active"),
			(SUCCESS, "Complete"),
			(WARNING, "Waiting"),
			(ERROR, "Error"),
			(TEXT_MUTED, "Inactive / unknown"),
		] {
			lights = lights.child(
				gpui::div()
					.flex()
					.items_center()
					.flex_none()
					.whitespace_nowrap()
					.gap(gpui::px(5.))
					.child(gpui::div().size(gpui::px(5.)).rounded_full().bg(ui_theme::rgb(color)))
					.child(label),
			);
		}

		gpui::div()
			.flex_none()
			.flex()
			.flex_col()
			.items_end()
			.gap(gpui::px(5.))
			.text_size(gpui::px(10.))
			.text_color(ui_theme::rgb(TEXT_MUTED))
			.child(links)
			.child(lights)
			.child("Bars: tokens · time · speed")
			.child("Rulers: 2M tokens · 30 min · 100 tok/s")
			.child("Display scales · No limits or completion implied")
			.into_any_element()
	}
}

impl Kind {
	pub(super) fn pattern(self) -> Option<(f32, f32)> {
		match self {
			Self::Context | Self::Resource => Some((8., 5.)),
			Self::Dependency | Self::Wait => Some((2., 5.)),
			_ => None,
		}
	}
}

impl Route {
	pub(super) fn paint_flow(
		&self,
		origin: Point<Pixels>,
		scale: f32,
		pan: (f32, f32),
		phase: f32,
		opacity: f32,
		window: &mut Window,
	) {
		// Geometry owns the arc-length samples, including every rounded corner.
		// Moving the pulse does not resample paths or allocate a distance table.
		let samples = &self.samples;
		let total = samples.last().expect("curve end").1;
		let Some((tail, head, visibility)) = flow_window(total, phase) else { return };
		let point =
			|(x, y)| origin + gpui::point(gpui::px(x * scale + pan.0), gpui::px(y * scale + pan.1));
		let point_at_distance = |distance: f32| {
			let index = samples
				.partition_point(|(_, length)| *length < distance)
				.clamp(1, samples.len() - 1);
			let (a, before) = samples[index - 1];
			let (b, after) = samples[index];
			let span = after - before;
			let fraction = if span > 0. { (distance - before) / span } else { 0. };

			point((a.0 + (b.0 - a.0) * fraction, a.1 + (b.1 - a.1) * fraction))
		};
		let from = point_at_distance(tail);
		let to = point_at_distance(head);
		let angle = f32::from(to.x - from.x).atan2(-f32::from(to.y - from.y)).to_degrees();
		let mut trail = stroke_builder(1.6);

		trail.move_to(from);

		for &(sample, distance) in samples {
			if distance > tail && distance < head {
				trail.line_to(point(sample));
			}
		}

		trail.line_to(to);

		if let Ok(path) = trail.build() {
			window.paint_path(
				path,
				gpui::linear_gradient(
					angle.rem_euclid(360.),
					gpui::linear_color_stop(ui_theme::rgba(0xdbe5ff00), 0.),
					gpui::linear_color_stop(
						ui_theme::rgb(0xdbe5ff).opacity(opacity * visibility * 0.95),
						1.,
					),
				),
			);
		}
	}

	pub(super) fn stroke(
		&self,
		kind: Kind,
		origin: Point<Pixels>,
		scale: f32,
		pan: (f32, f32),
		width: f32,
		complete: bool,
	) -> PathBuilder {
		let point =
			|(x, y)| origin + gpui::point(gpui::px(x * scale + pan.0), gpui::px(y * scale + pan.1));
		let mut path = stroke_builder(width);

		if let Some((dash, gap)) = kind.pattern() {
			path = path
				.dash_array(&[gpui::px((dash * scale).max(2.)), gpui::px((gap * scale).max(3.))]);
		}

		for curve in if complete { &self.curves } else { &self.ink } {
			path.move_to(point(curve.start));
			path.cubic_bezier_to(point(curve.end), point(curve.first), point(curve.second));
		}

		if kind != Kind::Resource && (complete || self.ink_arrow) {
			let curve = self.curves.last().expect("connection curve");
			let end = point(curve.end);
			let dx = curve.end.0 - curve.second.0;
			let dy = curve.end.1 - curve.second.1;
			let norm = dx.hypot(dy).max(0.001);
			let (ux, uy) = (dx / norm, dy / norm);
			let (ux, uy) = (ux * scale, uy * scale);

			path.move_to(
				end + gpui::point(gpui::px(-ux * 6. - uy * 3.), gpui::px(-uy * 6. + ux * 3.)),
			);
			path.line_to(end);
			path.line_to(
				end + gpui::point(gpui::px(-ux * 6. + uy * 3.), gpui::px(-uy * 6. - ux * 3.)),
			);
		}

		path
	}
}

/// Stroke weight stays in display points. Zoom changes the route, not its
/// legibility. Native curve tessellation and MSAA cover the final pixel edges.
fn stroke_builder(width: f32) -> PathBuilder {
	PathBuilder::default().with_style(PathStyle::Stroke(
		StrokeOptions::default().with_line_width(width.max(1.5)).with_tolerance(0.015),
	))
}

/// Fade at both ends of the cycle; a clipped pulse must not flash at its destination.
fn flow_window(total: f32, phase: f32) -> Option<(f32, f32, f32)> {
	const TRAIL: f32 = 56.;

	let distance = phase * (total + TRAIL);
	let tail = (distance - TRAIL).clamp(0., total);
	let head = distance.min(total);
	let visible = head - tail;

	if visible <= 0. {
		return None;
	}

	let t = (visible / (TRAIL / 2.)).min(1.);

	Some((tail, head, t * t * (3. - 2. * t)))
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::work_board::relations::style::{self, Kind, Route};

	#[test]
	fn edges_keep_screen_weight_while_direction_cues_follow_the_camera() {
		let route = Route::curve((0., 0.), (40., 0.), (80., 0.), (120., 0.));

		for kind in [Kind::Assigned, Kind::Message, Kind::Resource] {
			let path = |scale| {
				route
					.stroke(
						kind,
						gpui::point(gpui::px(0.), gpui::px(0.)),
						scale,
						(0., 0.),
						1.,
						false,
					)
					.build()
					.unwrap()
			};
			let normal = path(1.).bounds.size;

			if kind == Kind::Resource {
				assert!(normal.height < gpui::px(2.), "references remain undirected");
			} else {
				assert!(
					normal.height >= gpui::px(6.),
					"direction must be visible before selection"
				);
			}

			for scale in [0.7, 0.8, 0.9] {
				let distant = path(scale).bounds.size;

				assert!(
					distant.height >= normal.height * scale
						&& distant.height <= normal.height * scale + gpui::px(1.),
					"{kind:?} at {scale}: {distant:?} vs {normal:?}"
				);
				assert!((f32::from(distant.width) - f32::from(normal.width) * scale).abs() <= 1.);
				assert!(distant.height >= gpui::px(1.), "one-point screen stroke survives zoom");
			}
		}
	}

	#[test]
	fn flow_enters_and_leaves_without_an_endpoint_flash() {
		for length in [16., 240., 1_600.] {
			assert!(style::flow_window(length, 0.).is_none());
			assert!(style::flow_window(length, 1.).is_none());

			let enter = style::flow_window(length, 0.0001).unwrap();
			let leave = style::flow_window(length, 0.9999).unwrap();

			assert!(enter.2 < 0.01 && leave.2 < 0.01);

			let mut previous = (0., 0.);

			for step in 1..165 {
				let (tail, head, alpha) = style::flow_window(length, step as f32 / 165.).unwrap();

				assert!(tail >= previous.0 && head >= previous.1);
				assert!(head <= length && tail <= head);
				assert!((0. ..=1.).contains(&alpha));

				previous = (tail, head);
			}
		}
	}
}
