//! Agent topology: typed connections are inspectable evidence, never inferred from message prose.
use super::*;
use crate::ui_theme::{CANVAS, TEXT};
use gpui::AppContext;
#[path = "agent_relation_details.rs"] mod details;
#[path = "agent_relation_metrics.rs"] pub(super) mod metrics;
#[path = "agent_relation_resources.rs"] mod resources;
#[path = "agent_relation_scopes.rs"] mod scopes;
#[path = "agent_relation_style.rs"] mod style;
use gpui::{
	MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Role, ScrollWheelEvent, SharedString,
	StatefulInteractiveElement,
};

const GRID: f32 = 16.;
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
	flow_clock: std::cell::OnceCell<std::time::Instant>,
	positions: BTreeMap<String, (f32, f32)>,
	pan: (f32, f32),
	zoom: Option<f32>,
	viewport: (f32, f32),
	origin: (f32, f32),
	node_count: usize,
	drag: Option<Drag>,
	moved: bool,
	pub(super) edge: Option<Edge>,
	pub(super) show_record: bool,
}
struct Drag {
	key: Option<String>,
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
	fn color(self) -> u32 {
		match self {
			Self::Assigned => 0x8b9fff,
			Self::Spawned => crate::ui_theme::BLUE,
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
			&& matches!(self.kind, Kind::Assigned | Kind::Spawned)
			&& self.to == target.key
			&& target.running()
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
	fn running(&self) -> bool {
		self.row.as_ref().is_some_and(|r| r.status == "Running")
	}

	fn task(&self) -> Option<&str> {
		self.row.as_ref().filter(|r| r.native && r.title != self.title).map(|r| r.title.as_str())
	}

	fn height(&self) -> f32 {
		if self.row.is_some() { if self.task().is_some() { 80. } else { 64. } } else { 48. }
	}
}
struct RelationTip(String);
impl gpui::Render for RelationTip {
	fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.p_2()
			.max_w(gpui::px(360.))
			.rounded_md()
			.bg(gpui::rgb(CANVAS))
			.text_color(gpui::rgb(TEXT))
			.text_size(gpui::px(12.))
			.child(self.0.clone())
	}
}
pub(super) struct Graph {
	scopes: Vec<scopes::Scope>,
	pub(super) nodes: Vec<Node>,
	pub(super) edges: Vec<Edge>,
	width: f32,
	height: f32,
}
impl Graph {
	fn connections(&self) -> Vec<Edge> {
		let mut connections: Vec<Edge> = Vec::new();
		for edge in &self.edges {
			if !connections.iter().any(|existing| existing.same_pair(edge)) {
				connections.push(edge.clone());
			}
		}
		connections
	}

	fn content_bounds(&self, keys: &BTreeSet<String>) -> Option<[f32; 4]> {
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
			include(node.x + 224., node.y + node.height());
		}
		for scope in &self.scopes {
			include(scope.bounds[0], scope.bounds[1]);
			include(scope.bounds[2], scope.bounds[3]);
		}
		for edge in self.connections() {
			if !keys.contains(&edge.from) || !keys.contains(&edge.to) {
				continue;
			}
			let Some(a) = self.nodes.iter().find(|n| n.key == edge.from) else {
				continue;
			};
			let Some(b) = self.nodes.iter().find(|n| n.key == edge.to) else {
				continue;
			};
			let path = route(a, b, edge.kind);
			for i in 0..=160 {
				let (x, y) = path.at(i as f32 / 160.);
				include(x, y);
			}
			let count = self.edges.iter().filter(|other| edge.same_pair(other)).count();
			let label = if count > 1 { format!("{count} records") } else { edge.label.clone() };
			include(path.label.0 - 48., path.label.1 - 10.);
			include(path.label.0 - 48. + label.len() as f32 * 6. + 16., path.label.1 + 14.);
		}
		bounds
	}

	pub(super) fn agent_count(&self) -> usize {
		self.nodes.iter().filter(|n| n.row.is_some()).count()
	}
}

