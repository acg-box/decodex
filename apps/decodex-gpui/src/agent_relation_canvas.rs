//! Agent topology: typed connections are inspectable evidence, never inferred from message prose.
#[cfg(any(test, feature = "visual-capture"))] use super::Brief;
use super::{
	AMBER, AgentSurface, AnyElement, BTreeMap, BTreeSet, Context, FluentBuilder, GREEN,
	InteractiveElement, IntoElement, ParentElement, Row, Styled, TEXT_MUTED,
};
use crate::ui_theme::TEXT;
use gpui::AppContext;
#[path = "agent_relation_card.rs"] mod card;
#[path = "agent_relation_details.rs"] mod details;
#[path = "agent_relation_header.rs"] mod header;
#[path = "agent_relation_hover.rs"] mod hover;
#[path = "agent_relation_layout.rs"] mod layout;
#[path = "agent_relation_metrics.rs"] pub(super) mod metrics;
#[path = "agent_relation_resources.rs"] mod resources;
#[path = "agent_relation_route.rs"] mod routing;
use routing::Route;
#[cfg(test)] use routing::route;
#[path = "agent_relation_scopes.rs"] mod scopes;
#[path = "agent_relation_style.rs"] mod style;
#[path = "agent_relation_visibility.rs"] mod visibility;
use gpui::{
	MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Role, ScrollWheelEvent, SharedString,
	StatefulInteractiveElement,
};

const GRID: f32 = 16.;
const FRAME_INSET: f32 = 16.;
fn snap(value: f32) -> f32 {
	(value / GRID).round() * GRID
}
fn grid_spacing(scale: f32) -> f32 {
	// Omit minor dots at distant zoom levels, preserving the same world-space lattice.
	GRID * scale * (8. / (GRID * scale)).max(1.).log2().ceil().exp2()
}

#[derive(Default)]
pub(super) struct View {
	pub(super) camera_fixed: bool,
	reveal_after_resize: bool,
	flow_clock: std::cell::OnceCell<std::time::Instant>,
	positions: BTreeMap<String, (f32, f32)>,
	pan: (f32, f32),
	zoom: Option<f32>,
	viewport: (f32, f32),
	layout_viewport: (f32, f32),
	layout: std::cell::RefCell<layout::Cache>,
	routing: std::cell::RefCell<routing::Cache>,
	visibility: std::cell::RefCell<visibility::Cache>,
	origin: (f32, f32),
	node_count: usize,
	drag: Option<Drag>,
	moved: bool,
	pub(super) edge: Option<Edge>,
	pub(super) show_record: bool,
	legend_visible: bool,
	hover: hover::Hover,
	pub(super) picked_agent: Option<String>,
}
struct Drag {
	key: Option<String>,
	start: gpui::Point<gpui::Pixels>,
	last: gpui::Point<gpui::Pixels>,
	origin: (f32, f32),
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
	Assigned,
	Spawned,
	Dependency,
	Context,
	Resource,
	Message,
	Control,
	Wait,
	Returned,
}
impl Kind {
	fn hierarchy(self) -> bool {
		matches!(self, Self::Assigned | Self::Spawned)
	}

	fn color(self) -> u32 {
		match self {
			Self::Assigned | Self::Spawned => 0xb5b2bd,
			Self::Dependency => AMBER,
			Self::Context => 0xb0a0d8,
			Self::Resource => 0x9cbacb,
			Self::Message => 0x8b9fff,
			Self::Control => TEXT_MUTED,
			Self::Wait => AMBER,
			Self::Returned => GREEN,
		}
	}
}
#[derive(Clone)]
pub(super) struct Edge {
	pub(super) id: String,
	pub(super) from: String,
	pub(super) to: String,
	pub(super) kind: Kind,
	in_progress: bool,
	color: u32,
	label: String,
	detail: String,
	excerpt: Option<String>,
	source: Option<(String, String)>,
	link: Option<String>,
}
impl Edge {
	fn executing(&self, target: &Node, connected: bool) -> bool {
		connected
			&& (self.in_progress
				|| (self.kind.hierarchy() && self.to == target.key && target.running()))
	}

	fn same_pair(&self, other: &Self) -> bool {
		(self.from == other.from && self.to == other.to)
			|| (self.from == other.to && self.to == other.from)
	}
}

#[derive(Clone)]
pub(super) struct Node {
	pub(super) key: String,
	title: String,
	status: String,
	color: u32,
	row: Option<Row>,
	pub(super) size: (f32, f32),
	pub(super) x: f32,
	pub(super) y: f32,
}
fn compact_name(value: &str) -> String {
	let first = value.split(['\n', '：']).next().unwrap_or(value).trim();
	if first.chars().count() <= 32 {
		first.to_owned()
	} else {
		format!("{}…", first.chars().take(31).collect::<String>().trim_end())
	}
}
impl Node {
	fn screen_rect(&self, scale: f32, pan: (f32, f32)) -> card::Rect {
		card::Rect {
			x: self.x * scale + pan.0,
			y: self.y * scale + pan.1,
			width: self.width() * scale,
			height: self.height() * scale,
		}
	}

	fn running(&self) -> bool {
		self.row.as_ref().is_some_and(|r| r.status == "Running")
	}

	fn width(&self) -> f32 {
		self.size.0
	}

	fn height(&self) -> f32 {
		self.size.1
	}
}
struct RelationTip(String);
impl gpui::Render for RelationTip {
	fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
		crate::ui_motion::tooltip_surface(gpui::div())
			.p_2()
			.max_w(gpui::px(360.))
			.text_color(gpui::rgb(TEXT))
			.text_size(gpui::px(12.))
			.child(self.0.clone())
	}
}
pub(super) struct Graph {
	scopes: Vec<scopes::Scope>,
	pub(super) nodes: Vec<Node>,
	pub(super) edges: Vec<Edge>,
	paths: BTreeMap<String, std::rc::Rc<Route>>,
}
impl Graph {
	fn connections(&self) -> Vec<Edge> {
		let mut connections: Vec<Edge> = Vec::new();
		for edge in &self.edges {
			if let Some(existing) = connections.iter_mut().find(|existing| existing.same_pair(edge))
			{
				if edge.kind.hierarchy() && !existing.kind.hierarchy() {
					*existing = edge.clone();
				}
			} else {
				connections.push(edge.clone());
			}
		}
		connections
	}

	#[cfg(test)]
	fn content_bounds(&self, keys: &BTreeSet<String>) -> Option<[f32; 4]> {
		self.bounds(keys, true)
	}

	fn layout_bounds(&self, keys: &BTreeSet<String>) -> Option<[f32; 4]> {
		self.bounds(keys, false)
	}

	fn connection_label(&self, edge: &Edge) -> style::ConnectionLabel {
		let count = self.edges.iter().filter(|other| edge.same_pair(other)).count();
		style::ConnectionLabel::new(if count > 1 { count.to_string() } else { edge.label.clone() })
	}

	fn bounds(&self, keys: &BTreeSet<String>, routes: bool) -> Option<[f32; 4]> {
		let mut bounds: Option<[f32; 4]> = None;
		let mut include = |x: f32, y: f32| {
			let b = bounds.get_or_insert([x, y, x, y]);
			b[0] = b[0].min(x);
			b[1] = b[1].min(y);
			b[2] = b[2].max(x);
			b[3] = b[3].max(y);
		};
		for node in self.nodes.iter().filter(|n| keys.contains(&n.key)) {
			include(node.x, node.y);
			include(node.x + node.width(), node.y + node.height());
		}
		for scope in &self.scopes {
			include(scope.bounds[0], scope.bounds[1]);
			include(scope.bounds[2], scope.bounds[3]);
		}
		if !routes {
			return bounds;
		}
		for edge in self.connections() {
			if !keys.contains(&edge.from) || !keys.contains(&edge.to) {
				continue;
			}
			let Some(_) = self.nodes.iter().find(|n| n.key == edge.from) else {
				continue;
			};
			let Some(_) = self.nodes.iter().find(|n| n.key == edge.to) else {
				continue;
			};
			let path = &self.paths[&edge.id];
			for i in 0..=160 {
				let (x, y) = path.at(i as f32 / 160.);
				include(x, y);
			}
			let [left, top, right, bottom] = self.connection_label(&edge).bounds(path.label);
			include(left, top);
			include(right, bottom);
		}
		bounds
	}

	#[cfg(test)]
	pub(super) fn agent_count(&self) -> usize {
		self.nodes.iter().filter(|node| node.row.is_some()).count()
	}
}

impl AgentSurface {
	pub(in super::super) fn clear_relation_selection(&mut self) {
		self.work_board.view.picked_agent = None;
		self.work_board.view.edge = None;
		self.work_board.focus = None;
	}

	pub(super) fn relation_graph(&self) -> Graph {
		let mut graph =
			Graph { scopes: vec![], nodes: vec![], edges: vec![], paths: BTreeMap::new() };
		let Some(snapshot) = &self.snapshot else { return graph };
		let rows = self.factory_rows();
		let included: BTreeSet<String> = rows.iter().map(|r| r.key.clone()).collect();
		for source in rows.iter().filter(|r| included.contains(&r.key)) {
			let mut row = source.clone();
			if self.graph_question(&row).is_some() {
				row.status = "Needs you".into();
				row.color = AMBER;
			}
			graph.nodes.push(Node {
				key: row.key.clone(),
				title: compact_name(if row.native { &row.owner } else { &row.title }),
				status: if row.review {
					format!("{} · Result unread", row.status)
				} else {
					row.status.clone()
				},
				color: row.color,
				row: Some(row),
				size: (card::WIDTH, card::HEIGHT),
				x: 0.,
				y: 0.,
			});
		}
		let thread_keys: BTreeMap<_, _> =
			rows.iter().filter_map(|r| Some((r.thread.clone()?, r.key.clone()))).collect();
		self.add_assigned_relations(&mut graph, &included);
		self.add_native_ownership(&mut graph, &rows, &included, &thread_keys);
		self.add_context_relations(&mut graph, &included);
		self.add_native_calls(&mut graph, &rows, &included, &thread_keys);

		let spawned: BTreeSet<_> = graph
			.edges
			.iter()
			.filter(|e| e.kind == Kind::Spawned)
			.map(|e| (e.from.clone(), e.to.clone()))
			.collect();
		graph.edges.retain(|e| {
			e.kind != Kind::Assigned || !spawned.contains(&(e.from.clone(), e.to.clone()))
		});

		self.add_relation_resources(&rows, &included, &mut graph);

		self.place_relation_nodes(&mut graph);
		graph.paths =
			self.work_board.view.routing.borrow_mut().paths(&graph.nodes, &graph.connections());
		graph.scope_bounds(&snapshot.workspaces);
		graph
	}

	fn relation_scale(&self) -> f32 {
		self.work_board.view.zoom.unwrap_or(1.)
	}

