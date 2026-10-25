//! A bounded overview of the real graph. Navigation never opens or sends to a chat.
use std::time::Instant;

use gpui::{Div, KeyDownEvent, Pixels, Point, Stateful};

use crate::{
	shell::agent_surface::work_board::relations::{
		AgentSurface, AnyElement, BTreeMap, BTreeSet, Context, Graph, InteractiveElement as _,
		IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, Node, ParentElement as _,
		PathBuilder, Role, SharedString, SmoothControl as _, StatefulInteractiveElement as _,
		Styled as _, TEXT, card,
		layout::{Mode, Presentation},
	},
	ui_theme::{self, BLUE},
};

const WIDTH: f32 = 184.;
const HEIGHT: f32 = 132.;
const INSET: f32 = 12.;
const MIN_AGENTS: usize = 24;

pub(super) struct Drag {
	start: Point<Pixels>,
	offset: (f32, f32),
	projection: Projection,
}

#[derive(Clone, Copy)]
struct Projection {
	bounds: [f32; 4],
	scale: f32,
	offset: (f32, f32),
}
impl Projection {
	fn new(bounds: [f32; 4], agents: usize, zoom: f32, viewport: (f32, f32)) -> Option<Self> {
		let (width, height) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);

		if agents < MIN_AGENTS
			|| viewport.0 <= 0.
			|| viewport.1 <= 0.
			|| (width * zoom <= viewport.0 - 24. && height * zoom <= viewport.1 - 24.)
		{
			return None;
		}

		let scale = ((WIDTH - 28.) / width.max(1.)).min((HEIGHT - 28.) / height.max(1.));

		Some(Self {
			bounds,
			scale,
			offset: ((WIDTH - width * scale) / 2., (HEIGHT - height * scale) / 2.),
		})
	}

	fn point(self, world: (f32, f32)) -> (f32, f32) {
		(
			self.offset.0 + (world.0 - self.bounds[0]) * self.scale,
			self.offset.1 + (world.1 - self.bounds[1]) * self.scale,
		)
	}

	fn world(self, point: (f32, f32)) -> (f32, f32) {
		(
			(point.0 - self.offset.0) / self.scale + self.bounds[0],
			(point.1 - self.offset.1) / self.scale + self.bounds[1],
		)
	}
}

impl AgentSurface {
	pub(super) fn relation_minimap(
		&self,
		graph: &Graph,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let agents: Vec<_> = graph.nodes.iter().filter(|node| node.row.is_some()).collect();
		let bounds = Self::minimap_bounds(&agents);
		let zoom = self.relation_scale();
		let view = &self.work_board.view;
		let projection = Projection::new(bounds, agents.len(), zoom, view.viewport)?;
		let points: BTreeMap<_, _> = agents
			.iter()
			.map(|node| {
				(
					node.key.clone(),
					projection.point((
						node.navigation.0 + node.width() / 2.,
						node.navigation.1 + node.layout_height / 2.,
					)),
				)
			})
			.collect();
		let links: Vec<_> = graph
			.edges
			.iter()
			.filter(|edge| edge.kind.hierarchy() && view.presentation == Presentation::Links)
			.filter_map(|edge| Some((*points.get(&edge.from)?, *points.get(&edge.to)?)))
			.collect();
		let (left, top) = projection.point((-view.pan.0 / zoom, -view.pan.1 / zoom));
		let mut map = gpui::div()
			.id("relations-minimap")
			.debug_selector(|| "relations-minimap".into())
			.aria_label("Graph overview. Click an area or drag the view rectangle to move.")
			.absolute()
			.left(gpui::px(INSET))
			.top(gpui::px(INSET))
			.w(gpui::px(WIDTH))
			.h(gpui::px(HEIGHT))
			.rounded(gpui::px(7.))
			.border_1()
			.border_color(ui_theme::rgba(0xffffff24))
			.bg(ui_theme::rgb(0x464646))
			.overflow_hidden()
			.occlude()
			.on_scroll(|_, _, cx| cx.stop_propagation())
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(move |s, e: &MouseDownEvent, _, cx| {
					let view = &s.work_board.view;
					let point = projection.world((
						f32::from(e.position.x) - view.origin.0 - INSET,
						f32::from(e.position.y) - view.origin.1 - INSET,
					));
					let zoom = s.relation_scale();
					let center = (
						(view.viewport.0 / 2. - view.pan.0) / zoom,
						(view.viewport.1 / 2. - view.pan.1) / zoom,
					);
					let inside = (point.0 - center.0).abs() < view.viewport.0 / zoom / 2.
						&& (point.1 - center.1).abs() < view.viewport.1 / zoom / 2.;

					if !inside {
						s.navigate_relation_center(point, cx);
					}

					s.work_board.view.map_drag = Some(Drag {
						start: e.position,
						offset: if inside {
							(center.0 - point.0, center.1 - point.1)
						} else {
							(0., 0.)
						},
						projection,
					});

					cx.stop_propagation();
				}),
			)
			.on_mouse_move(cx.listener(|s, e: &MouseMoveEvent, _, cx| {
				s.move_relation_minimap(e, cx);
				cx.stop_propagation();
			}))
			.on_mouse_up(
				MouseButton::Left,
				cx.listener(|s, _, _, cx| {
					s.work_board.view.map_drag = None;

					cx.stop_propagation();
				}),
			)
			.child(map_connections(links))
			.child(
				gpui::div()
					.absolute()
					.left(gpui::px(left))
					.top(gpui::px(top))
					.w(gpui::px(view.viewport.0 / zoom * projection.scale))
					.h(gpui::px(view.viewport.1 / zoom * projection.scale))
					.rounded(gpui::px(3.))
					.border_1()
					.border_color(ui_theme::rgba(0xb0c6f3b0))
					.bg(ui_theme::rgba(0x8baaf70a)),
			);

