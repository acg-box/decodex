//! Selection reveals the whole ownership branch, with no second routing algorithm.
use super::{BTreeMap, Graph, Node, card, header};
use crate::ui_theme;
use gpui::{ParentElement as _, Styled as _};

// Routing obstacles describe painted text, not the blank area below it.
// Keep the character/HUD attachment intact so connections still end outside it.
pub(super) fn routing_nodes(graph: &Graph) -> Vec<Node> {
	let layout = graph.layout;
	graph
		.nodes
		.iter()
		.cloned()
		.map(|mut node| {
			if node.row.is_some() && node.attachment.is_some() && layout.labels() {
				let text = layout.spacing(card::DETAILS_TOP)
					+ layout.title_line()
					+ if layout.activity_visible() {
						layout.spacing(card::IDENTITY_GAP)
							+ header::detail_rows(&node.assignment, &node.activity)
								* layout.activity_line()
					} else {
						0.
					};
				node.size.1 = card::INSET_Y + text / layout.zoom + 3.;
			}
			node
		})
		.collect()
}

pub(super) fn focus(graph: &Graph, picked: Option<&str>) -> BTreeMap<String, f32> {
	let mut result = BTreeMap::new();
	let Some(picked) = picked else {
		return result;
	};
	let mut depths = BTreeMap::from([(picked.to_owned(), 0_usize)]);
	let mut queue = vec![picked.to_owned()];
	while let Some(parent) = queue.pop() {
		let depth = depths[&parent];
		for edge in graph.edges.iter().filter(|edge| edge.kind.hierarchy() && edge.from == parent) {
			result.insert(edge.id.clone(), if depth == 0 { 0.84 } else { 0.58 });
			if !depths.contains_key(&edge.to) {
				depths.insert(edge.to.clone(), depth + 1);
				queue.push(edge.to.clone());
			}
		}
	}
	let mut ancestors = std::collections::BTreeSet::from([picked.to_owned()]);
	let mut queue = vec![picked.to_owned()];
	while let Some(child) = queue.pop() {
		for edge in graph.edges.iter().filter(|edge| edge.kind.hierarchy() && edge.to == child) {
			result.entry(edge.id.clone()).or_insert(0.34);
			if ancestors.insert(edge.from.clone()) {
				queue.push(edge.from.clone());
			}
		}
	}
	result
}

pub(super) fn paint(
	mut canvas: gpui::Stateful<gpui::Div>,
	graph: &Graph,
	scale: f32,
	pan: (f32, f32),
	picked: Option<&str>,
) -> gpui::Stateful<gpui::Div> {
	let focus = focus(graph, picked);
	let presence = graph.layout.card_presence();
	for edge in graph.edges.iter().filter(|edge| edge.kind.hierarchy()) {
		let target = focus.get(&edge.id).copied().unwrap_or(0.);
		let Some(path) = graph.paths.get(&edge.id).cloned() else { continue };
		let id = format!("team-focus-edge-{}", edge.id);
		canvas = canvas.child(
			gpui::div().absolute().inset_0().child(
				gpui::canvas(
					|_, _, _| (),
					move |bounds, _, window, cx| {
						let alpha = crate::ui_motion::value(id.clone(), target, window, cx);
						if alpha <= 0.001 {
							return;
						}
						let mut ink = gpui::PathBuilder::stroke(gpui::px(1.));
						let p = |point: (f32, f32)| {
							bounds.origin
								+ gpui::point(
									gpui::px(point.0 * scale + pan.0),
									gpui::px(point.1 * scale + pan.1),
								)
						};
						for curve in &path.curves {
							ink.move_to(p(curve.start));
							ink.cubic_bezier_to(p(curve.end), p(curve.first), p(curve.second));
						}
						if let Ok(ink) = ink.build() {
							window.paint_path(
								ink,
								ui_theme::rgb(ui_theme::BLUE).opacity(alpha * presence),
							);
						}
					},
				)
				.size_full(),
			),
		);
	}
	canvas
}