	fn relation_controls(&self, cx: &mut Context<Self>) -> AnyElement {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let layout_viewport = self.work_board.view.layout_viewport;
		let cluster = || {
			gpui::div()
				.flex()
				.items_center()
				.h(gpui::px(26.))
				.rounded(gpui::px(7.))
				.px(gpui::px(2.))
				.gap(gpui::px(1.))
				.bg(gpui::rgba(0xffffff06))
		};
		gpui::div()
			.debug_selector(|| "relations-toolbar".into())
			.w_full()
			.h(gpui::px(38.))
			.px_2()
			.pr(gpui::px(68.))
			.flex_none()
			.flex()
			.items_center()
			.gap_2()
			.text_size(gpui::px(11.))
			.child(gpui::div().flex_1())
			.child(self.relation_selection_label(cx))
			.child(
				cluster()
					.child(self.graph_control(
						"arrange",
						crate::shell::workspace_symbols::icon_sized(
							crate::shell::workspace_symbols::Symbol::Arrange,
							14.,
						),
						"Restore automatic layout",
						|s, cx| {
							s.arrange_relations();
							cx.notify();
						},
						cx,
					))
					.child(self.graph_control(
						"fit",
						crate::shell::workspace_symbols::icon_sized(
							crate::shell::workspace_symbols::Symbol::Fit,
							14.,
						),
						"Fit all agents and workspace boundaries",
						|s, cx| {
							s.work_board.view.camera_fixed = true;
							s.fit_relations();
							cx.notify();
						},
						cx,
					))
					.child(self.relation_legend_toggle(cx)),
			)
			.child(
				cluster()
					.child(self.graph_control(
						"out",
						"−",
						"Zoom out",
						move |s, cx| {
							s.work_board.view.hold_camera(scale, pan, layout_viewport);
							s.zoom_relations(scale, scale / 1.2, None);
							cx.notify();
						},
						cx,
					))
					.child(self.graph_control(
						"reset",
						format!("{:.0}%", scale * 100.),
						"Reset zoom to 100%",
						move |s, cx| {
							s.work_board.view.hold_camera(scale, pan, layout_viewport);
							s.zoom_relations(scale, 1., None);
							cx.notify();
						},
						cx,
					))
					.child(self.graph_control(
						"in",
						"+",
						"Zoom in (maximum 100%)",
						move |s, cx| {
							s.work_board.view.hold_camera(scale, pan, layout_viewport);
							s.zoom_relations(scale, scale * 1.2, None);
							cx.notify();
						},
						cx,
					)),
			)
			.into_any_element()
	}

	fn graph_control(
		&self,
		id: &str,
		label: impl IntoElement,
		description: &str,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		let tip = description.to_owned();
		gpui::div()
			.id(SharedString::from(format!("relations-{id}")))
			.role(Role::Button)
			.aria_label(description.to_owned())
			.occlude()
			.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
			.tab_index(0)
			.h(gpui::px(24.))
			.min_w(gpui::px(24.))
			.when(id == "reset", |d| d.w(gpui::px(44.)).flex_none())
			.px(gpui::px(4.))
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(4.))
			.cursor_pointer()
			.hover(|d| d.bg(gpui::rgba(0xffffff10)))
			.tooltip(move |_, cx| cx.new(|_| RelationTip(tip.clone())).into())
			.child(label)
			.on_click(cx.listener(move |s, _, _, cx| action(s, cx)))
			.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					keyboard(s, cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	pub(super) fn relation_canvas(&self, graph: &Graph, cx: &mut Context<Self>) -> AnyElement {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let layout_viewport = self.work_board.view.layout_viewport;
		let hit_boxes: Vec<_> = graph
			.nodes
			.iter()
			.filter(|node| {
				node.row.is_some()
					|| self.work_board.view.visibility.borrow().node_opacity(graph, node) > 0.5
			})
			.map(|node| (node.key.clone(), node.screen_rect(scale, pan)))
			.collect();
		let mut canvas = gpui::div()
			.id("relations-canvas")
			.bg(gpui::rgba(crate::ui_theme::COLLABORATION_SURFACE))
			.debug_selector(|| "relations-canvas".into())
			.relative()
			.flex_1()
			.min_w_0()
			.min_h_0()
			.overflow_hidden()
			.on_scroll_wheel(cx.listener(move |s, e: &ScrollWheelEvent, _, cx| {
				let d = e.delta.pixel_delta(gpui::px(20.));
				let next = scale * (f32::from(d.y) * 0.003).exp();
				let center = (
					f32::from(e.position.x) - s.work_board.view.origin.0,
					f32::from(e.position.y) - s.work_board.view.origin.1,
				);
				s.work_board.view.hold_camera(scale, pan, layout_viewport);
				s.zoom_relations(scale, next, Some(center));
				cx.stop_propagation();
				cx.notify();
			}))
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(move |s, e: &MouseDownEvent, _, cx| {
					s.clear_relation_selection();
					cx.notify();
					s.work_board.view.hold_camera(scale, pan, layout_viewport);
					s.work_board.view.moved = false;
					s.work_board.view.drag =
						Some(Drag { key: None, start: e.position, last: e.position, origin: pan });
				}),
			)
			.on_mouse_move(cx.listener(move |s, e: &MouseMoveEvent, _, cx| {
				s.move_relation_pointer(e, &hit_boxes, cx)
			}))
			.on_hover(cx.listener(|s, inside, _, cx| {
				if !inside && s.work_board.view.hover.update(None, std::time::Instant::now()) {
					cx.notify();
				}
			}))
			.on_mouse_up(
				MouseButton::Left,
				cx.listener(|s, _, _, cx| {
					s.work_board.view.finish_drag();
					cx.notify();
				}),
			);
		canvas = self.relation_scope_layer(canvas, graph, scale, pan);
		canvas = self.relation_connection_layer(canvas, graph, cx);
		canvas = self.relation_node_layer(canvas, graph, cx);

		canvas = canvas.child(
			gpui::div()
				.absolute()
				.bottom(gpui::px(12.))
				.left(gpui::px(12.))
				.right(gpui::px(12.))
				.flex()
				.justify_end()
				.items_end()
				.gap(gpui::px(8.))
				.when(self.work_board.view.legend_visible, |d| d.child(self.relation_legend())),
		);
		gpui::div()
			.size_full()
			.flex()
			.flex_col()
			.child(self.relation_controls(cx))
			.child(canvas)
			.into_any_element()
	}

	fn move_relation_pointer(
		&mut self,
		e: &MouseMoveEvent,
		hit_boxes: &[(String, card::Rect)],
		cx: &mut Context<Self>,
	) {
		let scale = self.relation_scale();
		if e.pressed_button != Some(MouseButton::Left) {
			let dragging = self.work_board.view.drag.is_some();
			self.work_board.view.finish_drag();
			let view = &mut self.work_board.view;
			let x = f32::from(e.position.x) - view.origin.0;
			let y = f32::from(e.position.y) - view.origin.1;
			let now = std::time::Instant::now();
			let next = hit_boxes
				.iter()
				.rev()
				.find(|(_, bounds)| bounds.contains(x, y))
				.map(|(key, _)| key.clone());
			if view.hover.update(next, now) || dragging {
				cx.notify();
			}
			return;
		}
		if let Some(drag) = &mut self.work_board.view.drag {
			let from_start = e.position - drag.start;
			let (x, y) = (f32::from(from_start.x), f32::from(from_start.y));
			self.work_board.view.moved |= x * x + y * y >= 9.;
			if !self.work_board.view.moved {
				return;
			}
			let dx = f32::from(e.position.x - drag.last.x);
			let dy = f32::from(e.position.y - drag.last.y);
			if let Some(key) = &drag.key {
				drag.origin.0 += dx / scale;
				drag.origin.1 += dy / scale;
				self.work_board.view.positions.insert(key.clone(), drag.origin);
			} else {
				self.work_board.view.pan.0 += dx;
				self.work_board.view.pan.1 += dy;
			}
			drag.last = e.position;
			cx.notify();
		}
	}

	fn add_assigned_relations(&self, graph: &mut Graph, included: &BTreeSet<String>) {
		let Some(snapshot) = &self.snapshot else { return };
		for work in &snapshot.work_items {
			if let Some(parent) = &work.parent_goal_id
				&& included.contains(parent)
				&& included.contains(&work.id)
			{
				graph.edges.push(Edge {
					in_progress: false,
					id: format!("assigned:{}", work.id),
					from: parent.clone(),
					to: work.id.clone(),
					kind: Kind::Assigned,
					color: Kind::Assigned.color(),
					label: "Owner".into(),
					excerpt: None,
					detail: format!(
						"{}\n\nConversation owner: {}\nState: {}",
						self.work_label(work),
						parent,
						super::super::dock::progress_state(snapshot, work).label
					),
					link: None,
					source: work.codex_thread_id.clone().map(|t| (work.id.clone(), t)),
				});
			}
		}
	}

	fn add_native_ownership(
		&self,
		graph: &mut Graph,
		rows: &[Row],
		included: &BTreeSet<String>,
		thread_keys: &BTreeMap<String, String>,
	) {
		for agents in self.native_agents.lists.values() {
			for agent in agents {
				let (Some(from), Some(to)) =
					(thread_keys.get(&agent.parent_thread_id), thread_keys.get(&agent.thread_id))
				else {
					continue;
				};
				if !included.contains(from) || !included.contains(to) || from == to {
					continue;
				}
				let id = format!("spawn:{}", agent.thread_id);
				if graph.edges.iter().any(|e| e.id == id) {
					continue;
				}
				graph.edges.push(Edge {
					in_progress: false,
					id,
					from: from.clone(),
					to: to.clone(),
					kind: Kind::Spawned,
					color: Kind::Spawned.color(),
					label: "Started agent".into(),
					excerpt: (!agent.task.is_empty()).then(|| agent.task.clone()),
					detail: format!(
						"{}\n\nNative child state: {}\n\n{}",
						agent.title,
						agent.status,
						if agent.task.is_empty() {
							"No initial task text was returned."
						} else {
							&agent.task
						}
					),
					source: rows.iter().find(|row| &row.key == from).and_then(|row| {
						row.thread.as_ref().map(|thread| (row.work.clone(), thread.clone()))
					}),
					link: None,
				});
			}
		}
	}