		for node in &agents {
			let point = points[&node.key];

			map = map.child(
				gpui::div()
					.absolute()
					.left(gpui::px(point.0 - 1.75))
					.top(gpui::px(point.1 - 1.75))
					.size(gpui::px(3.5))
					.rounded_full()
					.bg(ui_theme::rgb(node.color)),
			);
		}

		Some(self.relation_map_labels(map, graph, &points, cx))
	}

	fn minimap_bounds(agents: &[&Node]) -> [f32; 4] {
		let mut bounds = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

		for node in agents {
			let (x, y) = node.navigation;

			bounds[0] = bounds[0].min(x);
			bounds[1] = bounds[1].min(y);
			bounds[2] = bounds[2].max(x + node.width());
			bounds[3] = bounds[3].max(y + node.layout_height);
		}

		bounds
	}

	fn relation_map_labels(
		&self,
		mut map: Stateful<Div>,
		graph: &Graph,
		points: &BTreeMap<String, (f32, f32)>,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let view = &self.work_board.view;
		let incoming: BTreeSet<_> = graph
			.edges
			.iter()
			.filter(|edge| edge.kind.hierarchy())
			.map(|edge| edge.to.as_str())
			.collect();
		let roots: BTreeSet<_> = graph
			.nodes
			.iter()
			.filter(|node| node.row.is_some())
			.filter(|node| !incoming.contains(node.key.as_str()))
			.map(|node| node.key.as_str())
			.collect();
		let parents: BTreeSet<_> = graph
			.edges
			.iter()
			.filter(|edge| edge.kind.hierarchy())
			.map(|edge| edge.from.as_str())
			.collect();
		let first_layer: BTreeSet<_> = graph
			.edges
			.iter()
			.filter(|edge| edge.kind.hierarchy() && roots.contains(edge.from.as_str()))
			.map(|edge| edge.to.as_str())
			.collect();
		let mut labels = Vec::new();

		for node in graph.nodes.iter().filter(|node| node.row.is_some()).filter(|node| {
			roots.contains(node.key.as_str())
				|| (first_layer.contains(node.key.as_str()) && parents.contains(node.key.as_str()))
		}) {
			let row = node.row.as_ref().expect("agent row");
			let number = self.relation_agent_number(row);
			let label = if node.title == "Main" { "Main".to_owned() } else { number.clone() };
			let point = points[&node.key];
			let root = roots.contains(node.key.as_str());
			let width = (label.len() as f32 * 6. + 8.).max(24.);
			let mut bounds = card::Rect {
				x: if root && view.layout_mode != Mode::Tree {
					point.0 - width - 12.
				} else {
					point.0 - width / 2.
				},
				y: if root && view.layout_mode == Mode::Tree {
					point.1 - 25.
				} else if root {
					point.1 - 8.
				} else {
					point.1 + 5.
				},
				width,
				height: 16.,
			};

			bounds.x = bounds.x.clamp(4., WIDTH - bounds.width - 4.);
			bounds.y = bounds.y.clamp(4., HEIGHT - bounds.height - 4.);

			if labels.iter().any(|other| overlaps(&bounds, other)) {
				continue;
			}

			labels.push(bounds);

			map = map.child(self.relation_map_label(node, bounds, label, cx));
		}

		map.into_any_element()
	}

	fn relation_map_label(
		&self,
		node: &Node,
		bounds: card::Rect,
		label: String,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let row = node.row.as_ref().expect("agent row");
		let number = self.relation_agent_number(row);
		let key = node.key.clone();
		let keyboard = key.clone();
		let selector = format!("relations-map-agent-{}", node.key);
		let button = gpui::div()
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector.clone())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("Go to {number}, {}", node.title))
			.size_full()
			.flex()
			.items_center()
			.justify_center()
			.font_family("Menlo")
			.text_size(gpui::px(9.))
			.text_color(ui_theme::rgb(
				if self.work_board.view.picked_agent.as_ref() == Some(&node.key) {
					BLUE
				} else {
					TEXT
				},
			))
			.rounded(gpui::px(3.))
			.cursor_pointer()
			.hover(|d| d.bg(ui_theme::rgba(0xffffff12)))
			.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
			.on_click(cx.listener(move |s, _, _, cx| {
				s.navigate_relation_agent(&key, cx);
				cx.stop_propagation();
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					s.navigate_relation_agent(&keyboard, cx);
					cx.stop_propagation();
				}
			}))
			.child(label)
			.smooth();

		// Feedback uses relative positioning. Keep the map coordinates on its
		// outer footprint so labels cannot enter flow or cover another shortcut.
		gpui::div()
			.absolute()
			.left(gpui::px(bounds.x))
			.top(gpui::px(bounds.y))
			.w(gpui::px(bounds.width))
			.h(gpui::px(bounds.height))
			.child(button)
	}

	pub(super) fn move_relation_minimap(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
		if event.pressed_button != Some(MouseButton::Left) {
			self.work_board.view.map_drag = None;

			return;
		}

		let zoom = self.relation_scale();
		let view = &mut self.work_board.view;
		let Some(drag) = &view.map_drag else {
			return;
		};
		let delta = event.position - drag.start;

		if f32::from(delta.x).powi(2) + f32::from(delta.y).powi(2) < 9. {
			return;
		}

		let point = drag.projection.world((
			f32::from(event.position.x) - view.origin.0 - INSET,
			f32::from(event.position.y) - view.origin.1 - INSET,
		));

		view.pan = (
			view.viewport.0 / 2. - (point.0 + drag.offset.0) * zoom,
			view.viewport.1 / 2. - (point.1 + drag.offset.1) * zoom,
		);
		view.motion = None;
		view.camera_fixed = true;

		view.hover.update(None, Instant::now());
		cx.notify();
	}
}