impl AgentSurface {
	pub(super) fn relation_graph(&self) -> Graph {
		let mut graph =
			Graph { scopes: vec![], nodes: vec![], edges: vec![], width: 260., height: 120. };
		let Some(snapshot) = &self.snapshot else { return graph };
		let rows = self.board_rows();
		let included: BTreeSet<String> = rows.iter().map(|r| r.key.clone()).collect();
		for row in rows.iter().filter(|r| included.contains(&r.key)) {
			graph.nodes.push(Node {
				key: row.key.clone(),
				title: compact_name(if row.native { &row.owner } else { &row.title }),
				status: row.status.clone(),
				color: row.color,
				row: Some(row.clone()),
				x: 0.,
				y: 0.,
			});
		}
		let thread_keys: BTreeMap<_, _> =
			rows.iter().filter_map(|r| Some((r.thread.clone()?, r.key.clone()))).collect();
		for work in &snapshot.work_items {
			if let Some(parent) = &work.parent_goal_id
				&& included.contains(parent)
				&& included.contains(&work.id)
			{
				graph.edges.push(Edge {
					id: format!("assigned:{}", work.id),
					from: parent.clone(),
					to: work.id.clone(),
					kind: Kind::Assigned,
					color: Kind::Assigned.color(),
					label: "Delegated".into(),
					excerpt: None,
					detail: format!(
						"{}\n\nRecorded parent task: {}\nState: {}",
						self.work_label(work),
						parent,
						super::super::dock::progress_state(snapshot, work).label
					),
					link: None,
					source: work.codex_thread_id.clone().map(|t| (work.id.clone(), t)),
				});
			}
		}
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
					x: 0.,
					y: 0.,
				});
			}
			graph.edges.push(Edge { id:format!("context:{}:{}:{}",reference.event_id,reference.recipient_work_id,reference.source_thread_id),from:key,to:reference.recipient_work_id.clone(),kind:Kind::Context,excerpt:None,color:Kind::Context.color(),label:if reference.delivery_turn_id.is_some(){"Shared context"}else{"Context queued"}.into(),detail:format!("Selected context reference\n\nSource thread: {}\nInput receipt: {}\nReceiving turn: {}\n\nDelivery makes the reference available; it does not prove it was read. No content version was recorded.",reference.source_thread_id,reference.event_id,reference.delivery_turn_id.as_deref().unwrap_or("Not delivered")),link:None,source:Some((reference.source_work_id.clone(),reference.source_thread_id.clone())) });
		}
		// Only typed native calls establish communication; never search prose for thread IDs.
		for row in &rows {
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
					graph.edges.push(Edge {
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

		// A stable ownership layout seeds positions; dependency and context edges may form cycles.
		let mut placed = BTreeSet::new();
		let mut cursor = 0.;
		fn place(
			key: &str,
			depth: usize,
			g: &mut Graph,
			placed: &mut BTreeSet<String>,
			cursor: &mut f32,
		) -> f32 {
			if !placed.insert(key.into()) {
				return *cursor;
			}
			let mut children: Vec<_> = g
				.edges
				.iter()
				.filter(|e| e.from == key && matches!(e.kind, Kind::Assigned | Kind::Spawned))
				.map(|e| e.to.clone())
				.collect();
			children.sort();
			children.dedup();
			let y = if children.is_empty() {
				let y = *cursor;
				*cursor += 96.;
				y
			} else {
				let ys: Vec<_> = children
					.iter()
					.filter(|c| !placed.contains(*c))
					.cloned()
					.collect::<Vec<_>>()
					.iter()
					.map(|c| place(c, depth + 1, g, placed, cursor))
					.collect();
				if ys.is_empty() { *cursor } else { (ys[0] + ys[ys.len() - 1]) / 2. }
			};
			if let Some(n) = g.nodes.iter_mut().find(|n| n.key == key) {
				n.x = 32. + depth as f32 * 352.;
				n.y = 32. + y;
			}
			y
		}
		let roots: Vec<_> = graph
			.nodes
			.iter()
			.filter(|n| {
				!n.key.starts_with("resource:")
					&& !graph
						.edges
						.iter()
						.any(|e| e.to == n.key && matches!(e.kind, Kind::Assigned | Kind::Spawned))
			})
			.map(|n| n.key.clone())
			.collect();
		for root in roots {
			place(&root, 0, &mut graph, &mut placed, &mut cursor);
		}
		for key in graph.nodes.iter().map(|n| n.key.clone()).collect::<Vec<_>>() {
			if !key.starts_with("resource:") && !placed.contains(&key) {
				place(&key, 0, &mut graph, &mut placed, &mut cursor);
			}
		}
		graph.pack_workspaces();
		resources::place_resources(&mut graph);
		for node in &mut graph.nodes {
			node.x = snap(node.x);
			node.y = snap(node.y);
			if let Some(&(x, y)) = self.work_board.view.positions.get(&node.key) {
				node.x = x;
				node.y = y;
			}
			graph.width = graph.width.max(node.x + 260.);
			graph.height = graph.height.max(node.y + 116.);
		}
		graph.scope_bounds(&snapshot.workspaces);
		graph.width += 180.;
		graph
	}

	fn relation_scale(&self, graph: &Graph) -> f32 {
		let (w, h) = self.work_board.view.viewport;
		self.work_board.view.zoom.unwrap_or_else(|| {
			if w > 0. && h > 0. {
				((w - 32.) / graph.width).min((h - 24.) / graph.height).clamp(0.2, 1.)
			} else {
				0.7
			}
		})
	}

	pub(super) fn relation_controls(&self, cx: &mut Context<Self>) -> AnyElement {
		let scale = self.relation_scale(&self.relation_graph());
		let focused = self.work_board.focus.is_some() || self.work_board.view.edge.is_some();
		let label = self
			.work_board
			.focus
			.as_ref()
			.and_then(|key| self.board_rows().into_iter().find(|r| &r.key == key))
			.map(|r| compact_name(if r.native { &r.owner } else { &r.title }))
			.unwrap_or_else(|| if focused { "Connection".into() } else { String::new() });
		let cluster = || {
			gpui::div()
				.flex()
				.items_center()
				.gap(gpui::px(2.))
				.px(gpui::px(2.))
				.h(gpui::px(28.))
				.rounded(gpui::px(6.))
				.border_1()
				.border_color(gpui::rgba(0xffffff12))
				.bg(gpui::rgba(0x00000016))
		};
		gpui::div()
			.h(gpui::px(36.))
			.px_2()
			.flex_none()
			.flex()
			.items_center()
			.gap_2()
			.text_size(gpui::px(11.))
			.child(
				gpui::div()
					.flex_1()
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
					}),
			)
			.child(
				cluster()
					.child(self.graph_control(
						"arrange",
						"Arrange",
						"Restore automatic layout",
						|s, cx| {
							s.arrange_relations();
							cx.notify();
						},
						cx,
					))
					.child(self.graph_control(
						"fit",
						"Fit",
						"Fit all agents and workspace boundaries",
						|s, cx| {
							s.fit_relations();
							cx.notify();
						},
						cx,
					)),
			)
			.child(
				cluster()
					.child(self.graph_control(
						"out",
						"−",
						"Zoom out",
						move |s, cx| {
							s.work_board.view.camera_fixed = true;
							s.work_board.view.zoom_at(scale, (scale / 1.2).max(0.15));
							cx.notify();
						},
						cx,
					))
					.child(self.graph_control(
						"reset",
						&format!("{:.0}%", scale * 100.),
						"Reset zoom to 100%",
						move |s, cx| {
							s.work_board.view.camera_fixed = true;
							s.work_board.view.zoom_at(scale, 1.);
							cx.notify();
						},
						cx,
					))
					.child(self.graph_control(
						"in",
						"+",
						"Zoom in (maximum 100%)",
						move |s, cx| {
							s.work_board.view.camera_fixed = true;
							s.work_board.view.zoom_at(scale, scale * 1.2);
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
		label: &str,
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
			.tab_index(0)
			.h(gpui::px(24.))
			.min_w(gpui::px(24.))
			.px(gpui::px(7.))
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(4.))
			.cursor_pointer()
			.hover(|d| d.bg(gpui::rgba(0xffffff10)))
			.tooltip(move |_, cx| cx.new(|_| RelationTip(tip.clone())).into())
			.child(label.to_owned())
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
		let scale = self.relation_scale(graph);
		let pan = self.work_board.view.pan;
		let mut canvas = gpui::div()
			.id("relations-canvas")
			.debug_selector(|| "relations-canvas".into())
			.relative()
			.flex_1()
			.min_w_0()
			.min_h_0()
			.overflow_hidden()
			.on_scroll_wheel(cx.listener(move |s, e: &ScrollWheelEvent, _, cx| {
				let d = e.delta.pixel_delta(gpui::px(20.));
				let next = (scale * (f32::from(d.y) * 0.003).exp()).max(0.15);
				let center = (
					f32::from(e.position.x) - s.work_board.view.origin.0,
					f32::from(e.position.y) - s.work_board.view.origin.1,
				);
				s.work_board.view.camera_fixed = true;
				s.work_board.view.zoom_around(scale, next, center);
				cx.stop_propagation();
				cx.notify();
			}))
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(move |s, e: &MouseDownEvent, _, _| {
					s.work_board.view.camera_fixed = true;
					s.work_board.view.zoom = Some(scale);
					s.work_board.view.moved = false;
					s.work_board.view.drag =
						Some(Drag { key: None, last: e.position, origin: s.work_board.view.pan });
				}),
			)
			.on_mouse_move(cx.listener(move |s, e: &MouseMoveEvent, _, cx| {
				if e.pressed_button != Some(MouseButton::Left) {
					s.work_board.view.finish_drag();
					cx.notify();
					return;
				}
				if let Some(drag) = &mut s.work_board.view.drag {
					let dx = f32::from(e.position.x - drag.last.x);
					let dy = f32::from(e.position.y - drag.last.y);
					if dx.abs() + dy.abs() > 1. {
						s.work_board.view.moved = true;
					}
					if let Some(key) = &drag.key {
						drag.origin.0 += dx / scale;
						drag.origin.1 += dy / scale;
						s.work_board.view.positions.insert(key.clone(), drag.origin);
					} else {
						s.work_board.view.pan.0 += dx;
						s.work_board.view.pan.1 += dy;
					}
					drag.last = e.position;
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
		for scope in &graph.scopes {
			let [left, top, right, bottom] = scope.bounds;
			canvas = canvas.child(
				gpui::div()
					.absolute()
					.left(gpui::px(left * scale + pan.0))
					.top(gpui::px(top * scale + pan.1))
					.w(gpui::px((right - left) * scale))
					.h(gpui::px((bottom - top) * scale))
					.children((0..4).map(|corner| {
						let line = gpui::px(1.5 * scale);
						gpui::div()
							.absolute()
							.w(gpui::px(GRID * scale))
							.h(gpui::px(GRID * scale))
							.border_color(gpui::rgba(0xb0bac7a0))
							.when(corner < 2, |d| d.top_0().border_t(line))
							.when(corner >= 2, |d| d.bottom_0().border_b(line))
							.when(corner % 2 == 0, |d| d.left_0().border_l(line))
							.when(corner % 2 == 1, |d| d.right_0().border_r(line))
					}))
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
		let connections = graph.connections();
		let lines: Vec<_> = connections
			.iter()
			.filter_map(|e| {
				Some((
					e.clone(),
					graph.nodes.iter().find(|n| n.key == e.from)?.clone(),
					graph.nodes.iter().find(|n| n.key == e.to)?.clone(),
				))
			})
			.collect();
		let flow_started = *self.work_board.view.flow_clock.get_or_init(std::time::Instant::now);
		let reduce_motion = crate::ui_motion::reduced();
		let connected = self.command_connection_ready();
		let paint_lines = lines.clone();
		let selected_edge = self.work_board.view.edge.clone();
		let focused_agent = self.work_board.focus.clone();
		let entity = cx.entity().downgrade();
		let node_count = graph.nodes.len();
		canvas = canvas.child(
			gpui::canvas(
				move |bounds, _, cx| {
					let _ = entity.update(cx, |s, cx| {
						let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
						s.work_board.view.origin =
							(f32::from(bounds.origin.x), f32::from(bounds.origin.y));
						if s.work_board.view.viewport != size
							|| s.work_board.view.node_count != node_count
						{
							s.work_board.view.node_count = node_count;
							s.work_board.view.viewport = size;
							if !s.work_board.view.camera_fixed {
								s.fit_relations();
							}
							cx.notify();
						}
					});
				},
				move |bounds, _, window, cx| {
					let mut animate = false;
					let spacing = grid_spacing(scale);
					let mut x = pan.0.rem_euclid(spacing);
					while x < f32::from(bounds.size.width) {
						let mut y = pan.1.rem_euclid(spacing);
						while y < f32::from(bounds.size.height) {
							window.paint_quad(gpui::fill(
								gpui::Bounds::new(
									bounds.origin + gpui::point(gpui::px(x), gpui::px(y)),
									gpui::size(gpui::px(1.), gpui::px(1.)),
								),
								gpui::rgba(0xffffff28),
							));
							y += spacing;
						}
						x += spacing;
					}
					for (edge, a, b) in &paint_lines {
						let mut route = route(a, b, edge.kind);
						let fact = selected_edge
							.as_ref()
							.filter(|picked| {
								!edge.executing(b, connected) && picked.same_pair(edge)
							})
							.unwrap_or(edge);
						if fact.from != edge.from {
							route = route.reversed();
						}
						let selected = focused_agent
							.as_ref()
							.is_some_and(|key| &edge.from == key || &edge.to == key)
							|| selected_edge.as_ref().is_some_and(|picked| {
								picked.same_pair(edge)
									|| (picked.kind == Kind::Resource
										&& edge.kind == Kind::Resource
										&& picked.to == edge.to)
							});
						let faded =
							(selected_edge.is_some() || focused_agent.is_some()) && !selected;
						let active = edge.executing(b, connected);
						let show_flow = !faded && active;
						let color = if active {
							crate::ui_theme::BLUE
						} else if selected {
							fact.color
						} else {
							edge.color
						};

						let path = route.stroke(
							fact.kind,
							bounds.origin,
							scale,
							pan,
							if selected { 1.6 } else { 1.2 },
						);
						if let Ok(path) = path.build() {
							window.paint_path(
								path,
								gpui::rgba(
									(color << 8)
										| if faded {
											0x30
										} else if selected {
											0xef
										} else {
											0x90
										},
								),
							);
						}
						if show_flow && !reduce_motion && window.is_window_active() {
							let phase = (flow_started.elapsed().as_secs_f32() / 2.8).fract();
							route.paint_flow(bounds.origin, scale, pan, phase, color, window);
							animate = true;
						}
					}
					if animate {
						crate::ui_motion::request_frame(window, cx);
					}
				},
			)
			.absolute()
			.inset_0(),
		);
		for (edge, a, b) in lines {
			let label = route(&a, &b, edge.kind).label;
			let picked = edge.clone();
			let pressed_edge = edge.clone();
			let keyboard_edge = edge.clone();
			let faded = self
				.work_board
				.focus
				.as_ref()
				.is_some_and(|key| &edge.from != key && &edge.to != key)
				|| self.work_board.view.edge.as_ref().is_some_and(|picked| {
					!picked.same_pair(&edge)
						&& !(picked.kind == Kind::Resource
							&& edge.kind == Kind::Resource
							&& picked.to == edge.to)
				});
			let count = graph.edges.iter().filter(|other| edge.same_pair(other)).count();
			let label_text =
				if count > 1 { format!("{count} records") } else { edge.label.clone() };
			let name = format!("relation-edge-{}", edge.id);
			canvas = canvas.child(
				gpui::div()
					.id(SharedString::from(name.clone()))
					.debug_selector(move || name.clone())
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("{}: {} → {}", label_text, a.title, b.title))
					.absolute()
					.left(gpui::px((label.0 - 48.) * scale + pan.0))
					.top(gpui::px((label.1 - 10.) * scale + pan.1))
					.px(gpui::px(8. * scale))
					.py(gpui::px(4. * scale))
					.opacity(if faded { 0.35 } else { 1. })
					.text_size(gpui::px(10. * scale))
					.text_color(gpui::rgb(edge.color))
					.cursor_pointer()
					.child(label_text)
					.on_mouse_down(
						MouseButton::Left,
						cx.listener(move |s, _, _, cx| {
							s.work_board.focus = None;
							s.handoffs.focus = None;
							s.work_board.view.show_record = false;
							s.work_board.view.camera_fixed = true;
							s.work_board.view.edge = Some(pressed_edge.clone());
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
							cx.stop_propagation();
							cx.notify();
						}
					}))
					.on_click(cx.listener(move |s, _, _, cx| {
						s.work_board.focus = None;
						s.handoffs.focus = None;
						s.work_board.view.camera_fixed = true;
						s.work_board.view.edge = Some(picked.clone());
						cx.notify();
					})),
			);
		}
		let neighborhood = self.relation_neighborhood(graph);
		for node in &graph.nodes {
			let selected = self.work_board.view.edge.as_ref().is_some_and(|e| {
				e.from == node.key
					|| e.to == node.key
					|| (e.kind == Kind::Resource
						&& graph.edges.iter().any(|other| {
							other.kind == Kind::Resource
								&& other.to == e.to
								&& other.from == node.key
						}))
			}) || (self.work_board.focus.is_some()
				&& neighborhood.contains(&node.key))
				|| (self.work_board.focus.is_none()
					&& self.work_board.view.edge.is_none()
					&& node.row.as_ref().is_some_and(|r| {
						if r.native {
							self.native_agents.selected.as_ref().is_some_and(|(owner, thread)| {
								owner == &r.work && r.thread.as_ref() == Some(thread)
							})
						} else {
							self.native_agents.selected.is_none()
								&& self.selected.as_ref() == Some(&r.work)
						}
					}));
			let faded = self.work_board.focus.is_some() && !neighborhood.contains(&node.key)
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
			let origin = (node.x, node.y);
			let dragkey = key.clone();
			let name = format!("relation-node-{key}");
			let tip = format!(
				"{}\n{}",
				node.row.as_ref().map(|r| r.title.as_str()).unwrap_or(&node.title),
				node.status
			);

			let view = gpui::div()
				.id(SharedString::from(name.clone()))
				.debug_selector(move || name.clone())
				.role(Role::Button)
				.tab_index(0)
				.aria_label(format!("{} · {}", node.title, node.status))
				.absolute()
				.left(gpui::px(node.x * scale + pan.0))
				.top(gpui::px(node.y * scale + pan.1))
				.w(gpui::px(224. * scale))
				.h(gpui::px(node.height() * scale))
				.px(gpui::px(8. * scale))
				.py(gpui::px(6. * scale))
				.rounded(gpui::px(if node.key.starts_with("resource:") {
					3. * scale
				} else {
					6. * scale
				}))
				.border(gpui::px(0.65 * scale))
				.border_color(gpui::rgba(if selected { 0x9aaeee70 } else { 0xffffff16 }))
				// A quiet matte surface. No simulated refraction or specular rim.
				.bg(gpui::rgb(if selected { 0x323641 } else { 0x2c2d32 }))
				.text_color(gpui::rgb(if faded { TEXT_MUTED } else { TEXT }))
				.hover(|d| d.bg(gpui::rgb(0x34363c)).border_color(gpui::rgba(0xd5dfff38)))
				.cursor_pointer()
				.flex()
				.flex_col()
				.gap(gpui::px(3. * scale))
				.overflow_hidden()
				.tooltip(move |_, cx| cx.new(|_| RelationTip(tip.clone())).into())
				.child(
					gpui::div()
						.flex()
						.items_center()
						.gap(gpui::px(8. * scale))
						.child(status_lamp(
							node.color,
							connected && node.running(),
							scale,
							flow_started,
						))
						.child(
							gpui::div()
								.flex_1()
								.w_0()
								.min_w_0()
								.text_size(gpui::px(12. * scale))
								.child(crate::ui_motion::OverflowLabel {
									id: format!("relation-title-{}", node.key).into(),
									text: node.title.clone().into(),
								}),
						),
				)
				.when_some(node.task(), |d, task| {
					d.child(
						gpui::div()
							.text_size(gpui::px(11. * scale))
							.text_color(gpui::rgb(TEXT_MUTED))
							.min_w_0()
							.child(crate::ui_motion::OverflowLabel {
								id: format!("relation-task-{}", node.key).into(),
								text: task.to_owned().into(),
							}),
					)
				})
				.when(node.row.is_some(), |d| d.child(self.node_metrics(node, graph, scale)))
				.on_mouse_down(
					MouseButton::Left,
					cx.listener(move |s, e: &MouseDownEvent, _, cx| {
						s.work_board.view.moved = false;
						s.work_board.view.camera_fixed = true;
						s.work_board.view.zoom = Some(scale);
						s.work_board.view.drag =
							Some(Drag { key: Some(dragkey.clone()), last: e.position, origin });
						cx.stop_propagation();
					}),
				)
				.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
					if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
						if let Some(row) = &keyboard_row {
							s.inspect_station(row, cx);
						} else {
							s.work_board.focus = None;
							s.work_board.view.edge = keyboard_target.clone();
							cx.notify();
						}
						cx.stop_propagation();
					}
				}))
				.on_click(cx.listener(move |s, _, _, cx| {
					if s.work_board.view.moved {
						s.work_board.view.moved = false;
						return;
					}
					if let Some(row) = &row {
						s.inspect_station(row, cx);
					} else {
						s.work_board.focus = None;
						s.work_board.view.edge = target.clone();
						cx.notify();
					}
				}));
			canvas = canvas.child(view);
		}
		canvas.into_any_element()
	}
}

fn status_lamp(color: u32, running: bool, scale: f32, clock: std::time::Instant) -> AnyElement {
	gpui::canvas(
		|_, _, _| (),
		move |bounds, _, window, cx| {
			let animate = running && !crate::ui_motion::reduced() && window.is_window_active();
			let phase = (clock.elapsed().as_secs_f32() / 2.8).fract();
			if animate {
				let alpha = (24. + 32. * (phase * std::f32::consts::TAU).sin().abs()) as u32;
				window.paint_quad(
					gpui::fill(bounds, gpui::rgba((color << 8) | alpha))
						.corner_radii(gpui::px(5. * scale)),
				);
				crate::ui_motion::request_frame(window, cx);
			}
			let dot = gpui::Bounds::new(
				bounds.origin + gpui::point(gpui::px(2. * scale), gpui::px(2. * scale)),
				gpui::size(gpui::px(6. * scale), gpui::px(6. * scale)),
			);
			window.paint_quad(gpui::fill(dot, gpui::rgb(color)).corner_radii(gpui::px(3. * scale)));
		},
	)
	.w(gpui::px(10. * scale))
	.h(gpui::px(10. * scale))
	.flex_none()
	.into_any_element()
}

struct Route {
	start: (f32, f32),
	end: (f32, f32),
	first: (f32, f32),
	second: (f32, f32),
	label: (f32, f32),
}
fn route(a: &Node, b: &Node, kind: Kind) -> Route {
	let offset = match kind {
		Kind::Context => 64.,
		Kind::Resource => -80.,
		Kind::Message =>
			if a.x < b.x {
				-112.
			} else {
				112.
			},
		Kind::Wait => 56.,
		Kind::Returned => -56.,
		_ => 0.,
	};
	let (start, end, first, second) = if (a.x - b.x).abs() < 1. {
		let x = a.x + 224.;
		let lane = x + 144. + if kind == Kind::Context { 64. } else { 0. };
		(
			(x, a.y + a.height() / 2.),
			(x, b.y + b.height() / 2.),
			(lane, a.y + a.height() / 2. + offset),
			(lane, b.y + b.height() / 2. + offset),
		)
	} else {
		let forward = a.x < b.x;
		let start = (a.x + if forward { 224. } else { 0. }, a.y + a.height() / 2.);
		let end = (b.x + if forward { 0. } else { 224. }, b.y + b.height() / 2.);
		let bend = (end.0 - start.0) * 0.45;
		(start, end, (start.0 + bend, start.1 + offset), (end.0 - bend, end.1 + offset))
	};
	let label = (
		(start.0 + 3. * first.0 + 3. * second.0 + end.0) / 8.,
		(start.1 + 3. * first.1 + 3. * second.1 + end.1) / 8.,
	);
	Route { start, end, first, second, label }
}

impl View {
	fn finish_drag(&mut self) {
		if let Some(Drag { key: Some(key), origin, .. }) = self.drag.take() {
			self.positions.insert(key, (snap(origin.0), snap(origin.1)));
		}
	}

	fn zoom_at(&mut self, old: f32, next: f32) {
		let center = (self.viewport.0 / 2., self.viewport.1 / 2.);
		self.zoom_around(old, next, center);
	}

	fn zoom_around(&mut self, old: f32, next: f32, center: (f32, f32)) {
		let next = next.min(1.);
		self.pan = (
			center.0 - (center.0 - self.pan.0) * next / old,
			center.1 - (center.1 - self.pan.1) * next / old,
		);
		self.zoom = Some(next);
	}
}
impl AgentSurface {
	fn relation_neighborhood(&self, graph: &Graph) -> BTreeSet<String> {
		if let Some(key) = &self.work_board.focus {
			let mut keys = BTreeSet::from([key.clone()]);
			for edge in &graph.edges {
				if &edge.from == key || &edge.to == key {
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

	fn arrange_relations(&mut self) {
		self.work_board.view.positions.clear();
		self.work_board.view.drag = None;
		self.work_board.view.moved = false;
		self.work_board.view.camera_fixed = true;
		self.fit_relations();
	}

	pub(super) fn fit_relations(&mut self) {
		let graph = self.relation_graph();
		let keys = graph.nodes.iter().map(|n| n.key.clone()).collect();
		let Some([min_x, min_y, max_x, max_y]) = graph.content_bounds(&keys) else {
			return;
		};
		let (w, h) = self.work_board.view.viewport;
		if w <= 48. || h <= 48. {
			return;
		}
		let width = max_x - min_x;
		let height = max_y - min_y;
		// A fixed screen-space margin, independent of node count and zoom.
		let scale = ((w - 48.) / width).min((h - 48.) / height).min(1.);
		self.work_board.view.zoom = Some(scale);
		self.work_board.view.pan =
			((w - width * scale) / 2. - min_x * scale, (h - height * scale) / 2. - min_y * scale);
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
		self.work_board.briefs.insert(row.key.clone(),Brief{metrics:Default::default(),resources:None,stamp:self.brief_stamp(&row),read_at:std::time::Instant::now(),relations:vec![("review-wait".into(),decodex_protocol::AgentCollaborationDto {sender_thread_id:"fixture-release".into(),receiver_thread_ids:vec!["fixture-reviewer".into()],tool:"wait".into(),status:"completed".into(),prompt:String::new(),results:vec![decodex_protocol::AgentCollaborationResultDto{thread_id:"fixture-reviewer".into(),status:"completed".into(),message:"Cancellation can leave the sign-in button disabled. Add a regression test before release.".into()}]})]});
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
	fn fit_ignores_selection_and_arrange_restores_dragged_nodes(cx: &mut gpui::TestAppContext) {
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
				for coordinate in [node.x, node.y, node.x + 224., node.y + node.height()] {
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
			assert!(a[3] <= b[1] || b[3] <= a[1]);
			for scope in &graph.scopes {
				for node in graph
					.nodes
					.iter()
					.filter(|n| n.row.as_ref().is_some_and(|r| r.workspace == scope.id))
				{
					assert!(node.x >= scope.bounds[0] && node.x + 224. <= scope.bounds[2]);
					assert!(
						node.y >= scope.bounds[1] + 30.
							&& node.y + node.height() <= scope.bounds[3]
					);
				}
			}
			s.work_board.view.viewport = (1200., 900.);
			s.fit_relations();
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
		let mut view = View { pan: (3.5, -7.25), ..Default::default() };
		view.drag = Some(Drag {
			key: Some("node".into()),
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
	fn zoom_stops_at_normal_size_without_moving_the_anchor() {
		let mut view = View { pan: (40., -20.), ..Default::default() };
		view.zoom_around(0.5, 4., (200., 120.));
		assert_eq!(view.zoom, Some(1.));
		assert_eq!(view.pan, (-120., -160.));
		let pan = view.pan;
		view.zoom_at(1., 1.2);
		assert_eq!(view.zoom, Some(1.));
		assert_eq!(view.pan, pan);
	}

	#[test]
	fn reverse_flow_follows_the_same_curve_in_the_opposite_direction() {
		let route = Route {
			start: (0., 0.),
			end: (200., 80.),
			first: (70., -40.),
			second: (150., 120.),
			label: (100., 40.),
		};
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
			x: 120.,
			y: 80.,
		};
		let graph =
			Graph { scopes: vec![], nodes: vec![node], edges: vec![], width: 5000., height: 5000. };
		assert_eq!(
			graph.content_bounds(&BTreeSet::from(["a".into()])),
			Some([120., 80., 344., 128.])
		);
	}
}