	fn add_context_relations(&self, graph: &mut Graph, included: &BTreeSet<String>) {
		let Some(snapshot) = &self.snapshot else { return };
		for edge in &snapshot.dependencies {
			if included.contains(&edge.depends_on_id) && included.contains(&edge.work_item_id) {
				let blocked =
					snapshot.work_items.iter().find(|w| w.id == edge.work_item_id).is_some_and(
						|w| {
							super::super::graph::blockers(snapshot, w)
								.iter()
								.any(|b| b.id == edge.depends_on_id)
						},
					);
				graph.edges.push(Edge {
					in_progress: false,
					id: format!("dependency:{}:{}", edge.depends_on_id, edge.work_item_id),
					from: edge.depends_on_id.clone(),
					to: edge.work_item_id.clone(),
					kind: Kind::Dependency,
					excerpt: None,
					color: if blocked { AMBER } else { TEXT_MUTED },
					label: if blocked { "Waiting" } else { "Prerequisite met" }.into(),
					detail: if blocked {
						"This recorded prerequisite has not been resolved."
					} else {
						"This prerequisite is marked resolved. That does not prove the downstream task ran."
					}
					.into(),
					source: None,
					link: None,
				});
			}
		}
		for reference in &snapshot.context_references {
			if !included.contains(&reference.recipient_work_id) {
				continue;
			}
			let current = snapshot.work_items.iter().find(|w| w.id == reference.source_work_id);
			let key = if current
				.is_some_and(|w| w.codex_thread_id.as_ref() == Some(&reference.source_thread_id))
				&& included.contains(&reference.source_work_id)
			{
				reference.source_work_id.clone()
			} else {
				format!("context:{}:{}", reference.source_work_id, reference.source_thread_id)
			};
			if !graph.nodes.iter().any(|n| n.key == key) {
				graph.nodes.push(Node {
					key: key.clone(),
					title: current
						.map(|w| self.work_label(w))
						.unwrap_or_else(|| "Referenced conversation".into()),
					status: "Referenced thread".into(),
					color: Kind::Context.color(),
					row: None,
					size: (224., 48.),
					x: 0.,
					y: 0.,
				});
			}
			graph.edges.push(Edge { in_progress: false, id:format!("context:{}:{}:{}",reference.event_id,reference.recipient_work_id,reference.source_thread_id),from:key,to:reference.recipient_work_id.clone(),kind:Kind::Context,excerpt:None,color:Kind::Context.color(),label:if reference.delivery_turn_id.is_some(){"Shared context"}else{"Context queued"}.into(),detail:format!("Selected context reference\n\nSource thread: {}\nInput receipt: {}\nReceiving turn: {}\n\nDelivery makes the reference available; it does not prove it was read. No content version was recorded.",reference.source_thread_id,reference.event_id,reference.delivery_turn_id.as_deref().unwrap_or("Not delivered")),link:None,source:Some((reference.source_work_id.clone(),reference.source_thread_id.clone())) });
		}
	}

	fn add_native_calls(
		&self,
		graph: &mut Graph,
		rows: &[Row],
		included: &BTreeSet<String>,
		thread_keys: &BTreeMap<String, String>,
	) {
		// Only typed native calls establish communication; never search prose for thread IDs.
		for row in rows {
			let Some(brief) =
				self.work_board.briefs.get(&row.key).filter(|b| b.stamp == self.brief_stamp(row))
			else {
				continue;
			};
			let mut latest = BTreeMap::new();
			for (item, call) in &brief.relations {
				for receiver in &call.receiver_thread_ids {
					latest.insert(
						(call.sender_thread_id.clone(), receiver.clone(), call.tool.clone()),
						(item, call),
					);
				}
			}
			for ((sender, receiver, tool), (item, call)) in latest {
				let (Some(from), Some(to)) = (thread_keys.get(&sender), thread_keys.get(&receiver))
				else {
					continue;
				};
				if !included.contains(from) || !included.contains(to) {
					continue;
				}
				let kind = match tool.as_str() {
					"spawnAgent" | "subAgentActivity/started" => Kind::Spawned,
					"subAgentActivity/interacted" => Kind::Message,
					"subAgentActivity/completed" => Kind::Returned,
					"wait" => Kind::Wait,
					"sendInput" | "sendMessage" | "followupTask" => Kind::Message,
					_ => Kind::Control,
				};
				let task_excerpt = graph
					.edges
					.iter()
					.find(|e| e.kind == Kind::Spawned && e.from == *from && e.to == *to)
					.and_then(|e| e.excerpt.clone());
				if kind == Kind::Spawned {
					graph
						.edges
						.retain(|e| !(e.kind == Kind::Spawned && e.from == *from && e.to == *to));
				}
				let native_activity = tool.starts_with("subAgentActivity/");
				let label = match tool.as_str() {
					"subAgentActivity/started" => "Started agent".into(),
					"subAgentActivity/interacted" => "Interacted".into(),
					"subAgentActivity/completed" => "Finished".into(),
					"subAgentActivity/interrupted" => "Interrupted".into(),
					"spawnAgent" => "Started agent".into(),
					"sendInput" | "sendMessage" | "followupTask" => "Sent input".into(),
					"wait" => "Waited".into(),
					_ => format!("{} · {}", tool, call.status),
				};
				let detail = if native_activity {
					format!(
						"Native activity: {}\nSource item: {item}\n\nThe provider recorded this event but did not include the message body. This is historical evidence, not the agent's current state.",
						call.status
					)
				} else {
					format!(
						"Native tool: {tool}\nCall state: {}\nSource item: {item}\n\nPrompt excerpt:\n{}\n\nThis is the last observed tool call, not a claim about the recipient's current execution.",
						call.status, call.prompt
					)
				};
				let (from, to) =
					if native_activity && kind == Kind::Returned { (to, from) } else { (from, to) };
				graph.edges.push(Edge {
					in_progress: call.status == "inProgress",
					id: format!("call:{sender}:{item}:{receiver}"),
					from: from.clone(),
					to: to.clone(),
					kind,
					color: kind.color(),
					label,
					excerpt: if !call.prompt.is_empty() {
						Some(call.prompt.clone())
					} else if kind == Kind::Spawned {
						task_excerpt
					} else {
						None
					},
					detail,
					link: None,
					source: Some((row.work.clone(), sender.clone())),
				});
				if let Some(result) =
					call.results.iter().find(|r| r.thread_id == receiver && !r.message.is_empty())
				{
					graph.edges.push(Edge { in_progress: false,
						id: format!("reply:{sender}:{item}:{receiver}"),
						from: to.clone(),
						to: from.clone(),
						kind: Kind::Returned,
						excerpt: Some(result.message.clone()),
						color: Kind::Returned.color(),
						label: if result.status == "errored" {
							"Error observed"
						} else {
							"Result available"
						}
						.into(),
						detail: format!(
							"Observed by {tool}\nSource item: {item}\n\nReply excerpt:\n{}\n\nThis target-state snapshot can describe an earlier turn; it does not prove a reply to this call.",
							result.message
						),
						link: None,
					source: Some((row.work.clone(), sender.clone())),
					});
				}
			}
		}
	}

	fn place_relation_nodes(&self, graph: &mut Graph) {
		for node in &mut graph.nodes {
			node.size = (card::WIDTH, if node.row.is_some() { card::HEIGHT } else { 48. });
		}
		self.work_board.view.layout.borrow_mut().place(graph, self.work_board.view.layout_viewport);
		resources::place_resources(graph);
		for node in &mut graph.nodes {
			node.x = snap(node.x);
			node.y = snap(node.y);
			if let Some(&(x, y)) = self.work_board.view.positions.get(&node.key) {
				node.x = x;
				node.y = y;
			}
		}
	}

	fn relation_selection_label(&self, cx: &mut Context<Self>) -> AnyElement {
		let focused = self.work_board.focus.is_some() || self.work_board.view.edge.is_some();
		let label = self
			.work_board
			.focus
			.as_ref()
			.and_then(|key| self.board_rows().into_iter().find(|r| &r.key == key))
			.map(|r| compact_name(if r.native { &r.owner } else { &r.title }))
			.unwrap_or_else(|| if focused { "Connection".into() } else { String::new() });
		gpui::div()
			.max_w(gpui::px(120.))
			.min_w_0()
			.flex()
			.items_center()
			.gap_1()
			.child(
				gpui::div()
					.min_w_0()
					.text_ellipsis()
					.whitespace_nowrap()
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(label),
			)
			.when(focused, |d| {
				d.child(self.graph_control(
					"clear",
					"×",
					"Clear selection",
					|s, cx| {
						s.work_board.focus = None;
						s.work_board.view.edge = None;
						cx.notify();
					},
					cx,
				))
			})
			.into_any_element()
	}

	fn relation_scope_layer(
		&self,
		mut canvas: gpui::Stateful<gpui::Div>,
		graph: &Graph,
		scale: f32,
		pan: (f32, f32),
	) -> gpui::Stateful<gpui::Div> {
		for scope in &graph.scopes {
			let [left, top, right, bottom] = scope.bounds;
			canvas = canvas.child(
				gpui::div()
					.absolute()
					.left(gpui::px(left * scale + pan.0))
					.top(gpui::px(top * scale + pan.1))
					.w(gpui::px((right - left) * scale))
					.h(gpui::px((bottom - top) * scale))
					.rounded(gpui::px(18. * scale))
					.px(gpui::px(12. * scale))
					.pt(gpui::px(8. * scale))
					.text_size(gpui::px(11. * scale))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(
						gpui::div()
							.flex()
							.items_center()
							.gap(gpui::px(8. * scale))
							.child(
								gpui::div()
									.text_color(gpui::rgb(0xc5c6ce))
									.font_weight(gpui::FontWeight::MEDIUM)
									.child(scope.title.clone()),
							)
							.child(
								gpui::div()
									.text_size(gpui::px(10. * scale))
									.text_color(gpui::rgb(TEXT_MUTED))
									.child(format!("{} agents", scope.count)),
							),
					),
			);
		}

		canvas
	}

