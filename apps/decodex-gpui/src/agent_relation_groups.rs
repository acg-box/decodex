//! Open team brackets expose immediate ownership without adding another card.
use super::{Graph, card, stream};
use crate::ui_theme::{self, foreground};
use gpui::{ParentElement as _, Styled as _};

pub(super) fn paint(
	mut canvas: gpui::Stateful<gpui::Div>,
	graph: &Graph,
	scale: f32,
	pan: (f32, f32),
	picked: Option<&str>,
) -> gpui::Stateful<gpui::Div> {
	for leader in graph.nodes.iter().filter(|node| node.row.is_some()) {
		// The top coordinator has no enclosing bracket. Each lower team owns one.
		if !graph.edges.iter().any(|edge| edge.kind.hierarchy() && edge.to == leader.key) {
			continue;
		}
		let children: Vec<_> = graph
			.edges
			.iter()
			.filter(|edge| edge.kind.hierarchy() && edge.from == leader.key)
			.filter_map(|edge| graph.nodes.iter().find(|node| node.key == edge.to))
			.collect();
		if children.is_empty() {
			continue;
		}
		let highlighted = picked == Some(leader.key.as_str())
			|| children.iter().any(|child| picked == Some(child.key.as_str()));
		let mut bounds = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
		for node in std::iter::once(leader).chain(children) {
			let height = graph.layout.header_height(if graph.layout.activity_visible() {
				node.activity.height / stream::LINE
			} else {
				0.
			}) / scale;
			let top = node.y
				+ if graph.layout.overview() {
					(node.height() - height) / 2.
				} else {
					card::INSET_Y
				};
			bounds[0] = bounds[0].min(node.x - 6.);
			bounds[1] = bounds[1].min(top - 6.);
			bounds[2] = bounds[2].max(node.x + node.width() + 6.);
			bounds[3] = bounds[3].max(top + height + 6.);
		}
		let opacity = graph.layout.card_presence();
		let key = format!("relation-team-{}", leader.key);
		canvas = canvas.child(
			gpui::canvas(
				|_, _, _| (),
				move |canvas_bounds, _, window, cx| {
					let selected = crate::ui_motion::value(
						gpui::SharedString::from(key.clone()),
						if highlighted { 1. } else { 0. },
						window,
						cx,
					);
					let [l, t, r, b] = [
						bounds[0] * scale + pan.0,
						bounds[1] * scale + pan.1,
						bounds[2] * scale + pan.0,
						bounds[3] * scale + pan.1,
					];
					let radius = (6. * scale).max(3.);
					let arm = (14. * scale).max(6.);
					let p = |x, y| canvas_bounds.origin + gpui::point(gpui::px(x), gpui::px(y));
					let mut path = gpui::PathBuilder::stroke(gpui::px(0.75));
					for (edge, sign) in [(l, 1.), (r, -1.)] {
						path.move_to(p(edge + sign * arm, t));
						path.line_to(p(edge + sign * radius, t));
						path.curve_to(p(edge, t + radius), p(edge, t));
						path.line_to(p(edge, b - radius));
						path.curve_to(p(edge + sign * radius, b), p(edge, b));
						path.line_to(p(edge + sign * arm, b));
					}
					if let Ok(path) = path.build() {
						let color = if highlighted { ui_theme::BLUE } else { foreground::BODY };
						window.paint_path(
							path,
							ui_theme::rgb(color).opacity((0.30 + selected * 0.35) * opacity),
						);
					}
				},
			)
			.absolute()
			.inset_0()
			.size_full(),
		);
	}
	canvas
}