fn overlaps(a: &card::Rect, b: &card::Rect) -> bool {
	a.x < b.x + b.width + 2.
		&& b.x < a.x + a.width + 2.
		&& a.y < b.y + b.height + 2.
		&& b.y < a.y + a.height + 2.
}

fn map_connections(links: Vec<((f32, f32), (f32, f32))>) -> impl IntoElement {
	gpui::canvas(
		|_, _, _| (),
		move |bounds, _, window, _| {
			let mut path = PathBuilder::stroke(gpui::px(0.7));

			for (from, to) in &links {
				path.move_to(bounds.origin + gpui::point(gpui::px(from.0), gpui::px(from.1)));
				path.line_to(bounds.origin + gpui::point(gpui::px(to.0), gpui::px(to.1)));
			}

			if let Ok(path) = path.build() {
				window.paint_path(path, ui_theme::rgba(0xd0c8d94a));
			}
		},
	)
	.absolute()
	.size_full()
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::work_board::relations::minimap::Projection;

	#[test]
	fn overview_requires_many_agents_and_content_larger_than_the_view() {
		let bounds = [0., 0., 3_000., 1_600.];

		assert!(Projection::new(bounds, 23, 1., (1_000., 700.)).is_none());
		assert!(Projection::new(bounds, 24, 1., (1_000., 700.)).is_some());
		assert!(Projection::new(bounds, 103, 0.25, (1_000., 700.)).is_none());
		assert!(Projection::new(bounds, 103, 1., (0., 0.)).is_none());
	}

	#[test]
	fn overview_round_trip_preserves_world_positions_in_any_layout() {
		for bounds in [[-2_000., -1_200., 2_000., 1_200.], [32., 48., 800., 5_000.]] {
			let projection = Projection::new(bounds, 103, 1., (815., 683.)).unwrap();

			for point in [(bounds[0], bounds[1]), (0., 0.), (bounds[2], bounds[3])] {
				let recovered = projection.world(projection.point(point));

				assert!((recovered.0 - point.0).abs() < 0.001);
				assert!((recovered.1 - point.1).abs() < 0.001);
			}
		}
	}
}