	fn relation_connection_layer(
		&self,
		mut canvas: gpui::Stateful<gpui::Div>,
		graph: &Graph,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let (lines, moving) = self.work_board.view.visibility.borrow_mut().sample(
			graph,
			self.relation_context(),
			self.work_board.view.edge.as_ref(),
			self.command_connection_ready(),
			std::time::Instant::now(),
		);
		canvas = self.paint_relation_connections(canvas, graph, &lines, moving, cx);
		for link in lines {
			if link.detail <= 0.001
				|| (link.fact.kind.hierarchy()
					&& !link.active
					&& self
						.work_board
						.view
						.edge
						.as_ref()
						.is_none_or(|edge| !edge.same_pair(&link.fact)))
			{
				continue;
			}
			let edge = link.fact;
			let (a, b, path) = (link.source, link.target, link.path);
			let label = graph.connection_label(&edge);
			let [left, top, _, _] = label.bounds(path.label);
			let picked = edge.clone();
			let pressed_edge = edge.clone();
			let keyboard_edge = edge.clone();
			let count = graph.edges.iter().filter(|other| edge.same_pair(other)).count();
			let description = if count > 1 {
				format!("{count} records: {} → {}", a.title, b.title)
			} else {
				format!("{}: {} → {}", label.text, a.title, b.title)
			};
			let name = format!("relation-edge-{}", edge.id);
			canvas = canvas.child(
				gpui::div()
					.id(SharedString::from(name.clone()))
					.debug_selector(move || name.clone())
					.role(Role::Button)
					.tab_index(0)
					.aria_label(description)
					.absolute()
					.left(gpui::px(left * scale + pan.0))
					.top(gpui::px(top * scale + pan.1))
					.w(gpui::px(label.width * scale))
					.h(gpui::px(style::ConnectionLabel::HEIGHT * scale))
					.flex()
					.items_center()
					.justify_center()
					.whitespace_nowrap()
					.rounded(gpui::px(3. * scale))
					.bg(gpui::rgba((crate::ui_theme::COMPOSER_MATERIAL & 0xffffff00) | 0xe8))
					.opacity(link.detail)
					.text_size(gpui::px(style::ConnectionLabel::FONT_SIZE * scale))
					.line_height(gpui::px(16. * scale))
					.text_color(gpui::rgb(edge.color))
					.cursor_pointer()
					.child(label.text)
					.on_mouse_down(
						MouseButton::Left,
						cx.listener(move |s, _, _, cx| {
							s.work_board.focus = None;
							s.handoffs.focus = None;
							s.work_board.view.show_record = false;
							s.work_board.view.camera_fixed = true;
							s.work_board.view.edge = Some(pressed_edge.clone());
							s.workspace.agent_tree_visible = true;
							cx.stop_propagation();
							cx.notify();
						}),
					)
					.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.work_board.focus = None;
							s.handoffs.focus = None;
							s.work_board.view.camera_fixed = true;
							s.work_board.view.edge = Some(keyboard_edge.clone());
							s.workspace.agent_tree_visible = true;
							cx.stop_propagation();
							cx.notify();
						}
					}))
					.on_click(cx.listener(move |s, _, _, cx| {
						s.work_board.focus = None;
						s.handoffs.focus = None;
						s.work_board.view.camera_fixed = true;
						s.work_board.view.picked_agent = None;
						s.work_board.view.edge = Some(picked.clone());
						s.workspace.agent_tree_visible = true;
						cx.notify();
					})),
			);
		}

		canvas
	}

	fn relation_node_layer(
		&self,
		mut canvas: gpui::Stateful<gpui::Div>,
		graph: &Graph,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let neighborhood = self.relation_neighborhood(graph);
		let layout_viewport = self.work_board.view.layout_viewport;
		let layout = card::Layout::new(scale);
		for node in &graph.nodes {
			let opacity = self.work_board.view.visibility.borrow().node_opacity(graph, node);
			if opacity <= 0.001 {
				continue;
			}
			let (selected, faded) = self.relation_node_emphasis(node, graph, &neighborhood);
			let key = node.key.clone();
			let row = node.row.clone();
			let target = graph
				.edges
				.iter()
				.find(|e| {
					(e.from == node.key && e.kind == Kind::Context)
						|| (e.to == node.key && e.kind == Kind::Resource)
				})
				.cloned();
			let keyboard_row = row.clone();
			let keyboard_target = target.clone();
			let dragkey = key.clone();
			let origin = (node.x, node.y);

			let view = self
				.relation_node_surface(node)
				.when(node.row.is_none(), |d| d.opacity(opacity))
				.on_mouse_down(
					MouseButton::Left,
					cx.listener(move |s, e: &MouseDownEvent, _, cx| {
						// Initial fitting can follow the frame that installed this handler.
						// Freeze the camera the user saw, rather than a pending fit.
						s.work_board.view.moved = false;
						s.work_board.view.hold_camera(scale, pan, layout_viewport);
						s.work_board.view.drag = Some(Drag {
							key: Some(dragkey.clone()),
							start: e.position,
							last: e.position,
							origin,
						});
						cx.stop_propagation();
					}),
				)
				.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
					if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
						if let Some(row) = &keyboard_row {
							s.select_workbench_agent(&row.key, cx);
						} else {
							s.work_board.focus = None;
							s.work_board.view.picked_agent = None;
							s.work_board.view.edge = keyboard_target.clone();
							s.workspace.agent_tree_visible = true;
							cx.notify();
						}
						cx.stop_propagation();
					}
				}))
				.on_click(cx.listener(move |s, event: &gpui::ClickEvent, _, cx| {
					if s.work_board.view.moved {
						return;
					}
					if let Some(row) = &row {
						if event.click_count() == 2 {
							s.open_graph_agent(row, cx);
						} else {
							s.select_workbench_agent(&row.key, cx);
						}
					} else {
						s.work_board.focus = None;
						s.work_board.view.picked_agent = None;
						s.work_board.view.edge = target.clone();
						s.workspace.agent_tree_visible = true;
						cx.notify();
					}
				}));
			canvas = canvas.child(card::Card {
				key: node.key.clone(),
				surface: view,
				content: self.relation_node_content(node, layout),
				emphasis: if self.relation_node_current(node) {
					1.
				} else if selected {
					0.45
				} else {
					0.
				},
				faded,
				visibility: opacity,
			});
		}
		canvas
	}

	fn update_relation_viewport(
		&mut self,
		bounds: gpui::Bounds<gpui::Pixels>,
		scale: f32,
		node_count: usize,
		cx: &mut Context<Self>,
	) {
		let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
		self.work_board.view.origin = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
		if self.workspace.panel_drag.is_some() {
			self.work_board.view.camera_fixed = true;
			self.work_board.view.zoom = Some(scale);
		}
		let resized = self.work_board.view.viewport != size;
		let reveal = resized && std::mem::take(&mut self.work_board.view.reveal_after_resize);
		if resized || self.work_board.view.node_count != node_count {
			self.work_board.view.node_count = node_count;
			self.work_board.view.viewport = size;
			if reveal {
				let graph = self.relation_graph();
				if let Some(node) = graph
					.nodes
					.iter()
					.find(|node| self.work_board.view.picked_agent.as_ref() == Some(&node.key))
				{
					self.work_board.view.pan = (
						size.0 / 2. - (node.x + node.width() / 2.) * scale,
						size.1 / 2. - (node.y + node.height() / 2.) * scale,
					);
					cx.notify();
				}
			} else if !self.work_board.view.camera_fixed {
				self.work_board.view.layout_viewport = size;
				self.frame_relations();
				cx.notify();
			}
		}
	}

	fn paint_relation_connections(
		&self,
		mut canvas: gpui::Stateful<gpui::Div>,
		graph: &Graph,
		lines: &[visibility::Link],
		moving: bool,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let flow_started = *self.work_board.view.flow_clock.get_or_init(std::time::Instant::now);
		let reduce_motion = crate::ui_motion::reduced() || !crate::ui_motion::animations_active();
		let paint_lines = lines.to_vec();
		let hover_pending = self.work_board.view.hover.pending(std::time::Instant::now());
		let entity = cx.entity().downgrade();
		let node_count = graph.nodes.len();
		canvas = canvas.child(
			gpui::canvas(
				move |bounds, _, cx| {
					let _ = entity.update(cx, |s, cx| {
						s.update_relation_viewport(bounds, scale, node_count, cx)
					});
				},
				move |bounds, _, window, cx| {
					let mut animate = moving || hover_pending;
					let mut base = style::StrokeBatch::default();
					let mut detail = style::StrokeBatch::default();
					let mut flows = Vec::new();
					let mut highlights = BTreeMap::<u32, routing::Ink>::new();
					paint_relation_grid(bounds, scale, pan, window);
					for link in &paint_lines {
						if link.base.kind.hierarchy() {
							let opacity = 1. - link.dim * 0.65 * (1. - link.detail);
							if let Ok(path) = link
								.path
								.stroke(link.base.kind, bounds.origin, scale, pan, 1.6, false)
								.build()
							{
								base.push(
									path,
									(link.base.color << 8) | (0xa0 as f32 * opacity) as u32,
								);
							}
						}
						if link.detail <= 0.001 {
							continue;
						}
						let reversed =
							(link.fact.from != link.base.from).then(|| link.path.reversed());
						let route = reversed.as_ref().unwrap_or(&link.path);
						let color =
							if link.active { crate::ui_theme::BLUE } else { link.fact.color };
						let mut ink = route.clone();
						if link.fact.kind.pattern().is_none() {
							highlights
								.entry((color << 8) | (link.detail * 255.) as u32)
								.or_default()
								.retain(&mut ink);
						}
						if let Ok(path) = ink
							.stroke(link.fact.kind, bounds.origin, scale, pan, 1.6, false)
							.build()
						{
							detail.push(path, (color << 8) | (0xdd as f32 * link.detail) as u32);
						}
						if link.active && !reduce_motion {
							flows.push((reversed, link.path.as_ref(), link.detail));
						}
					}
					// Keep one clean stroke; selection and flow add no outer halo.
					base.paint(window);
					detail.paint(window);
					let phase = (flow_started.elapsed().as_secs_f32() / 2.8).fract();
					for (reversed, forward, opacity) in flows {
						let route = reversed.as_ref().unwrap_or(forward);
						route.paint_flow(bounds.origin, scale, pan, phase, opacity, window);
						animate = true;
					}
					if animate {
						crate::ui_motion::request_frame(window, cx);
					}
				},
			)
			.absolute()
			.inset_0(),
		);
		canvas
	}

	fn relation_node_surface(&self, node: &Node) -> gpui::Stateful<gpui::Div> {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let layout = card::Layout::new(scale);
		let name = format!("relation-node-{}", node.key);
		let preview = node.row.as_ref().map(|row| self.graph_preview(row));
		gpui::div()
			.id(SharedString::from(name.clone()))
			.debug_selector(move || name.clone())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!(
				"{} · {} · {}",
				node.row.as_ref().map(|row| row.title.as_str()).unwrap_or(&node.title),
				node.status,
				preview
					.as_ref()
					.map(|(task, operation)| format!("{task} · {operation}"))
					.unwrap_or_default()
			))
			.absolute()
			.left(gpui::px(node.x * scale + pan.0))
			.top(gpui::px(node.y * scale + pan.1))
			.w(gpui::px(node.width() * scale))
			.h(gpui::px(node.height() * scale))
			.rounded(gpui::px(if node.key.starts_with("resource:") {
				3. * layout.chrome_scale
			} else {
				8. * layout.chrome_scale
			}))
			.border(gpui::px(0.75 * scale))
			.text_color(gpui::rgb(TEXT))
			.cursor_pointer()
			.overflow_hidden()
	}

	fn relation_node_content(&self, node: &Node, layout: card::Layout) -> AnyElement {
		let scale = self.relation_scale();
		let width = if node.row.is_some() { layout.width } else { node.width() * scale };
		let height = if node.row.is_some() { layout.height } else { node.height() * scale };
		let preview = node.row.as_ref().map(|row| {
			let (task, operation) = self.graph_preview(row);
			let text = if row.group == 1 { operation } else { self.work_activity_summary(row).1 };
			(task, text)
		});
		gpui::div()
			.w(gpui::px(width))
			.h(gpui::px(height))
			.px(gpui::px(layout.inset_x))
			.py(gpui::px(layout.inset_y))
			.flex()
			.flex_col()
			.justify_between()
			.gap(gpui::px(layout.gap))
			.when_some(preview, |view, (task, preview)| {
				view.child(header::Header {
					key: node.key.clone(),
					show_assignment: node.row.as_ref().is_some_and(|row| row.native),
					identity: node
						.row
						.as_ref()
						.map(|row| if row.native { row.owner.clone() } else { row.title.clone() })
						.unwrap_or_default(),
					task,
					preview: if preview == node.status { String::new() } else { preview },
					layout,
					color: node.color,
				})
			})
			.when(node.row.is_none(), |view| {
				view.child(
					gpui::div()
						.text_size(gpui::px(12. * scale))
						.line_height(gpui::px(14. * scale))
						.line_clamp(2)
						.text_ellipsis()
						.child(node.title.clone()),
				)
			})
			.when(node.row.is_some(), |view| view.child(self.node_metrics(node, layout)))
			.into_any_element()
	}

	fn relation_node_current(&self, node: &Node) -> bool {
		if let Some(picked) = &self.work_board.view.picked_agent {
			return picked == &node.key;
		}
		node.row.as_ref().is_some_and(|row| {
			if row.native {
				self.native_agents.selected.as_ref().is_some_and(|(owner, thread)| {
					owner == &row.work && row.thread.as_ref() == Some(thread)
				})
			} else {
				self.native_agents.selected.is_none() && self.selected.as_ref() == Some(&row.work)
			}
		})
	}

	fn relation_node_emphasis(
		&self,
		node: &Node,
		graph: &Graph,
		neighborhood: &BTreeSet<String>,
	) -> (bool, bool) {
		let linked = self.work_board.view.hover.current(std::time::Instant::now())
			== Some(node.key.as_str())
			|| self.work_board.view.picked_agent.as_ref() == Some(&node.key);
		let selected = linked
			|| self.work_board.view.edge.as_ref().is_some_and(|e| {
				e.from == node.key
					|| e.to == node.key
					|| (e.kind == Kind::Resource
						&& graph.edges.iter().any(|other| {
							other.kind == Kind::Resource
								&& other.to == e.to
								&& other.from == node.key
						}))
			})
			|| (self.work_board.focus.is_some() && neighborhood.contains(&node.key))
			|| (self.work_board.focus.is_none()
				&& self.work_board.view.edge.is_none()
				&& self.relation_node_current(node));
		let faded = self.relation_context().is_some() && !neighborhood.contains(&node.key)
			|| self.work_board.view.edge.as_ref().is_some_and(|e| {
				e.from != node.key
					&& e.to != node.key
					&& !(e.kind == Kind::Resource
						&& graph.edges.iter().any(|other| {
							other.kind == Kind::Resource
								&& other.to == e.to
								&& other.from == node.key
						}))
			});

		(selected, faded)
	}
}

fn paint_relation_grid(
	bounds: gpui::Bounds<gpui::Pixels>,
	scale: f32,
	pan: (f32, f32),
	window: &mut gpui::Window,
) {
	let spacing = grid_spacing(scale);
	// Grid dots never overlap. Give the whole background one draw order instead
	// of inserting thousands of independent bounds into the scene's order tree.
	window.paint_layer(bounds, |window| {
		let mut x = pan.0.rem_euclid(spacing);
		while x < f32::from(bounds.size.width) {
			let mut y = pan.1.rem_euclid(spacing);
			while y < f32::from(bounds.size.height) {
				window.paint_quad(gpui::fill(
					gpui::Bounds::new(
						bounds.origin + gpui::point(gpui::px(x), gpui::px(y)),
						gpui::size(gpui::px(1.), gpui::px(1.)),
					),
					gpui::rgba(0xffffff06),
				));
				y += spacing;
			}
			x += spacing;
		}
	});
}

impl View {
	fn hold_camera(&mut self, scale: f32, pan: (f32, f32), layout_viewport: (f32, f32)) {
		self.camera_fixed = true;
		self.zoom = Some(scale);
		self.pan = pan;
		self.layout_viewport = layout_viewport;
	}

	pub(super) fn keep_camera(&mut self) {
		self.camera_fixed |= self.zoom.is_some();
	}

	fn finish_drag(&mut self) {
		if let Some(Drag { key: Some(key), origin, .. }) = self.drag.take()
			&& self.moved
		{
			self.positions.insert(key, (snap(origin.0), snap(origin.1)));
		}
	}

	#[cfg(test)]
	fn zoom_at(&mut self, old: f32, next: f32) {
		let center = (self.viewport.0 / 2., self.viewport.1 / 2.);
		self.zoom_around(old, next, center);
	}

	fn zoom_around(&mut self, old: f32, next: f32, center: (f32, f32)) {
		let next = next.clamp(card::MIN_ZOOM, card::MAX_ZOOM);
		self.pan = (
			center.0 - (center.0 - self.pan.0) * next / old,
			center.1 - (center.1 - self.pan.1) * next / old,
		);
		self.zoom = Some(next);
	}
}
impl AgentSurface {
	pub(super) fn relation_context(&self) -> Option<&str> {
		self.work_board
			.view
			.hover
			.current(std::time::Instant::now())
			.or(self.work_board.view.picked_agent.as_deref())
			.or(self.work_board.focus.as_deref())
	}

	fn relation_neighborhood(&self, graph: &Graph) -> BTreeSet<String> {
		if let Some(key) = self.relation_context() {
			let mut keys = BTreeSet::from([key.to_owned()]);
			for edge in &graph.edges {
				if edge.from == key || edge.to == key {
					keys.insert(edge.from.clone());
					keys.insert(edge.to.clone());
				}
			}
			keys
		} else if let Some(picked) = &self.work_board.view.edge {
			let mut keys = BTreeSet::from([picked.from.clone(), picked.to.clone()]);
			if picked.kind == Kind::Resource {
				for edge in &graph.edges {
					if edge.kind == Kind::Resource && edge.to == picked.to {
						keys.insert(edge.from.clone());
					}
				}
			}
			keys
		} else {
			graph.nodes.iter().map(|n| n.key.clone()).collect()
		}
	}

	fn zoom_relations(&mut self, old: f32, next: f32, center: Option<(f32, f32)>) {
		let center = center.unwrap_or((
			self.work_board.view.viewport.0 / 2.,
			self.work_board.view.viewport.1 / 2.,
		));
		self.work_board.view.zoom_around(old, next, center);
	}

	fn arrange_relations(&mut self) {
		self.work_board.view.positions.clear();
		self.work_board.view.drag = None;
		self.work_board.view.moved = false;
		self.work_board.view.camera_fixed = true;
		self.work_board.view.layout_viewport = self.work_board.view.viewport;
		self.frame_relations();
	}

	pub(super) fn fit_relations(&mut self) {
		self.frame_relations();
	}

	fn frame_relations(&mut self) {
		// An explicit frame operation fits the current pane. Ordinary panel
		// resizing keeps the camera, and does not enter this path when fixed.
		self.work_board.view.layout_viewport = self.work_board.view.viewport;
		let graph = self.relation_graph();
		let keys = graph
			.nodes
			.iter()
			.filter(|node| node.row.is_some())
			.map(|node| node.key.clone())
			.collect();
		let Some([min_x, min_y, max_x, max_y]) = graph.layout_bounds(&keys) else {
			return;
		};
		let (w, h) = self.work_board.view.viewport;
		if w <= 48. || h <= 48. {
			return;
		}
		let width = (max_x - min_x).max(1.);
		let height = (max_y - min_y).max(1.);
		let top = FRAME_INSET;
		let usable_height = (h - 2. * FRAME_INSET).max(1.);
		// Fit visible cards and scope labels exactly. Hidden evidence routes do
		// not reserve another margin or force a root-only close-up.
		let low = ((w - 2. * FRAME_INSET) / width)
			.min(usable_height / height)
			.clamp(card::MIN_ZOOM, card::MAX_ZOOM);
		self.work_board.view.zoom = Some(low);
		self.work_board.view.pan = (
			(w - width * low) / 2. - min_x * low,
			top + (usable_height - height * low) / 2. - min_y * low,
		);
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(crate) fn visual_relation_resource_focus(&mut self) {
		self.work_board.view.edge =
			self.relation_graph().edges.into_iter().find(|e| e.kind == Kind::Resource);
	}

	pub(crate) fn visual_relation_evidence(&mut self) {
		let Some(snapshot) = &mut self.snapshot else { return };
		for work in &mut snapshot.work_items {
			work.codex_thread_id = Some(format!("fixture-{}", work.id));
		}
		snapshot.context_references = vec![decodex_protocol::AgentContextReferenceDto {
			event_id: 42,
			recipient_work_id: "verify".into(),
			source_work_id: "flow".into(),
			source_thread_id: "fixture-flow".into(),
			delivery_turn_id: Some("receiving-turn".into()),
		}];
		self.native_agents.lists.insert(
			"release".into(),
			vec![decodex_protocol::NativeAgentDto {
				thread_id: "fixture-reviewer".into(),
				parent_thread_id: "fixture-release".into(),
				title: "Security review".into(),
				task: "Review authentication changes".into(),
				status: "idle".into(),
			}],
		);
		let row = self
			.board_rows()
			.into_iter()
			.find(|r| r.key == "release")
			.expect("relation fixture owner");
		self.work_board.briefs.insert(row.key.clone(),Brief{output:None,source_thread:row.thread.clone(),assignments:Default::default(),outcome:None,preview:None,question:None,metrics:Default::default(),resources:None,stamp:self.brief_stamp(&row),read_at:std::time::Instant::now(),relations:vec![("review-wait".into(),decodex_protocol::AgentCollaborationDto {sender_thread_id:"fixture-release".into(),receiver_thread_ids:vec!["fixture-reviewer".into()],tool:"wait".into(),status:"completed".into(),prompt:String::new(),results:vec![decodex_protocol::AgentCollaborationResultDto{thread_id:"fixture-reviewer".into(),status:"completed".into(),message:"Cancellation can leave the sign-in button disabled. Add a regression test before release.".into()}]})]});
		for owner in ["flow", "verify"] {
			let row = self
				.board_rows()
				.into_iter()
				.find(|r| r.key == owner)
				.expect("resource fixture owner");
			let resource = decodex_protocol::AgentResourceDto {
                id: format!("pr-{owner}"), attachment_type: "pull_request".into(),
                identity_key: "example/release#42".into(),
                payload_json: r#"{"title":"PR #42 · Sign-in fix","url":"https://github.com/example/release/pull/42"}"#.into(),
                payload_omitted: false, created_at: 1791424800,
            };
			self.work_board.briefs.insert(
				row.key.clone(),
				Brief {
					output: None,
					source_thread: row.thread.clone(),
					assignments: Default::default(),
					outcome: None,
					preview: None,
					question: None,
					metrics: Default::default(),
					stamp: self.brief_stamp(&row),
					resources: Some(vec![resource]),
					read_at: std::time::Instant::now(),
					relations: vec![],
				},
			);
		}
	}
}

#[cfg(test)]
mod fit_tests {
	use super::*;
	#[gpui::test]
	fn relation_record_labels_scale_with_cards_and_stay_clear_of_them(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| super::super::tests::fixture(cx));
		visual.simulate_resize(gpui::size(gpui::px(1800.), gpui::px(1000.)));
		surface.update(visual, |s, _| {
			s.visual_relation_evidence();
			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;
			s.work_board.view.camera_fixed = true;
			s.work_board.view.picked_agent = Some("release".into());
			s.work_board.view.layout_viewport = (800., 900.);
		});
		for viewport in [(800., 900.), (1600., 500.)] {
			surface.update(visual, |s, _| s.work_board.view.layout_viewport = viewport);
			for scale in [1., 0.8, 0.5, 0.32, 0.15, 0.05, 0.5, 1.] {
				let (name, expected) = surface.update(visual, |s, cx| {
					s.work_board.view.zoom = Some(scale);
					let graph = s.relation_graph();
					let at = std::time::Instant::now() - std::time::Duration::from_millis(250);
					let mut cache = s.work_board.view.visibility.borrow_mut();
					cache.sample(&graph, Some("release"), None, false, at);
					let (links, _) = cache.sample(
						&graph,
						Some("release"),
						None,
						false,
						std::time::Instant::now(),
					);
					let link = links.iter().find(|link| link.base.kind == Kind::Spawned).unwrap();
					let label = graph.connection_label(&link.fact);
					assert_eq!(label.text, "3");
					let bounds = label.bounds(link.path.label);
					for node in &graph.nodes {
						assert!(
							bounds[2] <= node.x
								|| node.x + node.width() <= bounds[0]
								|| bounds[3] <= node.y
								|| node.y + node.height() <= bounds[1],
							"record label overlaps {} at {scale}: {bounds:?}",
							node.key
						);
					}
					s.work_board.view.pan =
						(400. - link.path.label.0 * scale, 300. - link.path.label.1 * scale);
					let name = "relation-edge-reply:fixture-release:review-wait:fixture-reviewer";
					assert_eq!(format!("relation-edge-{}", link.fact.id), name);
					drop(cache);
					cx.notify();
					(
						name,
						gpui::size(
							gpui::px(label.width * scale),
							gpui::px(style::ConnectionLabel::HEIGHT * scale),
						),
					)
				});
				visual.update(crate::ui_frame::draw_frame);
				let bounds = visual.debug_bounds(name).unwrap();
				assert!(
					f32::from(bounds.size.width - expected.width).abs() <= 0.5
						&& f32::from(bounds.size.height - expected.height).abs() <= 0.5,
					"label does not follow zoom {scale}: {:?} vs {expected:?}",
					bounds.size
				);
			}
		}
	}

	#[gpui::test]
	fn independent_trees_keep_their_lanes_across_summary_zoom(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, _| {
			let root = s.root_id().unwrap();
			s.snapshot.as_mut().unwrap().work_items.retain(|work| work.id != root);
			for work in &mut s.snapshot.as_mut().unwrap().work_items {
				work.parent_goal_id = None;
			}
			s.work_board.view.layout_viewport = (800., 240.);
			s.work_board.view.zoom = Some(1.);
			let graph = s.relation_graph();
			let children: BTreeSet<_> = graph
				.edges
				.iter()
				.filter(|edge| edge.kind.hierarchy())
				.map(|edge| edge.to.as_str())
				.collect();
			let roots: Vec<_> = graph
				.nodes
				.iter()
				.filter(|node| node.row.is_some() && !children.contains(node.key.as_str()))
				.map(|node| node.key.clone())
				.collect();
			let edge = graph.edges.iter().find(|edge| edge.kind == Kind::Spawned).unwrap();
			let parent = graph.nodes.iter().find(|node| node.key == edge.from).unwrap();
			let child = graph.nodes.iter().find(|node| node.key == edge.to).unwrap();
			let horizontal = child.x >= parent.x + parent.width();
			let lanes = |graph: &Graph| {
				roots
					.iter()
					.flat_map(|a| roots.iter().map(move |b| (a, b)))
					.map(|(a, b)| {
						let a = graph.nodes.iter().find(|node| &node.key == a).unwrap();
						let b = graph.nodes.iter().find(|node| &node.key == b).unwrap();
						if horizontal { a.x == b.x } else { a.y == b.y }
					})
					.collect::<Vec<_>>()
			};
			let original = lanes(&graph);
			let geometry = |graph: &Graph| {
				graph
					.nodes
					.iter()
					.map(|node| (node.key.clone(), node.x, node.y, node.size))
					.collect::<Vec<_>>()
			};
			let world = geometry(&graph);
			for zoom in [0.83, 0.65, 0.48, 0.33, 0.15, 0.05, 0.48, 1.] {
				s.work_board.view.zoom = Some(zoom);
				let graph = s.relation_graph();
				assert_eq!(lanes(&graph), original, "roots changed lanes at {zoom}");
				assert_eq!(geometry(&graph), world, "zoom changed world geometry at {zoom}");
			}
		});
	}

	#[gpui::test]
	fn summary_zoom_keeps_the_pointed_agent_stationary_and_returns_without_drift(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, _| {
			s.work_board.view.layout_viewport = (1200., 700.);
			s.work_board.view.viewport = (1200., 700.);
			s.work_board.view.zoom = Some(1.);
			s.work_board.view.pan = (45., -19.);
			let graph = s.relation_graph();
			let node = graph.nodes.iter().find(|node| node.key == "release").unwrap();
			let center = (node.x + node.width() / 2. + 45., node.y + node.height() / 2. - 19.);
			for scale in [0.8, 0.65, 0.48, 0.33, 0.15, 0.05, 0.33, 0.65, 1.] {
				let old = s.relation_scale();
				s.zoom_relations(old, scale, Some(center));
				let graph = s.relation_graph();
				let node = graph.nodes.iter().find(|node| node.key == "release").unwrap();
				let actual = (
					(node.x + node.width() / 2.) * scale + s.work_board.view.pan.0,
					(node.y + node.height() / 2.) * scale + s.work_board.view.pan.1,
				);
				assert!(
					(actual.0 - center.0).abs() < 0.01 && (actual.1 - center.1).abs() < 0.01,
					"anchor moved at {scale}: {actual:?} vs {center:?}"
				);
			}
			assert!((s.work_board.view.pan.0 - 45.).abs() < 0.01);
			assert!((s.work_board.view.pan.1 + 19.).abs() < 0.01);
		});
	}

	#[gpui::test]
	fn archived_coordinator_does_not_crop_the_remaining_forest(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, _| {
			let root = s.root_id().unwrap();
			s.snapshot.as_mut().unwrap().work_items.retain(|work| work.id != root);
			for work in &mut s.snapshot.as_mut().unwrap().work_items {
				work.parent_goal_id = None;
			}
			s.work_board.view.viewport = (800., 240.);
			s.frame_relations();
			let scale = s.work_board.view.zoom.unwrap();
			let pan = s.work_board.view.pan;
			let graph = s.relation_graph();
			assert!(!graph.nodes.is_empty());
			let camera = (s.work_board.view.zoom, s.work_board.view.pan);
			s.fit_relations();
			assert_eq!(camera, (s.work_board.view.zoom, s.work_board.view.pan));
			for node in &graph.nodes {
				assert!(
					node.x * scale + pan.0 >= FRAME_INSET - 0.01,
					"{} at {scale}, pan {pan:?}, size {:?}, position {:?}; graph {:?}",
					node.key,
					node.size,
					(node.x, node.y),
					graph.nodes.iter().map(|n| (&n.key, n.x, n.y, n.size)).collect::<Vec<_>>()
				);
				assert!(
					node.y * scale + pan.1 >= FRAME_INSET - 0.01,
					"{} at {scale}, pan {pan:?}, size {:?}, position {:?}; graph {:?}",
					node.key,
					node.size,
					(node.x, node.y),
					graph.nodes.iter().map(|n| (&n.key, n.x, n.y, n.size)).collect::<Vec<_>>()
				);
				assert!((node.x + node.width()) * scale + pan.0 <= 800. - FRAME_INSET + 0.01);
				assert!(
					(node.y + node.height()) * scale + pan.1 <= 240. - FRAME_INSET + 0.01,
					"{} at {scale}, pan {pan:?}, bottom {}; graph {:?}",
					node.key,
					(node.y + node.height()) * scale + pan.1,
					graph.nodes.iter().map(|n| (&n.key, n.x, n.y, n.size)).collect::<Vec<_>>()
				);
			}
		});
	}

	#[gpui::test]
	fn card_content_fits_across_zoom_levels_below_the_toolbar(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| super::super::tests::fixture(cx));
		surface.update(visual, |s, _| {
			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;
			s.work_board.view.camera_fixed = true;
			let row = s.board_rows().into_iter().find(|row| row.key == "release").unwrap();
			s.work_board.briefs.insert(
				row.key.clone(),
				Brief {
					output: None,
					source_thread: row.thread.clone(),
					assignments: Default::default(),
					outcome: None,
					preview: None,
					question: None,
					resources: None,
					relations: vec![],
					stamp: s.brief_stamp(&row),
					read_at: std::time::Instant::now(),
					metrics: crate::ui_work_metrics::Sample {
						tokens: Some(151744),
						stats: Some(decodex_protocol::AgentThreadMetrics {
							completed_ms: Some(123 * 3_600_000 + 59 * 60_000),
							active_ms: None,
							generating: false,
							rate: Some(decodex_protocol::AgentOutputRate {
								estimated: true,
								active: false,
								output_tokens: 795,
								duration_ms: 10000,
								started_at_ms: 0,
								completed_at_ms: Some(10000),
							}),
						}),
						..Default::default()
					},
				},
			);
		});
		let mut previous: Option<gpui::Size<gpui::Pixels>> = None;
		for scale in [
			1., 0.9, 0.81, 0.79, 0.7, 0.65, 0.6, 0.5, 0.47, 0.46, 0.4, 0.32, 0.3, 0.25, 0.2, 0.15,
			0.1, 0.05,
		] {
			surface.update(visual, |s, cx| {
				s.work_board.view.zoom = Some(scale);
				let graph = s.relation_graph();
				let node = graph.nodes.iter().find(|node| node.key == "release").unwrap();
				for a in graph.nodes.iter().filter(|node| node.row.is_some()) {
					assert_eq!(a.width(), card::WIDTH);
					assert_eq!(a.height(), card::HEIGHT);
					for b in
						graph.nodes.iter().filter(|node| node.row.is_some() && node.key != a.key)
					{
						assert!(
							a.x + a.width() <= b.x
								|| b.x + b.width() <= a.x
								|| a.y + a.height() <= b.y
								|| b.y + b.height() <= a.y,
							"overlapping cards at zoom {scale}: {} / {}",
							a.key,
							b.key
						);
					}
				}
				s.work_board.view.pan = (32. - node.x * scale, 32. - node.y * scale);
				cx.notify();
			});
			visual.update(crate::ui_frame::draw_frame);
			let card = visual.debug_bounds("relation-node-release").unwrap();
			if let Some(previous) = previous {
				assert!(
					card.size.width < previous.width && card.size.height <= previous.height,
					"card grew or stopped narrowing at {scale}: {:?} vs {previous:?}",
					card.size
				);
			}
			previous = Some(card.size);
			// The native renderer rounds bounds to the display's pixel grid.
			assert!((f32::from(card.size.width) - card::WIDTH * scale).abs() <= 0.5);
			assert!((f32::from(card.size.height) - card::HEIGHT * scale).abs() <= 0.5);
			let toolbar = visual.debug_bounds("relations-toolbar").unwrap();
			assert!(card.top() > toolbar.bottom());
			// At overview scale, verify the footprint. Subpixel text and insets
			// round to the display grid and are inspected after zooming back in.
			if scale < 0.15 {
				continue;
			}
			let layout = card::Layout::new(scale);
			assert_eq!(layout.text_scale, scale);
			let task = visual
				.debug_bounds("relation-task-release")
				.expect("work remains visible at every zoom");
			let metrics = visual.debug_bounds("relation-metrics-release").unwrap();
			let tokens = visual.debug_bounds("relation-tokens-release").unwrap();
			assert!(tokens.left() >= metrics.left() && tokens.right() <= metrics.right());
			assert!(tokens.top() >= metrics.top() && tokens.bottom() < metrics.bottom());
			assert!(task.left() > card.left() && task.right() < card.right());
			assert!(task.top() > card.top() && task.bottom() < metrics.top());
			if let Some(preview) = visual.debug_bounds("relation-preview-release") {
				let metrics = visual.debug_bounds("relation-metrics-release").unwrap();
				assert!(preview.left() > card.left() && preview.right() < card.right());
				assert!(metrics.left() > card.left() && metrics.right() < card.right());
				assert!(preview.bottom() < metrics.top(), "zoom {scale}: {preview:?}, {metrics:?}");
				assert!(metrics.bottom() < card.bottom(), "zoom {scale}: {metrics:?}, {card:?}");
			}
			let identity = visual.debug_bounds("relation-identity-release").unwrap();
			let metrics = visual.debug_bounds("relation-metrics-release").unwrap();
			assert!(
				identity.left() > card.left() && identity.right() < card.right(),
				"zoom {scale}: {identity:?}, {card:?}"
			);
			assert!((f32::from(metrics.size.height) - layout.metrics_height()).abs() <= 0.5);
			let time = visual.debug_bounds("relation-time-release").unwrap();
			assert!(time.left() >= metrics.left() && time.right() <= metrics.right());
			assert!(time.top() >= metrics.top() && time.bottom() <= metrics.bottom());
			assert!(metrics.top() > card.top() && metrics.bottom() < card.bottom());
		}
	}

	#[gpui::test]
	fn hovering_work_keeps_card_bounds_and_camera_unchanged(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| super::super::tests::fixture(cx));
		visual.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(900.)));
		surface.update(visual, |s, cx| {
			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;
			s.work_board.view.camera_fixed = true;
			s.work_board.view.zoom = Some(0.5);
			let graph = s.relation_graph();
			let node = graph.nodes.iter().find(|node| node.key == "release").unwrap();
			s.work_board.view.pan = (40. - node.x * 0.5, 80. - node.y * 0.5);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(250));
		visual.update(crate::ui_frame::draw_frame);
		let geometry = |s: &AgentSurface| {
			s.relation_graph()
				.nodes
				.into_iter()
				.map(|node| (node.key, node.x, node.y, node.size))
				.collect::<Vec<_>>()
		};
		let before = surface
			.read_with(visual, |s, _| (geometry(s), s.work_board.view.pan, s.work_board.view.zoom));
		let card = visual.debug_bounds("relation-node-release").unwrap();
		visual.simulate_mouse_move(card.center(), None, Default::default());
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(
			visual.debug_bounds("relation-node-release").unwrap().size,
			card.size,
			"passing over a node does not expand it immediately"
		);
		std::thread::sleep(std::time::Duration::from_millis(200));
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(230));
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("relation-node-release").unwrap(), card);
		assert!(visual.debug_bounds("relation-task-release-inspect").is_none());
		assert_eq!(
			surface.read_with(visual, |s, _| (
				geometry(s),
				s.work_board.view.pan,
				s.work_board.view.zoom
			)),
			before
		);
		visual.simulate_mouse_move(
			gpui::point(gpui::px(1.), gpui::px(1.)),
			None,
			Default::default(),
		);
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(140));
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(230));
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("relation-node-release").unwrap(), card);
		assert_eq!(
			surface.read_with(visual, |s, _| (
				geometry(s),
				s.work_board.view.pan,
				s.work_board.view.zoom
			)),
			before
		);
	}

	#[gpui::test]
	fn panel_resize_keeps_the_graph_camera_until_an_explicit_fit(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1_248.), gpui::px(840.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.work_board.graph = false;
			s.workspace.graph_visible = true;
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		std::thread::sleep(std::time::Duration::from_millis(250));
		visual.update(|w, cx| w.draw(cx).clear(cx));
		let camera =
			surface.read_with(visual, |s, _| (s.work_board.view.zoom, s.work_board.view.pan));
		assert!(camera.0.is_some());
		for height in [240., 380., 200.] {
			surface.update(visual, |s, cx| {
				s.workspace.panel_drag =
					Some((crate::shell::agent_surface::workspace_size::Panel::Bottom, 0., 0.));
				s.workspace.graph_panel_height = height;
				cx.notify();
			});
			visual.update(|w, cx| w.draw(cx).clear(cx));
			surface.read_with(visual, |s, _| {
				assert_eq!((s.work_board.view.zoom, s.work_board.view.pan), camera);
			});
		}
		surface.update(visual, |s, _| {
			s.workspace.panel_drag = None;
			s.fit_relations();
			assert_ne!((s.work_board.view.zoom, s.work_board.view.pan), camera);
		});
	}

	#[gpui::test]
	fn workbench_selection_preserves_camera_and_is_idempotent(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.work_board.view.pan = (80., -30.);
			s.work_board.view.zoom = Some(0.42);
			s.select_workbench_agent("agent", cx);
			assert_eq!(s.work_board.view.pan, (80., -30.));
			assert_eq!(s.work_board.view.zoom, Some(0.42));
			assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("agent"));
			s.select_workbench_agent("agent", cx);
			assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("agent"));
			s.clear_workbench_agent(cx);
			assert!(s.work_board.view.picked_agent.is_none());
		});
	}

	#[gpui::test]
	fn conversation_navigation_clears_a_previous_relation_pick_without_moving_the_camera(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.work_board.view.camera_fixed = true;
			s.work_board.view.pan = (80., -30.);
			s.work_board.view.zoom = Some(0.42);
			let row = s.factory_rows().into_iter().find(|row| row.key == "release").unwrap();
			s.open_graph_agent(&row, cx);
			assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("release"));
			s.open_page("agent", cx);
			assert!(s.work_board.view.picked_agent.is_none());
			let graph = s.relation_graph();
			assert!(s.relation_node_current(
				graph.nodes.iter().find(|node| node.key == "agent").unwrap()
			));
			assert!(!s.relation_node_current(
				graph.nodes.iter().find(|node| node.key == "release").unwrap()
			));
			assert_eq!(s.work_board.view.pan, (80., -30.));
			assert_eq!(s.work_board.view.zoom, Some(0.42));
			// Reopening the current conversation also releases metric-only selection.
			s.select_workbench_agent("release", cx);
			s.open_page("agent", cx);
			assert!(s.work_board.view.picked_agent.is_none());
		});
	}

	#[gpui::test]
	fn opening_an_agent_from_expanded_graph_reveals_it_without_reflowing_the_tree(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, cx| {
			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;
			s.work_board.view.camera_fixed = true;
			s.work_board.view.viewport = (1200., 900.);
			s.work_board.view.layout_viewport = (1200., 900.);
			s.work_board.view.zoom = Some(0.15);
			s.work_board.view.pan = (200., 500.);
			let row = s.factory_rows().into_iter().find(|row| row.key == "release").unwrap();
			s.open_graph_agent(&row, cx);
			assert!(!s.workspace.graph_expanded);
			let count = s.relation_graph().nodes.len();
			let positions = s
				.relation_graph()
				.nodes
				.into_iter()
				.map(|node| (node.key, node.x, node.y, node.size))
				.collect::<Vec<_>>();
			let expanded_camera = (s.work_board.view.zoom, s.work_board.view.pan);
			// The old scene can prepaint once before the restored panel is measured.
			s.update_relation_viewport(
				gpui::Bounds::new(
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(1200.), gpui::px(900.)),
				),
				0.15,
				count,
				cx,
			);
			assert_eq!(
				expanded_camera,
				(s.work_board.view.zoom, s.work_board.view.pan),
				"old bounds must not consume the pending reveal"
			);
			s.update_relation_viewport(
				gpui::Bounds::new(
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(800.), gpui::px(240.)),
				),
				0.15,
				count,
				cx,
			);
			let graph = s.relation_graph();
			assert_eq!(s.work_board.view.zoom, Some(0.15));
			assert_eq!(
				graph
					.nodes
					.iter()
					.map(|node| (node.key.clone(), node.x, node.y, node.size))
					.collect::<Vec<_>>(),
				positions
			);
			let node = graph.nodes.iter().find(|node| node.key == row.key).unwrap();
			let scale = s.relation_scale();
			let pan = s.work_board.view.pan;
			assert!(node.x * scale + pan.0 >= 0. && node.y * scale + pan.1 >= 0.);
			assert!((node.x + node.width()) * scale + pan.0 <= 800.);
			assert!((node.y + node.height()) * scale + pan.1 <= 240.);
			let camera = (s.work_board.view.zoom, pan);
			s.update_relation_viewport(
				gpui::Bounds::new(
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(800.), gpui::px(240.)),
				),
				scale,
				count,
				cx,
			);
			assert_eq!(
				camera,
				(s.work_board.view.zoom, s.work_board.view.pan),
				"fit is consumed once"
			);
		});
	}

	#[gpui::test]
	fn fit_ignores_selection_and_arrange_restores_a_readable_tree(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, _| {
			s.work_board.view.viewport = (900., 600.);
			s.fit_relations();
			let baseline = (s.work_board.view.zoom, s.work_board.view.pan);
			assert!(baseline.0.unwrap() <= 1.);
			let graph = s.relation_graph();
			s.work_board.focus = Some(graph.nodes[0].key.clone());
			s.fit_relations();
			assert_eq!(baseline, (s.work_board.view.zoom, s.work_board.view.pan));
			s.work_board.view.edge = graph.edges.first().cloned();
			s.work_board.focus = None;
			s.fit_relations();
			assert_eq!(baseline, (s.work_board.view.zoom, s.work_board.view.pan));
			s.work_board.view.positions.insert(graph.nodes[0].key.clone(), (9000., 8000.));
			s.fit_relations();
			assert_ne!(baseline, (s.work_board.view.zoom, s.work_board.view.pan));
			s.arrange_relations();
			assert!(s.work_board.view.positions.is_empty());
			assert_eq!(baseline, (s.work_board.view.zoom, s.work_board.view.pan));
			s.fit_relations();
			assert_eq!(baseline, (s.work_board.view.zoom, s.work_board.view.pan));
		});
	}

	#[gpui::test]
	fn initial_camera_fits_every_card_in_wide_shallow_and_narrow_panes(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, _| {
			for viewport in [(900., 300.), (680., 210.), (440., 600.)] {
				s.work_board.view.viewport = viewport;
				s.frame_relations();
				let scale = s.relation_scale();
				assert!((card::MIN_ZOOM..=card::MAX_ZOOM).contains(&scale));
				for node in s.relation_graph().nodes {
					let bounds = node.screen_rect(scale, s.work_board.view.pan);
					assert!(bounds.x >= FRAME_INSET - 1. && bounds.y >= FRAME_INSET - 1.);
					assert!(bounds.x + bounds.width <= viewport.0 - FRAME_INSET + 1.);
					assert!(bounds.y + bounds.height <= viewport.1 - FRAME_INSET + 1.);
				}
				let before = (s.work_board.view.zoom, s.work_board.view.pan);
				s.fit_relations();
				assert_eq!(before, (s.work_board.view.zoom, s.work_board.view.pan));
			}
		});
	}

	#[gpui::test]
	fn hidden_attachments_do_not_shrink_the_agent_overview(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_workbench_page("workbench-agent", cx);
			assert!(s.relation_graph().nodes.iter().any(|node| node.row.is_none()));
			s.work_board.view.viewport = (920., 680.);
			s.fit_relations();
			assert!(
				s.relation_scale() >= 0.85,
				"invisible attachments cannot reserve overview space"
			);
		});
	}

	#[gpui::test]
	fn only_running_assignment_targets_animate(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, _| {
			let graph = s.relation_graph();
			let mut edge = graph
				.edges
				.iter()
				.find(|e| matches!(e.kind, Kind::Assigned | Kind::Spawned))
				.unwrap()
				.clone();
			let mut target = graph.nodes.iter().find(|n| n.key == edge.to).unwrap().clone();
			for status in
				["Starting", "Approval", "Input needed", "Idle", "Marked complete", "Unknown"]
			{
				target.row.as_mut().unwrap().status = status.into();
				assert!(!edge.executing(&target, true), "{status}");
			}
			target.row.as_mut().unwrap().status = "Running".into();
			assert!(edge.executing(&target, true));
			assert!(!edge.executing(&target, false));
			for kind in [Kind::Returned, Kind::Context, Kind::Message, Kind::Wait, Kind::Resource] {
				edge.kind = kind;
				assert!(!edge.executing(&target, true));
			}
		});
	}

	#[gpui::test]
	fn workspace_layout_does_not_reserve_space_for_another_workspace(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, _| {
			let template = s.snapshot.as_ref().unwrap().work_items[0].clone();
			let work = |id: &str, parent: Option<&str>| {
				let mut item = template.clone();
				item.id = id.into();
				item.parent_goal_id = parent.map(str::to_owned);
				item.codex_thread_id = None;
				item
			};
			for count in [2, 7] {
				let snapshot = s.snapshot.as_mut().unwrap();
				snapshot.work_items = vec![
					work("root", None),
					work("a-reading", Some("root")),
					work("b-project", Some("root")),
					work("c-reading", Some("root")),
				];
				let mut project = vec!["b-project".into()];
				for i in 0..count {
					let id = format!("project-child-{i}");
					snapshot.work_items.push(work(&id, Some("b-project")));
					project.push(id);
				}
				snapshot.workspaces = vec![
					decodex_protocol::WorkspaceDto {
						id: "reading".into(),
						name: "Reading".into(),
						directory: "/tmp/reading".into(),
						work_ids: vec!["a-reading".into(), "c-reading".into()],
					},
					decodex_protocol::WorkspaceDto {
						id: "project".into(),
						name: "Project".into(),
						directory: "/tmp/project".into(),
						work_ids: project,
					},
				];
				let graph = s.relation_graph();
				let node = |key: &str| graph.nodes.iter().find(|n| n.key == key).unwrap();
				let first = node("a-reading");
				let last = node("c-reading");
				assert!(
					(last.x - first.x).abs() + (last.y - first.y).abs() <= 272.,
					"another workspace must not leave empty rows or columns"
				);
				assert_eq!(graph.nodes.len(), count + 4);
				assert_eq!(graph.edges.len(), count + 3);
			}
		});
	}

	#[gpui::test]
	fn global_coordinator_stays_above_workspaces_with_unassigned_children(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, _| {
			let snapshot = s.snapshot.as_mut().unwrap();
			let works = snapshot
				.work_items
				.iter()
				.filter(|w| w.id != "agent")
				.map(|w| w.id.clone())
				.collect();
			let mut unassigned = snapshot.work_items[0].clone();
			unassigned.id = "unassigned".into();
			unassigned.parent_goal_id = Some("agent".into());
			unassigned.codex_thread_id = None;
			snapshot.work_items.push(unassigned);
			snapshot.workspaces = vec![decodex_protocol::WorkspaceDto {
				id: "project".into(),
				name: "Project".into(),
				directory: "/tmp/project".into(),
				work_ids: works,
			}];
			s.work_board.view.layout_viewport = (1600., 600.);
			let graph = s.relation_graph();
			let root = graph.nodes.iter().find(|n| n.key == "agent").unwrap();
			let child = graph.nodes.iter().find(|n| n.key == "unassigned").unwrap();
			let project = graph.nodes.iter().find(|n| n.key == "release").unwrap();
			assert!(
				(root.y + root.height() < child.y && child.y == project.y)
					|| (root.x + root.width() < child.x && child.x == project.x)
			);
			assert_eq!(graph.scopes.iter().find(|scope| scope.id.is_none()).unwrap().count, 1);
		});
	}

	#[gpui::test]
	fn workspace_frames_preserve_global_graph_and_do_not_overlap(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::super::tests::fixture);
		surface.update(cx, |s, _| {
			let original = s.relation_graph();
			let works: Vec<_> =
				s.snapshot.as_ref().unwrap().work_items.iter().map(|w| w.id.clone()).collect();
			s.snapshot.as_mut().unwrap().workspaces = vec![decodex_protocol::WorkspaceDto {
				id: "scope-a".into(),
				name: "Workspace A".into(),
				directory: "/tmp/a".into(),
				work_ids: vec![works[0].clone()],
			}];
			s.workspace.workspace_filter = Some("scope-a".into());
			let graph = s.relation_graph();
			for node in &graph.nodes {
				for coordinate in [node.x, node.y, node.x + node.width(), node.y + node.height()] {
					assert_eq!(coordinate, snap(coordinate));
				}
			}
			for scope in &graph.scopes {
				for coordinate in scope.bounds {
					assert_eq!(coordinate, snap(coordinate));
				}
			}
			assert_eq!(graph.nodes.len(), original.nodes.len());
			assert_eq!(graph.edges.len(), original.edges.len());
			assert_eq!(graph.scopes.len(), 2);
			let a = &graph.scopes[0].bounds;
			let b = &graph.scopes[1].bounds;
			assert!(a[3] <= b[1] || b[3] <= a[1] || a[2] <= b[0] || b[2] <= a[0]);
			for scope in &graph.scopes {
				for node in graph
					.nodes
					.iter()
					.filter(|n| n.row.as_ref().is_some_and(|r| r.workspace == scope.id))
				{
					assert!(node.x >= scope.bounds[0] && node.x + node.width() <= scope.bounds[2]);
					assert!(
						node.y >= scope.bounds[1] + 30.
							&& node.y + node.height() <= scope.bounds[3]
					);
				}
			}
			s.work_board.view.viewport = (1200., 900.);
			s.fit_relations();
			let graph = s.relation_graph();
			let zoom = s.work_board.view.zoom.unwrap();
			let pan = s.work_board.view.pan;
			for scope in &graph.scopes {
				assert!(scope.bounds[0] * zoom + pan.0 >= 0.);
				assert!(scope.bounds[1] * zoom + pan.1 >= 0.);
				assert!(scope.bounds[2] * zoom + pan.0 <= 1200.);
				assert!(scope.bounds[3] * zoom + pan.1 <= 900.);
			}
		});
	}

	#[test]
	fn releasing_a_node_snaps_world_coordinates_without_snapping_the_camera() {
		let mut view = View { pan: (3.5, -7.25), moved: true, ..Default::default() };
		view.drag = Some(Drag {
			key: Some("node".into()),
			start: gpui::point(gpui::px(0.), gpui::px(0.)),
			last: gpui::point(gpui::px(0.), gpui::px(0.)),
			origin: (37., -21.),
		});
		view.finish_drag();
		assert_eq!(view.positions["node"], (32., -16.));
		assert_eq!(view.pan, (3.5, -7.25));
		assert!(view.drag.is_none());
		for scale in [0.15, 0.33, 0.5, 0.77, 1.] {
			let stride = grid_spacing(scale) / (GRID * scale);
			assert_eq!(stride, stride.round());
			assert!(grid_spacing(scale) >= 8.);
		}
	}

	#[test]
	fn zoom_keeps_the_graph_point_under_the_pointer() {
		let mut view = View { pan: (40., -20.), ..Default::default() };
		let pointer = (200., 120.);
		let point = ((pointer.0 - view.pan.0) / 0.5, (pointer.1 - view.pan.1) / 0.5);
		view.zoom_around(0.5, 0.8, pointer);
		assert!((point.0 * 0.8 + view.pan.0 - pointer.0).abs() < 0.001);
		assert!((point.1 * 0.8 + view.pan.1 - pointer.1).abs() < 0.001);
	}

	#[test]
	fn zoom_limits_preserve_the_anchor_at_overview_and_normal_size() {
		let mut view = View { pan: (40., -20.), ..Default::default() };
		view.zoom_around(0.5, 4., (200., 120.));
		assert_eq!(view.zoom, Some(card::MAX_ZOOM));
		assert_eq!(view.pan, (-120., -160.));
		let pan = view.pan;
		view.zoom_at(card::MAX_ZOOM, 4.);
		assert_eq!(view.zoom, Some(card::MAX_ZOOM));
		assert_eq!(view.pan, pan);
		view.zoom_around(card::MAX_ZOOM, 0.01, (200., 120.));
		assert_eq!(view.zoom, Some(card::MIN_ZOOM));
		assert!((320. * card::MIN_ZOOM + view.pan.0 - 200.).abs() < 0.001);
		assert!((280. * card::MIN_ZOOM + view.pan.1 - 120.).abs() < 0.001);
	}

	#[test]
	fn reverse_flow_follows_the_same_curve_in_the_opposite_direction() {
		let route = Route::curve((0., 0.), (70., -40.), (150., 120.), (200., 80.));
		let reversed = route.reversed();
		for i in 0..=20 {
			let t = i as f32 / 20.;
			let (x, y) = route.at(t);
			let (rx, ry) = reversed.at(1. - t);
			assert!((x - rx).abs() < 0.001 && (y - ry).abs() < 0.001);
		}
	}

	#[test]
	fn content_bounds_use_card_size_instead_of_reserved_empty_space() {
		let node = Node {
			key: "a".into(),
			title: "A".into(),
			status: String::new(),
			color: 0,
			row: None,
			size: (224., 48.),
			x: 120.,
			y: 80.,
		};
		let graph =
			Graph { scopes: vec![], nodes: vec![node], edges: vec![], paths: BTreeMap::new() };
		assert_eq!(
			graph.content_bounds(&BTreeSet::from(["a".into()])),
			Some([120., 80., 344., 128.])
		);
	}

	#[test]
	fn aligned_agent_connections_use_facing_ports_without_hooks() {
		let a = Node {
			key: "hubble".into(),
			title: "Hubble".into(),
			status: String::new(),
			color: 0,
			row: None,
			size: (224., 48.),
			x: 80.,
			y: 64.,
		};
		for (x, y) in [(80., 176.), (80., -48.), (400., 64.), (-240., 64.)] {
			let b = Node { key: "kant".into(), title: "Kant".into(), x, y, ..a.clone() };
			let path = route(&a, &b);
			let start = path.curves[0].start;
			let end = path.curves[0].end;
			let reverse = route(&b, &a);
			for i in 0..=40 {
				let t = i as f32 / 40.;
				let (px, py) = path.at(t);
				let (rx, ry) = reverse.at(1. - t);
				assert!((px - rx).abs() < 0.001 && (py - ry).abs() < 0.001);
				assert!(px >= start.0.min(end.0) - 0.001 && px <= start.0.max(end.0) + 0.001);
				assert!(py >= start.1.min(end.1) - 0.001 && py <= start.1.max(end.1) + 0.001);
			}
		}
	}
}
