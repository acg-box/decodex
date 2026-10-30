//! Agent topology: typed connections are inspectable evidence, never inferred from message prose.
#[path = "agent_relation_metrics.rs"] pub(super) mod metrics;

#[path = "agent_relation_allocation.rs"] mod allocation;
#[path = "agent_relation_camera.rs"] mod camera;
#[path = "agent_relation_card.rs"] mod card;
#[path = "agent_relation_groups.rs"] mod groups;
#[path = "agent_relation_header.rs"] mod header;
#[path = "agent_relation_hover.rs"] mod hover;
#[path = "agent_relation_layers.rs"] mod layers;
#[path = "agent_relation_layout.rs"] mod layout;
#[path = "agent_relation_mascot.rs"] mod mascot;
#[path = "agent_relation_minimap.rs"] mod minimap;
#[path = "agent_relation_picker.rs"] mod picker;
#[path = "agent_relation_resources.rs"] mod resources;
#[path = "agent_relation_route.rs"] mod routing;
#[path = "agent_relation_scopes.rs"] mod scopes;
#[path = "agent_relation_stream.rs"] mod stream;
#[path = "agent_relation_style.rs"] mod style;
#[path = "agent_relation_usage.rs"] mod usage;
#[path = "agent_relation_visibility.rs"] mod visibility;

use std::{
	cell::{OnceCell, RefCell},
	iter, mem,
	time::Duration,
};

use gpui::{
	AppContext, ClickEvent, Div, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
	PathBuilder, Pixels, Point, Render, Role, ScrollEvent, SharedString, Stateful,
	StatefulInteractiveElement, TextRun, Window, WindowTextSystem,
};

#[cfg(any(test, feature = "visual-capture"))]
use crate::shell::agent_surface::work_board::Brief;
use crate::{
	frame_trace,
	shell::{
		agent_surface::{
			dock, graph,
			panel_controls::Panel,
			work_board::{
				AgentSurface, AnyElement, BTreeMap, BTreeSet, Context, FluentBuilder,
				InteractiveElement, IntoElement, ParentElement, Row, SUCCESS, Styled, TEXT_MUTED,
				WARNING,
			},
			workspace,
		},
		workspace_symbols,
		workspace_symbols::Symbol,
	},
	ui_motion::{self, SmoothControl},
	ui_theme::{self, COMPOSER_MATERIAL, FONT_FAMILY, INFO, LINE_STRONG, TEXT},
};
use card::{Card, INSET_X, INSET_Y, MAX_ZOOM, MIN_ZOOM, OVERVIEW_HEIGHT, SELECTED};
use decodex_protocol::{AgentSnapshotDto, AgentWorkKindDto};
use header::Header;
use hover::Hover;
use layers::Layers;
#[cfg(test)] use routing::route;
use routing::{Ink, Route};
use scopes::Scope;
use stream::{Frame, LINE};
use style::StrokeBatch;
use usage::Metric;
use visibility::Link;

const GRID: f32 = 16.;
const FRAME_INSET: f32 = 16.;

#[derive(Default)]
pub(super) struct View {
	pub(super) camera_fixed: bool,
	reveal_after_resize: bool,
	resize_anchor: Option<String>,
	flow_clock: OnceCell<std::time::Instant>,
	positions: BTreeMap<String, (f32, f32)>,
	pan: (f32, f32),
	zoom: Option<f32>,
	card_transition: Option<crate::ui_motion::Tween>,
	viewport: (f32, f32),
	display_scale: f32,
	layout_viewport: (f32, f32),
	layout: RefCell<layout::Cache>,
	layout_mode: layout::Mode,
	presentation: layout::Presentation,
	parked_positions: BTreeMap<String, (f32, f32)>,
	wires: Option<crate::ui_motion::Tween>,
	motion: Option<SceneMotion>,
	map_drag: Option<minimap::Drag>,
	activity: RefCell<BTreeMap<String, stream::Stream>>,
	mascots: RefCell<BTreeMap<String, mascot::Actor>>,
	layers: Layers,
	routing: RefCell<routing::Cache>,
	visibility: RefCell<visibility::Cache>,
	origin: (f32, f32),
	node_count: usize,
	drag: Option<Drag>,
	moved: bool,
	pub(super) edge: Option<Edge>,
	pub(super) show_record: bool,
	legend_visible: bool,
	usage_metric: Metric,
	usage_hidden: bool,
	usage_menu: bool,
	hover: Hover,
	pub(super) picked_agent: Option<String>,
	pub(super) picked_project: Option<String>,
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
		self.map_drag = None;

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
		let next = next.clamp(MIN_ZOOM, MAX_ZOOM);

		self.pan = (
			center.0 - (center.0 - self.pan.0) * next / old,
			center.1 - (center.1 - self.pan.1) * next / old,
		);
		self.zoom = Some(next);
	}
}

#[derive(Clone)]
pub(super) struct Edge {
	pub(super) id: String,
	pub(super) from: String,
	pub(super) to: String,
	pub(super) kind: Kind,
	pub(super) in_progress: bool,
	pub(super) color: u32,
	pub(super) label: String,
	pub(super) detail: String,
	pub(super) excerpt: Option<String>,
	pub(super) source: Option<(String, String)>,
	pub(super) link: Option<String>,
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
	assignment: String,
	status: String,
	color: u32,
	row: Option<Row>,
	activity: Frame,
	pub(super) size: (f32, f32),
	// Character/HUD attachment in local world coordinates; resources use their full bounds.
	attachment: Option<[f32; 4]>,
	// Visible identity is a routing obstacle, never a connection attachment.
	identity: Option<[f32; 4]>,
	// Only visible speech is an obstacle; empty camera space never blocks a link.
	speech: Option<[f32; 4]>,
	base_height: f32,
	// Available detail controls placement; inspection changes only the visible footprint.
	layout_height: f32,
	// Navigation uses the final placement while the canvas animates x and y.
	navigation: (f32, f32),
	pub(super) x: f32,
	pub(super) y: f32,
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

	fn port_y(&self) -> f32 {
		let [_, top, _, bottom] = self.connection_bounds();

		(top + bottom) / 2.
	}

	fn connection_bounds(&self) -> [f32; 4] {
		let [left, top, right, bottom] =
			self.attachment.unwrap_or([0., 0., self.width(), self.height()]);

		[self.x + left, self.y + top, self.x + right, self.y + bottom]
	}

	fn connection_center(&self) -> (f32, f32) {
		let [left, top, right, bottom] = self.connection_bounds();

		((left + right) / 2., (top + bottom) / 2.)
	}
}

pub(super) struct RelationTip(pub(super) String);
impl Render for RelationTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		ui_motion::tooltip_surface(gpui::div())
			.p_2()
			.max_w(gpui::px(360.))
			.text_color(ui_theme::rgb(TEXT))
			.text_size(gpui::px(12.))
			.child(self.0.clone())
	}
}

pub(super) struct Graph {
	layout: card::Layout,
	scopes: Vec<Scope>,
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

	/// Captions use free space along the route; they must not cover an agent.
	fn connection_label_bounds(&self, edge: &Edge, path: &Route) -> Option<[f32; 4]> {
		let label = self.connection_label(edge);

		iter::once(path.label)
			.chain((1..=64).flat_map(|step| {
				let offset = step as f32 / 130.;

				[path.at(0.5 - offset), path.at(0.5 + offset)]
			}))
			.map(|point| label.bounds(point))
			.find(|bounds| {
				self.nodes.iter().all(|node| {
					bounds[2] <= node.x
						|| node.x + node.width() <= bounds[0]
						|| bounds[3] <= node.y
						|| node.y + node.height() <= bounds[1]
				})
			})
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

			if let Some([left, top, right, bottom]) = self.connection_label_bounds(&edge, path) {
				include(left, top);
				include(right, bottom);
			}
		}

		bounds
	}

	#[cfg(test)]
	pub(super) fn agent_count(&self) -> usize {
		self.nodes.iter().filter(|node| node.row.is_some()).count()
	}
}

struct SceneMotion {
	from: BTreeMap<String, (f32, f32)>,
	progress: crate::ui_motion::Tween,
	started: std::time::Instant,
}

struct TextWidths {
	nameplate: f32,
	speech: Option<f32>,
}

struct Drag {
	key: Option<String>,
	start: Point<Pixels>,
	last: Point<Pixels>,
	origin: (f32, f32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
			Self::Dependency => WARNING,
			Self::Context => 0xb0a0d8,
			Self::Resource => 0x9cbacb,
			Self::Message => 0x8b9fff,
			Self::Control => TEXT_MUTED,
			Self::Wait => WARNING,
			Self::Returned => SUCCESS,
		}
	}
}

impl AgentSurface {
	pub(in super::super) fn reveal_relations_after_resize(&mut self) {
		let graph = self.relation_graph();
		let view = &mut self.work_board.view;
		let scale = view.zoom.unwrap_or(1.).clamp(MIN_ZOOM, MAX_ZOOM);
		let center = (view.viewport.0 / 2., view.viewport.1 / 2.);

		view.resize_anchor = view.picked_agent.clone().or_else(|| {
			graph
				.nodes
				.iter()
				.filter(|node| node.row.is_some())
				.min_by(|a, b| {
					let distance = |node: &Node| {
						let rect = node.screen_rect(scale, view.pan);

						(rect.x + rect.width / 2. - center.0).powi(2)
							+ (rect.y + rect.height / 2. - center.1).powi(2)
					};

					distance(a).total_cmp(&distance(b))
				})
				.map(|node| node.key.clone())
		});
		view.reveal_after_resize = view.resize_anchor.is_some();
	}

	pub(in super::super) fn clear_relation_selection(&mut self) {
		self.work_board.view.picked_agent = None;
		self.work_board.view.picked_project = None;
		self.work_board.view.edge = None;
		self.work_board.focus = None;
	}

	pub(super) fn relation_graph(&self) -> Graph {
		self.relation_graph_at(std::time::Instant::now())
	}

	fn relation_graph_at(&self, now: std::time::Instant) -> Graph {
		self.relation_graph_with_layout(now, self.relation_layout_at(now))
	}

	fn relation_layout_at(&self, now: std::time::Instant) -> card::Layout {
		let layout = card::Layout::new(self.relation_scale());

		self.work_board.view.card_transition.as_ref().map_or(layout, |transition| {
			layout.with_presence(
				if layout.overview() { transition.sample(now) } else { 1. },
				transition.moving(now),
			)
		})
	}

	fn relation_graph_with_layout(&self, now: std::time::Instant, layout: card::Layout) -> Graph {
		let mut graph =
			Graph { layout, scopes: vec![], nodes: vec![], edges: vec![], paths: BTreeMap::new() };
		let Some(snapshot) = &self.snapshot else { return graph };
		let text_system = WindowTextSystem::new(self.work_board.text_system.clone());
		let mut rows = self.factory_rows();

		rows.sort_by_key(|row| self.relation_row_order(row, snapshot));

		let included: BTreeSet<String> = rows.iter().map(|r| r.key.clone()).collect();
		let mut streams = self.work_board.view.activity.borrow_mut();

		streams.retain(|key, _| included.contains(key));
		self.work_board.view.mascots.borrow_mut().retain(|key, _| included.contains(key));

		for source in rows.iter().filter(|r| included.contains(&r.key)) {
			let mut row = source.clone();

			if self.graph_question(&row).is_some() {
				row.status = "Needs you".into();
				row.color = WARNING;
			}

			let identity = self.relation_agent_name(&row);
			let (scope, operations) = self.graph_operations(&row);
			let stream = streams
				.entry(row.key.clone())
				.or_insert_with(|| stream::Stream::new(&scope, &operations, now));

			stream.sync(&scope, &operations, now);

			let text_layout = layout.on_display(self.work_board.view.display_scale);
			let activity = stream.measured_frame(now, ui_motion::reduced(), |text| {
				header::operation_height(text, text_layout, &text_system)
			});
			let assignment = self.graph_purpose(&row);
			let layout_height = layout.world_height(header::detail_rows(
				&assignment,
				&activity,
				text_layout,
				&text_system,
			));
			let height = layout_height;
			let attachment = Some(text_layout.character_bounds(activity.height / LINE));
			let title_lines =
				header::content_rows(&assignment, &activity, text_layout, &text_system).0;
			let identity_bounds = text_layout.identity_bounds(activity.height / LINE, title_lines);
			let speech = text_layout.speech_bounds(activity.height / LINE);

			graph.nodes.push(Node {
				key: row.key.clone(),
				title: identity,
				assignment,
				status: if row.review {
					format!("{} · Result unread", row.status)
				} else {
					row.status.clone()
				},
				color: row.color,
				row: Some(row),
				activity,
				size: (layout.world_width(), height),
				base_height: layout.world_height(0.),
				layout_height,
				navigation: (0., 0.),
				attachment,
				identity: identity_bounds,
				speech,
				x: 0.,
				y: 0.,
			});
		}

		drop(streams);

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
		self.place_relation_nodes(&mut graph, layout, &text_system);

		graph.paths = self
			.work_board
			.view
			.routing
			.borrow_mut()
			.paths(&groups::routing_nodes(&graph, &text_system), &graph.connections());

		graph.scope_bounds(&snapshot.projects);

		graph
	}

	fn relation_row_order(&self, row: &Row, snapshot: &AgentSnapshotDto) -> (usize, bool, usize) {
		let owner =
			snapshot.work_items.iter().position(|work| work.id == row.work).unwrap_or(usize::MAX);
		let child = self
			.native_agents
			.lists
			.get(&row.work)
			.and_then(|agents| {
				agents.iter().position(|agent| Some(&agent.thread_id) == row.thread.as_ref())
			})
			.unwrap_or(0);

		(owner, row.native, child)
	}

	fn relation_scale(&self) -> f32 {
		self.work_board.view.zoom.unwrap_or(1.).clamp(MIN_ZOOM, MAX_ZOOM)
	}

	fn relation_controls(&self, cx: &mut Context<Self>) -> AnyElement {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let layout_viewport = self.work_board.view.layout_viewport;
		let cluster = crate::ui_theme::canvas_control_group;

		gpui::div()
			.id("relations-toolbar")
			.debug_selector(|| "relations-toolbar".into())
			.w_full()
			.h(gpui::px(38.))
			.px_2()
			.flex_none()
			.flex()
			.items_center()
			.gap_2()
			.text_size(gpui::px(11.))
			.child(self.relation_layout_picker(cx))
			.child(self.relation_presentation_picker(cx))
			.child(gpui::div().flex_1())
			.child(
				cluster()
					.debug_selector(|| "relations-layout-controls".into())
					.child(self.graph_control(
						"arrange",
						workspace_symbols::icon_sized(Symbol::Arrange, 14.),
						"Restore automatic layout",
						|s, cx| {
							s.arrange_relations();
							cx.notify();
						},
						cx,
					))
					.child(self.graph_control(
						"fit",
						workspace_symbols::icon_sized(Symbol::Fit, 14.),
						&format!("Frame agents (minimum {:.0}%)", card::MIN_ZOOM * 100.),
						|s, cx| {
							s.work_board.view.camera_fixed = true;

							s.fit_relations();
							cx.notify();
						},
						cx,
					))
					.child(self.relation_legend_toggle(cx))
					.child(self.relation_usage_options(cx)),
			)
			.child(
				cluster()
					.debug_selector(|| "relations-zoom-controls".into())
					.child(self.graph_control(
						"out",
						"−",
						&format!("Zoom out (minimum {:.0}%)", card::MIN_ZOOM * 100.),
						move |s, cx| {
							s.work_board.view.hold_camera(scale, pan, layout_viewport);
							s.zoom_relations(scale, scale - 0.05, None);
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
							s.zoom_relations(scale, scale + 0.05, None);
							cx.notify();
						},
						cx,
					)),
			)
			.child(self.panel_controls(Panel::Dock, cx))
			.into_any_element()
	}

	fn switch_relation_layout(&mut self, mode: layout::Mode, cx: &mut Context<Self>) {
		if self.work_board.view.layout_mode == mode {
			return;
		}

		let before = self.relation_graph();
		let old_pan = self.work_board.view.pan;
		let scale = self.relation_scale();
		let center = (self.work_board.view.viewport.0 / 2., self.work_board.view.viewport.1 / 2.);
		let anchor = self.work_board.view.picked_agent.clone().or_else(|| {
			before
				.nodes
				.iter()
				.filter(|node| node.row.is_some())
				.min_by(|a, b| {
					let distance = |node: &Node| {
						((node.x + node.width() / 2.) * scale + old_pan.0 - center.0).powi(2)
							+ ((node.y + node.height() / 2.) * scale + old_pan.1 - center.1).powi(2)
					};

					distance(a).total_cmp(&distance(b))
				})
				.map(|node| node.key.clone())
		});
		let view = &mut self.work_board.view;

		view.layout_mode = mode;
		view.motion = None;
		view.map_drag = None;
		view.drag = None;

		view.positions.clear();
		view.hover.update(None, std::time::Instant::now());

		let target = self.relation_graph();

		if let Some(node) = target.nodes.iter().find(|node| Some(&node.key) == anchor.as_ref()) {
			self.work_board.view.pan = (
				center.0 - (node.x + node.width() / 2.) * scale,
				center.1 - (node.y + node.height() / 2.) * scale,
			);
		}

		self.animate_relation_change(&before, old_pan);
		cx.notify();
	}

	fn switch_relation_presentation(&mut self, mode: layout::Presentation, cx: &mut Context<Self>) {
		if self.work_board.view.presentation == mode {
			return;
		}

		let before = self.relation_graph();
		let pan = self.work_board.view.pan;
		let scale = self.relation_scale();
		let center = (
			(self.work_board.view.viewport.0 / 2. - pan.0) / scale,
			(self.work_board.view.viewport.1 / 2. - pan.1) / scale,
		);
		let anchor = self.work_board.view.picked_agent.clone().or_else(|| {
			before
				.nodes
				.iter()
				.filter(|node| node.row.is_some())
				.min_by(|a, b| {
					let distance = |n: &Node| {
						let p = n.connection_center();

						(p.0 - center.0).hypot(p.1 - center.1)
					};

					distance(a).total_cmp(&distance(b))
				})
				.map(|node| node.key.clone())
		});
		let view = &mut self.work_board.view;

		view.presentation = mode;
		view.motion = None;
		view.drag = None;
		view.map_drag = None;

		mem::swap(&mut view.positions, &mut view.parked_positions);

		view.hover.update(None, std::time::Instant::now());
		view.wires.get_or_insert_with(|| crate::ui_motion::Tween::new(1.)).target(
			if mode == layout::Presentation::Links { 1. } else { 0. },
			std::time::Instant::now(),
		);

		// Retain the selected character's screen position as its team gathers.
		let target = self.relation_graph();

		if let Some((old, new)) = anchor.as_ref().and_then(|key| {
			Some((
				before.nodes.iter().find(|n| &n.key == key)?,
				target.nodes.iter().find(|n| &n.key == key)?,
			))
		}) {
			let (a, b) = (old.connection_center(), new.connection_center());

			self.work_board.view.pan = (pan.0 + (a.0 - b.0) * scale, pan.1 + (a.1 - b.1) * scale);
		}

		self.animate_relation_change(&before, pan);
		cx.notify();
	}

	fn relation_wire_presence(&self, now: std::time::Instant) -> f32 {
		self.work_board.view.wires.as_ref().map_or(
			if self.work_board.view.presentation == layout::Presentation::Links { 1. } else { 0. },
			|wires| wires.sample(now),
		)
	}

	fn animate_relation_change(&mut self, before: &Graph, old_pan: (f32, f32)) {
		let scale = self.relation_scale();
		let view = &mut self.work_board.view;

		view.camera_fixed = true;
		view.zoom = Some(scale);

		let started = std::time::Instant::now();
		let mut progress = crate::ui_motion::Tween::new(0.);

		if view.presentation == layout::Presentation::Together {
			progress = progress.duration(Duration::from_millis(560));
		}

		progress.target(1., started);

		view.motion = Some(SceneMotion {
			from: before
				.nodes
				.iter()
				.map(|node| {
					(node.key.clone(), {
						let center = node.connection_center();

						(
							center.0 + (old_pan.0 - view.pan.0) / scale,
							center.1 + (old_pan.1 - view.pan.1) / scale,
						)
					})
				})
				.collect(),
			progress,
			started,
		});
	}

	fn navigate_relation_center(&mut self, center: (f32, f32), cx: &mut Context<Self>) {
		let before = self.relation_graph();
		let old_pan = self.work_board.view.pan;
		let scale = self.relation_scale();
		let view = &mut self.work_board.view;

		view.motion = None;
		view.pan =
			(view.viewport.0 / 2. - center.0 * scale, view.viewport.1 / 2. - center.1 * scale);

		view.hover.update(None, std::time::Instant::now());
		self.animate_relation_change(&before, old_pan);
		cx.notify();
	}

	fn navigate_relation_agent(&mut self, key: &str, cx: &mut Context<Self>) {
		let before = self.relation_graph();
		let old_pan = self.work_board.view.pan;

		self.work_board.view.motion = None;

		self.select_workbench_agent(key, cx);

		let target = self.relation_graph();
		let Some(node) = target.nodes.iter().find(|node| node.key == key) else {
			return;
		};
		let center = (node.x + node.width() / 2., node.y + node.height() / 2.);
		let scale = self.relation_scale();
		let view = &mut self.work_board.view;

		view.pan =
			(view.viewport.0 / 2. - center.0 * scale, view.viewport.1 / 2. - center.1 * scale);

		view.hover.update(None, std::time::Instant::now());
		self.animate_relation_change(&before, old_pan);
		cx.notify();
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
		let selector = format!("relations-{id}");

		gpui::div()
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector.clone())
			.role(Role::Button)
			.aria_label(description.to_owned())
			.when(id == "usage", |button| button.aria_expanded(self.work_board.view.usage_menu))
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
			.rounded(gpui::px(8.))
			.cursor_pointer()
			.hover(|d| d.bg(ui_theme::rgba(0xffffff10)))
			.tooltip(move |_, cx| cx.new(|_| RelationTip(tip.clone())).into())
			.child(label)
			.on_click(cx.listener(move |s, _, window, cx| {
				action(s, cx);

				workspace::restore_shell_focus(window, cx);
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, window, cx| {
				if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					keyboard(s, cx);

					workspace::restore_shell_focus(window, cx);

					cx.stop_propagation();
				}
			}))
			.smooth()
			.into_any_element()
	}

	/// Keep the active character above overlapping nodes, using the same order
	/// for painting and pointer targets after compact layouts or manual dragging.
	fn relation_nodes<'a>(&'a self, graph: &'a Graph) -> impl DoubleEndedIterator<Item = &'a Node> {
		let foreground =
			self.work_board.view.drag.as_ref().and_then(|drag| drag.key.as_deref()).or(self
				.work_board
				.view
				.picked_agent
				.as_deref());

		graph
			.nodes
			.iter()
			.filter(move |node| Some(node.key.as_str()) != foreground)
			.chain(graph.nodes.iter().filter(move |node| Some(node.key.as_str()) == foreground))
	}

	pub(super) fn relation_canvas(&self, graph: &Graph, cx: &mut Context<Self>) -> AnyElement {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let layout_viewport = self.work_board.view.layout_viewport;
		let (lines, moving) = self.work_board.view.visibility.borrow_mut().sample(
			graph,
			self.relation_context(),
			self.work_board.view.edge.as_ref(),
			self.command_connection_ready(),
			std::time::Instant::now(),
		);
		let mut canvas = gpui::div()
			.id("relations-canvas")
			.debug_selector(|| "relations-canvas".into())
			.font_family(FONT_FAMILY)
			.relative()
			.flex_1()
			.min_w_0()
			.min_h_0()
			.overflow_hidden()
			.on_scroll(cx.listener(move |s, e: &ScrollEvent, _, cx| {
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
			.on_mouse_move(
				cx.listener(move |s, e: &MouseMoveEvent, _, cx| s.move_relation_pointer(e, cx)),
			)
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
		canvas = self.paint_relation_connections(canvas, graph, &lines, moving, cx);
		canvas = self.relation_node_layer(canvas, graph, cx);

		self.work_board.view.layers.links.replace(lines);

		canvas = canvas.child(self.work_board.view.layers.bounds_recorder());

		gpui::div()
			.size_full()
			.flex()
			.flex_col()
			.child(self.relation_controls(cx))
			.child(canvas)
			.child(ui_motion::disclosure(
				"graph-usage-reveal",
				!self.work_board.view.usage_hidden,
				self.relation_usage(cx),
			))
			.into_any_element()
	}

	fn relation_foreground(
		&self,
		mut canvas: Stateful<Div>,
		graph: &Graph,
		lines: &[Link],
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		// Visible relationship labels keep their own hit targets above transparent node areas.
		canvas = self.relation_label_layer(canvas, graph, lines, cx);
		canvas = self.relation_scope_headers(canvas, graph, cx);

		if let Some(map) = self.relation_minimap(graph, cx) {
			canvas = canvas.child(map);
		}

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
				.child(ui_motion::disclosure(
					"graph-legend-reveal",
					self.work_board.view.legend_visible,
					self.relation_legend(),
				)),
		);

		canvas
	}

	fn move_relation_pointer(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
		if self.work_board.view.map_drag.is_some() {
			self.move_relation_minimap(e, cx);

			return;
		}

		let scale = self.relation_scale();

		if e.pressed_button != Some(MouseButton::Left) {
			let dragging = self.work_board.view.drag.is_some();

			self.work_board.view.finish_drag();

			if dragging {
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
			if snapshot.agents.iter().any(|a| a.id == work.id) {
				continue;
			}

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
						dock::progress_state(snapshot, work).label
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
						"{}\n\nNative child state: {}\nModel: {}\nEffort: {}\nCurrent native configuration when loaded; otherwise the latest saved values.\n\n{}",
						agent.title,
						agent.status,
						agent.model.as_deref().unwrap_or("Unknown"),
						agent.reasoning_effort.as_deref().unwrap_or("Unknown"),
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
						|w| graph::blockers(snapshot, w).iter().any(|b| b.id == edge.depends_on_id),
					);

				graph.edges.push(Edge {
					in_progress: false,
					id: format!("dependency:{}:{}", edge.depends_on_id, edge.work_item_id),
					from: edge.depends_on_id.clone(),
					to: edge.work_item_id.clone(),
					kind: Kind::Dependency,
					excerpt: None,
					color: if blocked { WARNING } else { TEXT_MUTED },
					label: if blocked { "Waiting" } else { "Prerequisite met" }.into(),
					detail: if blocked {
						"Waiting for this prerequisite."
					} else {
						"Prerequisite resolved · Downstream execution unconfirmed."
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
					assignment: String::new(),
					status: "Referenced thread".into(),
					color: Kind::Context.color(),
					row: None,
					activity: Default::default(),
					size: (224., 48.),
					attachment: None,
					identity: None,
					speech: None,
					base_height: 48.,
					layout_height: 48.,
					navigation: (0., 0.),
					x: 0.,
					y: 0.,
				});
			}

			graph.edges.push(Edge { in_progress: false, id:format!("context:{}:{}:{}",reference.event_id,reference.recipient_work_id,reference.source_thread_id),from:key,to:reference.recipient_work_id.clone(),kind:Kind::Context,excerpt:None,color:Kind::Context.color(),label:if reference.delivery_turn_id.is_some(){"Shared context"}else{"Context queued"}.into(),detail:format!("Shared context\n\nSource thread: {}\nInput receipt: {}\nReceiving turn: {}\n\nRead status unknown · Content version not recorded.",reference.source_thread_id,reference.event_id,reference.delivery_turn_id.as_deref().unwrap_or("Not delivered")),link:None,source:Some((reference.source_work_id.clone(),reference.source_thread_id.clone())) });
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
			let calls = brief.relations.iter().flat_map(|(item, call)| {
				call.receiver_thread_ids.iter().map(move |receiver| {
					(
						(call.sender_thread_id.clone(), receiver.clone(), call.tool.clone()),
						(item, call),
					)
				})
			});

			for ((sender, receiver, tool), (item, call)) in calls {
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
				let label = Self::native_call_label(&tool, &call.status);
				let detail = if native_activity {
					format!(
						"Recorded activity: {}\nSource item: {item}\n\nMessage body unavailable.",
						call.status
					)
				} else {
					format!(
						"Last observed tool: {tool}\nCall state: {}\nSource item: {item}\n\nPrompt excerpt:\n{}",
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
							"Observed by {tool}\nSource item: {item}\n\nReply excerpt:\n{}\n\nMay describe an earlier turn; reply linkage unknown.",
							result.message
						),
						link: None,
					source: Some((row.work.clone(), sender.clone())),
					});
				}
			}
		}
	}

	fn native_call_label(tool: &str, status: &str) -> String {
		match (tool, status) {
			(_, "failed") => "Call failed".into(),
			(_, "interrupted") => "Call interrupted".into(),
			(tool, _) => match tool {
				"subAgentActivity/started" => "Started agent".into(),
				"subAgentActivity/interacted" => "Interacted".into(),
				"subAgentActivity/completed" => "Turn completed".into(),
				"subAgentActivity/interrupted" => "Interrupted".into(),
				"spawnAgent" => "Started agent".into(),
				"sendInput" | "sendMessage" | "followupTask" => "Sent input".into(),
				"wait" => "Waited".into(),
				_ => format!("{} · {}", tool, status),
			},
		}
	}

	fn place_relation_nodes(
		&self,
		graph: &mut Graph,
		layout: card::Layout,
		text_system: &WindowTextSystem,
	) {
		self.work_board.view.layout.borrow_mut().place(
			graph,
			self.work_board.view.layout_viewport,
			self.work_board.view.layout_mode,
			self.work_board.view.presentation,
		);

		resources::place_resources(graph);

		let text_widths = self.relation_text_widths(graph, layout, text_system);

		for node in &mut graph.nodes {
			if layout.overview() && node.row.is_some() {
				let inset = layout.node_inset();

				node.x += inset.0;
				node.y += inset.1;
				node.size = layout.node_size(node.height());
				node.layout_height = node.height();
			}
			if node.row.is_some() {
				let native = layout.on_display(self.work_board.view.display_scale);
				let title_lines =
					header::content_rows(&node.assignment, &node.activity, native, text_system).0;

				node.identity = native.identity_bounds(node.activity.height / LINE, title_lines);

				if let Some(bounds) = node.identity.as_mut()
					&& let Some(widths) = text_widths.get(&node.key)
				{
					bounds[0] = (native.world_width() - widths.nameplate) / 2.;
					bounds[2] = bounds[0] + widths.nameplate;
				}

				node.speech = native.speech_bounds(node.activity.height / LINE);

				if let Some(bounds) = node.speech.as_mut()
					&& let Some(width) = text_widths.get(&node.key).and_then(|widths| widths.speech)
				{
					bounds[0] = (native.world_width() - width) / 2.;
					bounds[2] = bounds[0] + width;
				}

				node.attachment = Some(native.character_bounds(node.activity.height / LINE));
			}

			if let Some(&(x, y)) = self.work_board.view.positions.get(&node.key) {
				let offset =
					node.attachment.map_or((0., 0.), |b| ((b[0] + b[2]) / 2., (b[1] + b[3]) / 2.));

				node.x = x - offset.0;
				node.y = y - offset.1;
			}

			node.navigation = (node.x, node.y);
		}

		if let Some(motion) = &self.work_board.view.motion {
			let progress = motion.progress.sample(std::time::Instant::now());

			for node in &mut graph.nodes {
				if let Some(&(x, y)) = motion.from.get(&node.key) {
					let center = node.connection_center();

					node.x += (x - center.0) * (1. - progress);
					node.y += (y - center.1) * (1. - progress);
				}
			}
		}
	}

	fn relation_node_relatives(
		&self,
		node: &Node,
		graph: &Graph,
	) -> (Option<String>, Option<usize>) {
		let children = graph.edges.iter().any(|e| e.kind.hierarchy() && e.from == node.key);
		let coordinator = self
			.snapshot
			.iter()
			.flat_map(|s| s.agents.iter())
			.any(|a| a.id == node.key && a.assistant);
		let team = (children && !coordinator).then(|| groups::descendants(graph, &node.key).len());
		let parent = graph
			.edges
			.iter()
			.find(|e| e.kind.hierarchy() && e.to == node.key)
			.and_then(|e| graph.nodes.iter().find(|n| n.key == e.from))
			.and_then(|n| n.row.as_ref())
			.map(|row| self.relation_agent_number(row));

		(parent, team)
	}

	fn relation_text_widths(
		&self,
		graph: &Graph,
		layout: card::Layout,
		text: &WindowTextSystem,
	) -> BTreeMap<String, TextWidths> {
		if !layout.activity_visible() {
			return BTreeMap::new();
		}

		let native = layout.on_display(self.work_board.view.display_scale);
		let measure = |value: &str, numeric: bool| {
			let mut font = gpui::font(FONT_FAMILY);

			if numeric {
				font.features = card::number_features();
			}

			f32::from(
				text.shape_line(
					value.to_owned().into(),
					gpui::px(native.title_size()),
					&[TextRun { len: value.len(), font, ..Default::default() }],
					None,
				)
				.width,
			)
		};

		graph
			.nodes
			.iter()
			.filter_map(|node| {
				let row = node.row.as_ref()?;
				let (parent, team) = self.relation_node_relatives(node, graph);
				let mut width = 7. + measure(&self.relation_agent_number(row), true);

				if self.work_board.view.picked_agent.as_deref() == Some(&node.key) {
					if let Some(parent) = parent {
						width += 10. + native.title_size() + measure(&parent, false);
					}
					if let Some(team) = team {
						width += 7. + native.team_icon_size() + measure(&team.to_string(), true);
					}
				}
				if !node.assignment.is_empty() {
					width += 6. + measure(&node.assignment, false).min(native.caption_width());
				}

				Some((
					node.key.clone(),
					TextWidths {
						nameplate: (width + 1.) / native.zoom,
						speech: header::speech_width(&node.activity, native, text)
							.map(|width| width / native.zoom),
					},
				))
			})
			.collect()
	}

	fn relation_scope_layer(
		&self,
		mut canvas: Stateful<Div>,
		graph: &Graph,
		scale: f32,
		pan: (f32, f32),
	) -> Stateful<Div> {
		if self.work_board.view.presentation == layout::Presentation::Together {
			canvas = groups::paint(
				canvas,
				graph,
				scale,
				pan,
				self.work_board.view.picked_agent.as_deref(),
			);
		}

		let labels = self.relation_scope_labels(graph);

		for scope in &graph.scopes {
			let [left, top, right, bottom] = scope.bounds;
			let selected = self.relation_scope_selected(scope);
			let label = scope.label();
			let title_gap = labels
				.iter()
				.find(|(label_scope, _, _)| label_scope.id == scope.id)
				.map(|(_, rect, _)| rect)
				.filter(|rect| {
					(rect.y + scopes::HEADER_HEIGHT / 2. - (top * scale + pan.1)).abs() < 0.1
				})
				.map(|rect| {
					(
						rect.x - (left * scale + pan.0) - 5.,
						rect.x + rect.width - (left * scale + pan.0) + 5.,
					)
				});

			canvas = canvas.child(
				gpui::div()
					.id(SharedString::from(format!("project-{}", scope.id)))
					.debug_selector(|| "project-scope".into())
					.role(Role::Group)
					.aria_label(label.clone())
					.absolute()
					.left(gpui::px(left * scale + pan.0))
					.top(gpui::px(top * scale + pan.1))
					.w(gpui::px((right - left) * scale))
					.h(gpui::px((bottom - top) * scale))
					.child(scope.outline(scale, selected, title_gap)),
			);
		}

		canvas
	}

	fn relation_label_layer(
		&self,
		mut canvas: Stateful<Div>,
		graph: &Graph,
		lines: &[Link],
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;

		if graph.layout.points()
			|| self.work_board.view.presentation == layout::Presentation::Together
		{
			return canvas;
		}

		for link in lines {
			if link.detail <= 0.001
				|| (link.base.kind.hierarchy()
					&& self
						.work_board
						.view
						.edge
						.as_ref()
						.is_none_or(|edge| !edge.same_pair(&link.fact)))
			{
				continue;
			}

			let edge = link.fact.clone();
			let (a, b, path) = (&link.source, &link.target, &link.path);
			let label = graph.connection_label(&edge);
			let Some([left, top, _, _]) = graph.connection_label_bounds(&edge, path) else {
				continue;
			};
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
					.bg(ui_theme::rgba((COMPOSER_MATERIAL & 0xffffff00) | 0xe8))
					.opacity(
						link.detail
							* graph.layout.card_presence()
							* self.relation_wire_presence(std::time::Instant::now()),
					)
					.text_size(gpui::px(style::ConnectionLabel::FONT_SIZE * scale))
					.line_height(gpui::px(16. * scale))
					.text_color(ui_theme::rgb(edge.color))
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
							s.work_board.view.picked_project = None;
							s.workspace.agent_tree_visible = true;

							cx.stop_propagation();
							cx.notify();
						}),
					)
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.work_board.focus = None;
							s.handoffs.focus = None;
							s.work_board.view.camera_fixed = true;
							s.work_board.view.edge = Some(keyboard_edge.clone());
							s.work_board.view.picked_project = None;
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
						s.work_board.view.picked_project = None;
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
		mut canvas: Stateful<Div>,
		graph: &Graph,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let neighborhood = self.relation_neighborhood(graph);
		let layout = graph.layout.on_display(self.work_board.view.display_scale);

		for node in self.relation_nodes(graph) {
			let opacity = self.work_board.view.visibility.borrow().node_opacity(graph, node);

			if opacity <= 0.001 {
				continue;
			}

			let (emphasized, faded) = self.relation_node_emphasis(node, graph, &neighborhood);
			let faded =
				faded && self.work_board.view.presentation != layout::Presentation::Together;
			let (parent, team_members) = self.relation_node_relatives(node, graph);
			let interaction = self.relation_node_interaction(node, graph, cx);
			let view = interaction.keyboard(
				self.relation_node_surface(node, layout)
					.when(node.row.is_none(), |d| interaction.apply(d.opacity(opacity), true)),
			);

			canvas = canvas.child(Card {
				key: node.key.clone(),
				surface: view,
				content: self.relation_node_presentation(
					node,
					layout,
					emphasized,
					parent,
					team_members,
					&interaction,
				),
				faded,
				visibility: opacity,
				mascot_opacity: self
					.work_board
					.view
					.mascots
					.borrow()
					.get(&node.key)
					.map(mascot::Actor::opacity),
			});
		}

		canvas
	}

	fn relation_node_interaction(
		&self,
		node: &Node,
		graph: &Graph,
		cx: &mut Context<Self>,
	) -> card::Interaction {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let layout_viewport = self.work_board.view.layout_viewport;
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
		let dragkey = key.clone();
		let origin = if node.row.is_some() { node.connection_center() } else { (node.x, node.y) };
		let teamkey = key.clone();

		card::Interaction {
			team: std::rc::Rc::new(cx.listener(move |s, _, _, cx| {
				s.frame_relation_team(&teamkey, cx);
				cx.stop_propagation();
			})),
			drag: std::rc::Rc::new(cx.listener(move |s, e: &MouseDownEvent, _, cx| {
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
			})),
			click: std::rc::Rc::new(cx.listener(move |s, event: &ClickEvent, _, cx| {
				if s.work_board.view.moved && !matches!(event, gpui::ClickEvent::Keyboard(_)) {
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
					s.work_board.view.picked_project = None;
					s.work_board.view.edge = target.clone();
					s.workspace.agent_tree_visible = true;

					cx.notify();
				}
			})),
		}
	}

	fn update_relation_viewport(
		&mut self,
		bounds: gpui::Bounds<Pixels>,
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
		let reveal = resized && self.work_board.view.reveal_after_resize;

		if resized || self.work_board.view.node_count != node_count {
			self.work_board.view.node_count = node_count;
			self.work_board.view.viewport = size;

			if reveal {
				let graph = self.relation_graph();

				if let Some(node) = graph.nodes.iter().find(|node| {
					self.work_board.view.resize_anchor.as_ref().or(self
						.work_board
						.view
						.picked_agent
						.as_ref())
						== Some(&node.key)
				}) {
					self.work_board.view.pan = (
						size.0 / 2. - (node.x + node.width() / 2.) * scale,
						size.1 / 2. - (node.y + node.height() / 2.) * scale,
					);

					cx.notify();
				}

				if !self
					.workspace
					.sidebar_motion
					.borrow()
					.as_ref()
					.is_some_and(|motion| motion.moving(std::time::Instant::now()))
				{
					self.work_board.view.reveal_after_resize = false;
					self.work_board.view.resize_anchor = None;
				}
			} else if !self.work_board.view.camera_fixed {
				self.work_board.view.layout_viewport = size;

				self.frame_relations();
				cx.notify();
			}
		}
	}

	fn relation_connections_moving(&self) -> bool {
		self.work_board.view.wires.as_ref().is_some_and(|w| w.moving(std::time::Instant::now()))
			|| self
				.work_board
				.view
				.motion
				.as_ref()
				.is_some_and(|motion| motion.progress.moving(std::time::Instant::now()))
	}

	fn paint_relation_connections(
		&self,
		mut canvas: Stateful<Div>,
		graph: &Graph,
		lines: &[Link],
		moving: bool,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let moving = moving || self.relation_connections_moving();
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let flow_started = *self.work_board.view.flow_clock.get_or_init(std::time::Instant::now);
		let reduce_motion = ui_motion::reduced() || !ui_motion::animations_active();
		let paint_lines = lines.to_vec();
		let wire_presence = self.relation_wire_presence(std::time::Instant::now());
		let graph_moving = graph.layout.moving;
		let selected_path = self.relation_selected_path(graph);
		let hover_pending = self.work_board.view.hover.pending(std::time::Instant::now());
		let entity = cx.entity().downgrade();
		let node_count = graph.nodes.len();

		canvas = canvas.child(
			gpui::canvas(
				move |bounds, window, cx| {
					let _ = entity.update(cx, |s, cx| {
						frame_trace::graph_paint(
							scale,
							pan,
							(f32::from(bounds.size.width), f32::from(bounds.size.height)),
							s.workspace.arrival.progress(0.06),
						);

						s.sync_relation_density(window.scale_factor(), cx);

						s.update_relation_viewport(bounds, scale, node_count, cx)
					});
				},
				move |bounds, _, window, cx| {
					let mut animate = moving || hover_pending || graph_moving;
					let mut base = StrokeBatch::default();
					let mut detail = StrokeBatch::default();
					let mut flows = Vec::new();
					let mut highlights = BTreeMap::<u32, Ink>::new();

					paint_relation_grid(bounds, scale, pan, window);

					for link in paint_lines.iter().filter(|_| wire_presence > 0.001) {
						Self::paint_hierarchy_stroke(
							link,
							bounds.origin,
							scale,
							pan,
							wire_presence,
							&selected_path,
							&mut base,
						);

						if link.detail <= 0.001 {
							continue;
						}

						let reversed =
							(link.fact.from != link.base.from).then(|| link.path.reversed());
						let route = reversed.as_ref().unwrap_or(&link.path);
						let flowing = link.flowing() && !reduce_motion;

						if link.base.kind.hierarchy() {
							if flowing {
								flows.push((reversed, link.path.as_ref(), link.detail * 0.65));
							}

							continue;
						}

						let color = if link.active { INFO } else { link.fact.color };
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
							detail.push(
								path,
								(color << 8) | (0xdd as f32 * link.detail * wire_presence) as u32,
							);
						}

						if flowing {
							flows.push((reversed, link.path.as_ref(), link.detail));
						}
					}

					// Keep one clean stroke; selection and flow add no outer halo.
					base.paint(window);
					detail.paint(window);

					let phase = (flow_started.elapsed().as_secs_f32() / 2.8).fract();

					animate |= Self::paint_link_flows(
						flows,
						bounds.origin,
						scale,
						pan,
						phase,
						wire_presence,
						window,
					);

					if animate {
						ui_motion::request_frame(window, cx);
					}
				},
			)
			.absolute()
			.inset_0(),
		);

		canvas
	}

	fn sync_relation_density(&mut self, density: f32, cx: &mut Context<Self>) {
		if self.work_board.view.display_scale != density {
			self.work_board.view.display_scale = density;

			cx.notify();
		}
	}

	fn paint_link_flows(
		flows: Vec<(Option<Route>, &Route, f32)>,
		origin: Point<Pixels>,
		scale: f32,
		pan: (f32, f32),
		phase: f32,
		wire_presence: f32,
		window: &mut Window,
	) -> bool {
		let painted = !flows.is_empty();

		for (reversed, forward, opacity) in flows {
			let route = reversed.as_ref().unwrap_or(forward);

			route.paint_flow(origin, scale, pan, phase, opacity * wire_presence, window);
		}

		painted
	}

	fn paint_hierarchy_stroke(
		link: &Link,
		origin: Point<Pixels>,
		scale: f32,
		pan: (f32, f32),
		wire_presence: f32,
		selected_path: &BTreeMap<String, f32>,
		base: &mut StrokeBatch,
	) {
		let opacity = (1. - link.dim * 0.4 * (1. - link.detail)) * wire_presence;

		if let Ok(path) = link.path.stroke(link.base.kind, origin, scale, pan, 1.1, false).build() {
			base.push(
				path,
				((if selected_path.contains_key(&link.base.id) {
					SELECTED
				} else {
					link.base.color
				}) << 8)
					| (0xd0 as f32 * opacity) as u32,
			);
		}
	}

	fn relation_selected_path(&self, graph: &Graph) -> BTreeMap<String, f32> {
		groups::focus(
			graph,
			self.relation_context()
				.or_else(|| self.work_board.view.edge.as_ref().map(|edge| edge.to.as_str())),
		)
	}

	fn relation_node_surface(&self, node: &Node, layout: card::Layout) -> Stateful<Div> {
		let scale = self.relation_scale();
		let pan = self.work_board.view.pan;
		let layout = layout.on_display(self.work_board.view.display_scale);
		let name = format!("relation-node-{}", node.key);
		let preview = node.row.as_ref().map(|row| self.graph_preview(row));

		gpui::div()
			.id(SharedString::from(name.clone()))
			.debug_selector(move || name.clone())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!(
				"{} · {} · {}",
				node.row
					.as_ref()
					.map(|row| format!("{} · {}", self.relation_agent_number(row), row.title))
					.unwrap_or_else(|| node.title.clone()),
				node.status,
				preview
					.as_ref()
					.map(|(task, operation)| format!("{task} · {operation}"))
					.unwrap_or_default()
			))
			.absolute()
			.left(gpui::px(layout.pixel(node.x * scale + pan.0)))
			.top(gpui::px(layout.pixel(node.y * scale + pan.1)))
			.w(gpui::px(layout.pixel(node.width() * scale)))
			.h(gpui::px(layout.pixel(node.height() * scale)))
			.when(node.row.is_none(), |surface| {
				surface
					.rounded(gpui::px(3. * layout.zoom))
					.border(gpui::px(0.75 * scale))
					.border_color(ui_theme::rgb(LINE_STRONG))
					.bg(ui_theme::rgb(0x222228))
					.overflow_hidden()
			})
			.text_color(ui_theme::rgb(TEXT))
	}

	fn relation_node_presentation(
		&self,
		node: &Node,
		layout: card::Layout,
		selected: bool,
		parent: Option<String>,
		team_members: Option<usize>,
		interaction: &card::Interaction,
	) -> AnyElement {
		if node.row.is_some() && layout.overview() {
			card::overview(
				&node.key,
				node.color,
				selected,
				layout,
				(!layout.points()).then(|| {
					self.relation_node_content(
						node,
						layout,
						selected,
						parent,
						team_members,
						interaction,
					)
				}),
				interaction,
			)
		} else {
			self.relation_node_content(node, layout, selected, parent, team_members, interaction)
		}
	}

	fn relation_node_content(
		&self,
		node: &Node,
		layout: card::Layout,
		selected: bool,
		parent: Option<String>,
		team_members: Option<usize>,
		interaction: &card::Interaction,
	) -> AnyElement {
		let scale = self.relation_scale();
		let width = if node.row.is_some() { layout.width } else { node.width() * scale };
		let height = layout.pixel(
			if node.row.is_some() && layout.overview() { OVERVIEW_HEIGHT } else { node.height() }
				* scale,
		);
		let preview = node.row.as_ref().map(|_| node.assignment.clone());

		gpui::div()
			.relative()
			.w(gpui::px(width))
			.h(gpui::px(height))
			.px(gpui::px(if node.row.is_some() { layout.spacing(INSET_X) } else { 8. * scale }))
			.when(node.row.is_some(), |body| body.py(gpui::px(layout.spacing(INSET_Y))))
			.flex()
			.flex_col()
			.items_start()
			.when(layout.overview(), |body| body.justify_center())
			.gap(gpui::px(layout.gap()))
			.when_some(preview, |view, task| {
				let now = std::time::Instant::now();
				let status = node.row.as_ref().map(|row| row.status.as_str()).unwrap_or_default();
				let mut avatar = self
					.work_board
					.view
					.mascots
					.borrow_mut()
					.entry(node.key.clone())
					.or_insert_with(|| {
						mascot::Actor::new(
							&node.key,
							status,
							now,
							self.work_board.view.layers.actors.clone(),
						)
					})
					.frame(status, now);

				if self.work_board.view.presentation == layout::Presentation::Together
					&& let Some(motion) = &self.work_board.view.motion
				{
					let offset = node.connection_center();
					let target = (
						node.navigation.0 + offset.0 - node.x,
						node.navigation.1 + offset.1 - node.y,
					);

					if motion.progress.moving(now)
						&& motion
							.from
							.get(&node.key)
							.is_some_and(|p| (p.0 - target.0).hypot(p.1 - target.1) > 1.)
					{
						avatar =
							avatar.travelling(now.duration_since(motion.started).as_secs_f32());
					}
				}

				view.child(Header {
					reviewer: node
						.row
						.as_ref()
						.is_some_and(|row| self.reviewer_role(row).is_some()),
					team_members,
					key: node.key.clone(),
					parent,
					number: node
						.row
						.as_ref()
						.map(|row| self.relation_agent_number(row))
						.unwrap_or_default(),
					color: node.color,
					selected,
					picked: self.work_board.view.picked_agent.as_deref() == Some(node.key.as_str()),
					live: node.row.as_ref().is_some_and(|row| row.group == 1),
					task,
					avatar,
					metrics: (!layout.overview()).then(|| self.node_metrics(node, layout)),
					activity: node.activity.clone(),
					layout,
					interaction: interaction.clone(),
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
			.into_any_element()
	}

	pub(super) fn relation_agent_number(&self, row: &Row) -> String {
		self.snapshot
			.as_ref()
			.and_then(|snapshot| snapshot.agent_slots.get(&row.key))
			.map_or_else(|| "—".into(), |slot| format!("A{slot}"))
	}

	/// Agent identity follows explicit project and role metadata, never prompt prose.
	pub(in super::super) fn relation_agent_name(&self, row: &Row) -> String {
		if row.native && !row.owner.trim().is_empty() {
			return row.owner.clone();
		}

		let snapshot = self.snapshot.as_ref();
		let work = snapshot
			.and_then(|snapshot| snapshot.work_items.iter().find(|work| work.id == row.work));

		if let Some(agent) = snapshot.and_then(|s| s.agents.iter().find(|a| a.id == row.work)) {
			return agent.name.clone();
		}

		if work.is_some_and(|work| work.parent_goal_id.is_none()) {
			return "Assistant".into();
		}
		if work.is_some_and(|work| work.kind == AgentWorkKindDto::Manager) {
			"Agent".into()
		} else {
			"Legacy execution".into()
		}
	}

	fn relation_node_current(&self, node: &Node) -> bool {
		node.row.as_ref().is_some_and(|row| self.relation_row_current(row))
	}

	fn relation_row_current(&self, row: &Row) -> bool {
		if let Some(picked) = &self.work_board.view.picked_agent {
			return picked == &row.key;
		}

		if row.native {
			self.native_agents.selected.as_ref().is_some_and(|(owner, thread)| {
				owner == &row.work && row.thread.as_ref() == Some(thread)
			})
		} else {
			self.native_agents.selected.is_none() && self.selected.as_ref() == Some(&row.work)
		}
	}

	fn relation_node_emphasis(
		&self,
		node: &Node,
		graph: &Graph,
		neighborhood: &BTreeSet<String>,
	) -> (bool, bool) {
		let project = self
			.work_board
			.view
			.picked_project
			.as_ref()
			.and_then(|picked| graph.scopes.iter().find(|scope| &scope.id == picked));
		let member = project.is_some_and(|scope| scope.members.contains(&node.key));
		let linked = self.work_board.view.hover.current(std::time::Instant::now())
			== Some(node.key.as_str())
			|| self.work_board.view.picked_agent.as_ref() == Some(&node.key);
		let selected = member
			|| linked
			|| self.work_board.view.picked_agent.as_ref().is_some_and(|picked| {
				graph.edges.iter().any(|edge| {
					edge.kind.hierarchy() && edge.to == *picked && edge.from == node.key
				})
			})
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
				&& project.is_none()
				&& self.relation_node_current(node));
		let faded = project.is_some() && !member
			|| self.relation_context().is_some() && !neighborhood.contains(&node.key)
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
		self.zoom_relations_at(old, next, center, std::time::Instant::now());
	}

	fn zoom_relations_at(
		&mut self,
		old: f32,
		next: f32,
		center: Option<(f32, f32)>,
		now: std::time::Instant,
	) {
		let center = center.unwrap_or((
			self.work_board.view.viewport.0 / 2.,
			self.work_board.view.viewport.1 / 2.,
		));
		let old_layout = card::Layout::new(old);
		let next_layout = card::Layout::new(next.clamp(MIN_ZOOM, MAX_ZOOM));
		let anchor = (old_layout.overview() != next_layout.overview()
			|| old_layout.full_detail() != next_layout.full_detail())
		.then(|| {
			self.relation_graph().nodes.into_iter().find_map(|node| {
				let rect = node.screen_rect(old, self.work_board.view.pan);

				rect.contains(center.0, center.1).then(|| {
					(
						node.key,
						((center.0 - rect.x) / rect.width, (center.1 - rect.y) / rect.height),
					)
				})
			})
		})
		.flatten();
		let presence = self.relation_layout_at(now).card_presence();
		let transition = self
			.work_board
			.view
			.card_transition
			.get_or_insert_with(|| crate::ui_motion::Tween::new(presence));

		transition.target(next_layout.card_presence(), now);
		self.work_board.view.zoom_around(old, next, center);

		if let Some((key, relative)) = anchor
			&& let Some(node) = self.relation_graph().nodes.iter().find(|node| node.key == key)
		{
			self.work_board.view.pan = (
				center.0 - (node.x + node.width() * relative.0) * next_layout.zoom,
				center.1 - (node.y + node.height() * relative.1) * next_layout.zoom,
			);
		}
	}

	fn arrange_relations(&mut self) {
		self.work_board.view.layout.replace(Default::default());

		self.work_board.view.motion = None;

		self.work_board.view.positions.clear();

		self.work_board.view.drag = None;
		self.work_board.view.moved = false;
		self.work_board.view.camera_fixed = true;
		self.work_board.view.layout_viewport = self.work_board.view.viewport;

		self.frame_relations();
	}

	pub(super) fn fit_relations(&mut self) {
		self.work_board.view.picked_project = None;
		self.frame_relations();
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	#[cfg(feature = "visual-capture")]
	#[allow(dead_code)] // Only the isolated native capture driver selects a test layout.
	pub(crate) fn visual_relation_layout(&mut self, name: &str, cx: &mut Context<Self>) {
		let mode = layout::Mode::ALL
			.into_iter()
			.find(|mode| mode.label().eq_ignore_ascii_case(name))
			.expect("known capture layout");

		self.switch_relation_layout(mode, cx);
	}

	#[allow(dead_code)] // Only the isolated native capture driver sets a test camera.
	pub(crate) fn visual_relation_zoom(&mut self, scale: f32, cx: &mut Context<Self>) {
		self.work_board.view.camera_fixed = true;

		self.frame_relations();

		let old = self.relation_scale();

		self.zoom_relations(old, scale, None);

		self.work_board.view.card_transition = None;

		cx.notify();
	}

	#[cfg(feature = "visual-capture")]
	#[allow(dead_code)] // The isolated driver can inspect a complete working team.
	pub(crate) fn visual_relation_team(&mut self, key: &str, cx: &mut Context<Self>) {
		assert!(self.factory_rows().iter().any(|row| row.key == key), "known capture team");

		self.frame_relation_team(key, cx);
	}

	#[cfg(feature = "visual-capture")]
	#[allow(dead_code)] // A small speech capture frames its speaker, not the complete team.
	pub(crate) fn visual_relation_agent(&mut self, key: &str, cx: &mut Context<Self>) {
		self.locate_relation_agent(key, cx);
	}

	#[cfg(feature = "visual-capture")]
	#[allow(dead_code)]
	pub(crate) fn visual_relation_presentation(&mut self, together: bool, cx: &mut Context<Self>) {
		self.switch_relation_presentation(
			if together { layout::Presentation::Together } else { layout::Presentation::Links },
			cx,
		);
	}

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
				model: None,
				reasoning_effort: None,
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

		self.work_board.briefs.insert(row.key.clone(),Brief{review:None,output:None,source_thread:row.thread.clone(),assignments:Default::default(),outcome:None,preview:None,question:None,metrics:Default::default(),resources:None,stamp:self.brief_stamp(&row),read_at:std::time::Instant::now(),relations:vec![("review-wait".into(),decodex_protocol::AgentCollaborationDto { model: None, reasoning_effort: None,sender_thread_id:"fixture-release".into(),receiver_thread_ids:vec!["fixture-reviewer".into()],tool:"wait".into(),status:"completed".into(),prompt:String::new(),results:vec![decodex_protocol::AgentCollaborationResultDto{thread_id:"fixture-reviewer".into(),status:"completed".into(),message:"Cancellation can leave the sign-in button disabled. Add a regression test before release.".into()}]})]});

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
                payload_omitted: false, created_at: 1_791_424_800,
            };

			self.work_board.briefs.insert(
				row.key.clone(),
				Brief {
					review: None,
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

fn snap(value: f32) -> f32 {
	(value / GRID).round() * GRID
}

fn grid_spacing(scale: f32) -> f32 {
	// Omit minor dots at distant zoom levels, preserving the same world-space lattice.
	GRID * scale * (8. / (GRID * scale)).max(1.).log2().ceil().exp2()
}

fn paint_relation_grid(
	bounds: gpui::Bounds<Pixels>,
	scale: f32,
	pan: (f32, f32),
	window: &mut Window,
) {
	let spacing = grid_spacing(scale);

	// GPUI snaps these square dots to physical pixels. Rounding a one-point
	// dot softens all four pixels on a Retina display.
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
					ui_theme::rgba(0xffffff18),
				));

				y += spacing;
			}

			x += spacing;
		}
	});
}

#[cfg(test)]
mod fit_tests {
	use std::{slice, thread};

	use gpui::AppContext;

	#[cfg(any(test, feature = "visual-capture"))]
	use crate::shell::agent_surface::work_board::relations::Brief;
	use crate::{
		shell::agent_surface::work_board::{
			relations::{
				self, AgentSurface, BTreeMap, BTreeSet, Context, Drag, FRAME_INSET, GRID, Graph,
				Kind, MouseButton, Node, Route, TEXT, View, card, groups, layout, routing, style,
				visibility,
			},
			tests,
		},
		ui_theme,
	};

	#[gpui::test]
	fn fullscreen_restore_keeps_an_agent_visible_after_manual_zoom(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, cx| {
			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;
			s.work_board.view.camera_fixed = true;
			s.work_board.view.viewport = (1_200., 900.);
			s.work_board.view.layout_viewport = (1_200., 900.);
			s.work_board.view.zoom = Some(0.7);
			s.work_board.view.pan = (200., 500.);

			s.toggle_graph_fullscreen(cx);

			assert!(!s.workspace.graph_expanded);

			let anchor = s.work_board.view.resize_anchor.clone().expect("visible context");
			let count = s.relation_graph().nodes.len();

			s.update_relation_viewport(
				gpui::Bounds::new(
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(800.), gpui::px(240.)),
				),
				0.7,
				count,
				cx,
			);

			let graph = s.relation_graph();
			let node = graph.nodes.iter().find(|node| node.key == anchor).unwrap();
			let rect = node.screen_rect(0.7, s.work_board.view.pan);

			assert!(rect.x >= 0. && rect.x + rect.width <= 800.);
			assert!(rect.y >= 0. && rect.y + rect.height <= 240.);
			assert_eq!(s.work_board.view.zoom, Some(0.7));
			assert!(!s.work_board.view.reveal_after_resize);
		});
	}

	#[gpui::test]
	fn names_follow_roles_and_explicit_projects_instead_of_changing_prompts(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, _| {
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "release")
				.unwrap()
				.kind = decodex_protocol::AgentWorkKindDto::Manager;

			let mut row = s.factory_rows().into_iter().find(|row| row.key == "release").unwrap();

			row.workspace = None;

			let name = s.relation_agent_name(&row);

			assert_eq!(name, "Agent");

			row.title = "A completely different and much longer request for this project".into();

			assert_eq!(s.relation_agent_name(&row), name);

			s.snapshot.as_mut().unwrap().projects.push(decodex_protocol::ProjectDto {
				native_project_id: None,
				lead_agent_id: None,
				id: "named-project".into(),
				name: "Decodex".into(),
				directory: "/tmp/decodex".into(),
				agent_ids: vec![row.work.clone()],
			});

			row.workspace = Some("named-project".into());

			assert_eq!(
				s.relation_agent_name(&row),
				"Agent",
				"Project membership does not rename an independent Agent"
			);

			row.native = true;
			row.owner = "Euler".into();

			assert_eq!(s.relation_agent_name(&row), "Euler");

			let main = s.factory_rows().into_iter().find(|row| row.key == "agent").unwrap();

			assert_eq!(s.relation_agent_name(&main), "Assistant");
		});
	}

	#[gpui::test]
	fn messages_use_one_slot_without_replaying_history_or_expiring_the_last_reply(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, _| {
			s.work_board.view.zoom = Some(1.);
			s.work_board.view.picked_agent = Some("agent".into());

			let now = std::time::Instant::now();
			let row = s.factory_rows().into_iter().find(|row| row.key == "release").unwrap();
			let (_, operations) = s.graph_operations(&row);

			assert!(operations.is_empty());

			let empty = s
				.relation_graph_at(now)
				.nodes
				.into_iter()
				.find(|node| node.key == row.key)
				.unwrap();
			let brief = s.work_board.briefs.entry(row.key.clone()).or_insert_with(|| Brief {
				review: None,
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: None,
				question: None,
				resources: None,
				relations: vec![],
				metrics: Default::default(),
				stamp: String::new(),
				read_at: now,
			});

			brief.preview = Some(super::super::activity::Preview {
				turn: "footprint".into(),
				task: row.title.clone(),
				text: "Check".into(),
				running: None,
				recent: (0..3)
					.map(|i| super::super::activity::Operation {
						id: format!("step-{i}"),
						text: format!("Check {i}"),
						kind: super::super::activity::OperationKind::Action,
					})
					.collect(),
			});

			let arrival = now + std::time::Duration::from_secs(1);

			s.relation_graph_at(arrival);

			let three = s
				.relation_graph_at(arrival + std::time::Duration::from_millis(300))
				.nodes
				.into_iter()
				.find(|node| node.key == row.key)
				.unwrap();

			assert_eq!(three.activity.lines.len(), 1);
			assert_eq!(three.activity.lines[0].text, "Check 2");
			assert!(three.height() >= empty.height());

			let idle = arrival + std::time::Duration::from_secs(7);

			for i in 0..4 {
				s.relation_graph_at(idle + std::time::Duration::from_millis(i * 300));
			}

			let settled = s
				.relation_graph_at(idle + std::time::Duration::from_millis(1_500))
				.nodes
				.into_iter()
				.find(|node| node.key == row.key)
				.unwrap();

			assert_eq!(settled.activity.lines.len(), 1);
			assert_eq!(
				settled.height(),
				three.height(),
				"quiet time does not erase or reflow the last reply"
			);
			assert_eq!(settled.activity.height, three.activity.height);
		});
	}

	#[gpui::test]
	fn changing_detail_tiers_keeps_the_pointed_agent_under_the_pointer(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, _| {
			s.work_board.view.zoom = Some(1.);

			let center = (350., 240.);

			for next in [0.6, 0.25, 0.19, 0.05, 0.2, 0.8, 1.] {
				let old = s.relation_scale();
				let node = s
					.relation_graph()
					.nodes
					.into_iter()
					.find(|node| node.key == "release")
					.unwrap();

				s.work_board.view.pan = (
					center.0 - (node.x + node.width() / 2.) * old,
					center.1 - (node.y + node.height() / 2.) * old,
				);

				s.zoom_relations(old, next, Some(center));

				let node = s
					.relation_graph()
					.nodes
					.into_iter()
					.find(|node| node.key == "release")
					.unwrap();
				let rect = node.screen_rect(next, s.work_board.view.pan);

				assert!((rect.x + rect.width / 2. - center.0).abs() < 0.01);
				assert!((rect.y + rect.height / 2. - center.1).abs() < 0.01);
			}
		});
	}

	#[gpui::test]
	fn graph_control_groups_share_one_row_and_equal_gutters(cx: &mut gpui::TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, cx| tests::fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_000.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);

		let groups =
			["relations-layout-controls", "relations-zoom-controls", "relations-pane-controls"]
				.map(|selector| visual.debug_bounds(selector).expect(selector));

		for pair in groups.windows(2) {
			assert_eq!(pair[0].center().y, pair[1].center().y);
			assert_eq!(pair[0].size.height, pair[1].size.height);
			assert!((f32::from(pair[1].left() - pair[0].right()) - 8.).abs() <= 0.5);
		}
	}

	#[gpui::test]
	fn layouts_and_map_navigation_preserve_the_main_chat_and_collaboration(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, cx| {
			s.visual_workspace_page("workbench", cx);
			s.visual_relation_density(103, cx);

			s.work_board.view.viewport = (815., 683.);
			s.work_board.view.layout_viewport = (815., 683.);
			s.work_board.view.zoom = Some(1.);

			s.composer.update(cx, |input, cx| input.set_content("Preserve the Main draft", cx));

			let main = (s.selected.clone(), s.native_agents.selected.clone());
			let collaboration = s.work_board.collaboration_expanded.clone();
			let original = s.relation_graph();
			let identities: BTreeSet<_> = original.nodes.iter().map(|n| n.key.clone()).collect();
			let edges: BTreeSet<_> = original
				.edges
				.iter()
				.map(|e| (e.id.clone(), e.from.clone(), e.to.clone()))
				.collect();

			for mode in layout::Mode::ALL {
				s.switch_relation_layout(mode, cx);

				s.work_board.view.motion = None;

				s.navigate_relation_agent("density-006", cx);

				s.work_board.view.motion = None;

				let graph = s.relation_graph();

				assert_eq!(
					graph.nodes.iter().map(|n| n.key.clone()).collect::<BTreeSet<_>>(),
					identities
				);
				assert_eq!(
					graph
						.edges
						.iter()
						.map(|e| (e.id.clone(), e.from.clone(), e.to.clone()))
						.collect::<BTreeSet<_>>(),
					edges
				);
				assert_eq!(s.work_board.view.zoom, Some(1.));
				assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("density-006"));

				let node = graph.nodes.iter().find(|n| n.key == "density-006").unwrap();
				let r = node.screen_rect(1., s.work_board.view.pan);

				assert!((r.x + r.width / 2. - 815. / 2.).abs() < 0.01);
				assert!((r.y + r.height / 2. - 683. / 2.).abs() < 0.01);
				assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), main);
				assert_eq!(s.work_board.collaboration_expanded, collaboration);
				assert_eq!(s.composer.read(cx).content(), "Preserve the Main draft");
			}
		});
	}

	#[gpui::test]
	fn large_graph_map_is_below_the_toolbar_and_its_owner_labels_are_clickable(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = tests::fixture(cx);

			s.visual_workspace_page("workbench", cx);
			s.visual_relation_density(103, cx);

			s
		});

		visual.simulate_resize(gpui::size(gpui::px(1_536.), gpui::px(1_024.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		surface.update(visual, |s, cx| s.visual_relation_zoom(1., cx));
		visual.update(crate::ui_frame::draw_frame);

		let map = visual.debug_bounds("relations-minimap").expect("large graph map");
		let canvas = visual.debug_bounds("relations-canvas").unwrap();

		assert!((f32::from(map.left() - canvas.left()) - 12.).abs() < 0.01);
		assert!((f32::from(map.top() - canvas.top()) - 12.).abs() < 0.01);

		let main = surface.read_with(visual, |s, _| s.selected.clone());
		let target =
			visual.debug_bounds("relations-map-agent-density-001").expect("project shortcut");

		visual.simulate_click(target.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("density-001"));
			assert_eq!(s.selected, main);
			assert_eq!(s.work_board.view.zoom, Some(1.));
		});

		for mode in layout::Mode::ALL {
			surface.update(visual, |s, cx| {
				s.switch_relation_layout(mode, cx);

				s.work_board.view.motion = None;
			});

			visual.update(crate::ui_frame::draw_frame);

			let map = visual.debug_bounds("relations-minimap").unwrap();
			let root = visual.debug_bounds("relations-map-agent-agent").expect("root shortcut");

			assert!(root.left() >= map.left() && root.right() <= map.right());
			assert!(root.top() >= map.top() && root.bottom() <= map.bottom());

			visual.simulate_mouse_move(root.center(), None, Default::default());

			assert_eq!(
				visual.debug_bounds("relations-map-agent-agent").unwrap(),
				root,
				"the minimap target stays still under the pointer in {mode:?}"
			);

			visual.simulate_click(root.center(), Default::default());
			surface.read_with(visual, |s, _| {
				assert_eq!(
					s.work_board.view.picked_agent.as_deref(),
					Some("agent"),
					"{mode:?}: {root:?}"
				);
				assert_eq!(s.selected, main);
				assert_eq!(s.work_board.view.zoom, Some(1.));
			});
		}
	}

	#[gpui::test]
	fn locating_a_fitting_team_reveals_all_members_without_changing_the_chat_or_zoom(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, cx| {
			s.visual_workspace_page("workbench", cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep this Main draft", cx));

			let main = (s.selected.clone(), s.native_agents.selected.clone());

			for mode in layout::Mode::ALL {
				for presentation in [layout::Presentation::Links, layout::Presentation::Together] {
					let view = &mut s.work_board.view;

					view.layout_mode = mode;
					view.presentation = presentation;
					view.viewport = (1_400., 900.);
					view.layout_viewport = view.viewport;
					view.camera_fixed = true;
					view.zoom = Some(0.6);
					view.pan = (-10_000., -10_000.);

					let before = s.relation_graph();
					let shape = |g: &Graph| {
						g.nodes
							.iter()
							.map(|n| (n.key.clone(), n.x, n.y, n.size))
							.collect::<Vec<_>>()
					};

					s.locate_relation_agent("mira", cx);

					let after = s.relation_graph();

					assert_eq!(shape(&after), shape(&before));

					let members = groups::descendants(&after, "mira");

					assert!(members.len() > 1);

					for node in after.nodes.iter().filter(|n| members.contains_key(&n.key)) {
						let rect = node.screen_rect(0.6, s.work_board.view.pan);

						assert!(
							rect.x >= FRAME_INSET && rect.y >= FRAME_INSET,
							"{mode:?} / {presentation:?}: {rect:?}"
						);
						assert!(
							rect.x + rect.width <= 1_400. - FRAME_INSET
								&& rect.y + rect.height <= 900. - FRAME_INSET
						);
					}

					assert_eq!(s.work_board.view.zoom, Some(0.6));
					assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), main);
					assert_eq!(s.composer.read(cx).content(), "Keep this Main draft");
				}
			}
		});
	}

	#[gpui::test]
	fn a_hundred_agents_keep_their_ids_topology_and_geometry_across_zoom(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = tests::fixture(cx);

			surface.visual_workspace_page("workbench", cx);
			surface.visual_relation_density(100, cx);

			surface
		});

		visual.simulate_resize(gpui::size(gpui::px(1_600.), gpui::px(1_000.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		surface.update(visual, |s, cx| s.visual_relation_zoom(1., cx));
		visual.update(crate::ui_frame::draw_frame);

		let geometry = |s: &AgentSurface| {
			s.relation_graph()
				.nodes
				.into_iter()
				.map(|node| (node.key, node.x, node.y, node.size))
				.collect::<Vec<_>>()
		};
		let before = surface.read_with(visual, |s, _| geometry(s));

		for scale in [
			1.,
			0.9,
			0.8,
			0.7 - f32::EPSILON,
			0.7,
			0.6,
			0.45 - f32::EPSILON,
			0.45,
			0.35,
			0.25,
			0.2,
			0.19,
			0.1,
			0.05,
			0.2,
			1.,
		] {
			surface.update(visual, |s, cx| {
				s.visual_relation_zoom(scale, cx);

				if (scale * 100.).round() >= 70. {
					assert_eq!(geometry(s), before, "full-detail geometry is reversible");
				}

				let graph = s.relation_graph();

				assert_eq!(graph.nodes.len(), 100);
				assert_eq!(graph.edges.len(), 99);

				let messages =
					&graph.nodes.iter().find(|node| node.key == "agent").unwrap().activity.lines;

				assert_eq!(messages.len(), 1, "retain the quiet agent's one native reply");
				assert_eq!(messages[0].kind, super::super::activity::OperationKind::Message);

				let slots: BTreeSet<_> =
					s.factory_rows().iter().map(|row| s.relation_agent_number(row)).collect();

				assert_eq!(slots.len(), 100);
				assert!(slots.contains("A99"));
				assert_eq!(s.work_statistics().unwrap().samples.len(), 100);
			});

			visual.update(crate::ui_frame::draw_frame);

			assert!(visual.debug_bounds("graph-usage-segment-density-099").is_some());

			let layout = card::Layout::new(scale);

			assert_eq!(visual.debug_bounds("relation-metrics-agent").is_some(), !layout.overview());
			assert_eq!(
				visual.debug_bounds("relation-preview-agent").is_some(),
				layout.activity_visible(),
				"show one retained reply only at readable zoom"
			);
			assert_eq!(visual.debug_bounds("relation-tokens-agent").is_some(), !layout.overview());
			assert_eq!(visual.debug_bounds("relation-number-agent").is_some(), layout.labels());
			assert_eq!(visual.debug_bounds("relation-status-agent").is_some(), layout.labels());
			assert_eq!(
				visual.debug_bounds("relation-point-agent").is_some(),
				layout.card_presence() < 1.
			);
			assert_eq!(
				visual.debug_bounds("relation-assignment-agent").is_some(),
				layout.activity_visible()
			);
			assert!(surface.read_with(visual, |s, _| s.relation_scale()) >= card::MIN_ZOOM);
		}
	}

	#[gpui::test]
	fn empty_footprints_and_text_pan_the_canvas_while_portraits_move_the_agent(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = tests::fixture(cx);

			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;

			s.composer.update(cx, |input, cx| input.set_content("Keep the Main draft", cx));

			s
		});

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_000.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();

		let main = surface.read_with(visual, |s, _| s.selected.clone());

		for mode in layout::Mode::ALL {
			for scale in [0.5, 0.75, 1.] {
				surface.update(visual, |s, cx| {
					s.switch_relation_layout(mode, cx);

					s.work_board.view.motion = None;

					s.work_board.view.positions.clear();
					s.visual_relation_zoom(scale, cx);

					let graph = s.relation_graph();
					let node = graph.nodes.iter().find(|n| n.key == "release").unwrap();
					let view = &mut s.work_board.view;

					view.picked_agent = None;
					view.pan = (
						view.viewport.0 / 2. - (node.x + node.width() / 2.) * scale,
						view.viewport.1 / 2. - (node.y + node.height() / 2.) * scale,
					);

					for (i, other) in graph.nodes.iter().filter(|n| n.key != "release").enumerate()
					{
						view.positions
							.insert(other.key.clone(), (10_000. + i as f32 * 1_000., 10_000.));
					}
				});

				visual.update(crate::ui_frame::draw_frame);

				let avatar = visual.debug_bounds("relation-avatar-release").unwrap();
				let gap = gpui::point(avatar.right() + gpui::px(5.), avatar.center().y);

				visual.simulate_click(gap, Default::default());
				surface.read_with(visual, |s, _| {
					assert!(
						s.work_board.view.picked_agent.is_none(),
						"empty space is not an agent"
					);
				});

				for target in ["gap", "padding", "identity", "message"] {
					let avatar = visual.debug_bounds("relation-avatar-release").unwrap();
					let bounds = visual.debug_bounds("relation-node-release").unwrap();
					let start = match target {
						"gap" => gpui::point(avatar.right() + gpui::px(5.), avatar.center().y),
						"padding" => gpui::point(avatar.center().x, bounds.bottom() - gpui::px(1.)),
						"identity" =>
							visual.debug_bounds("relation-number-release").unwrap().center(),
						_ => {
							let Some(message) = visual.debug_bounds("relation-preview-release")
							else {
								continue;
							};

							message.center()
						},
					};
					let (positions, pan) = surface.read_with(visual, |s, _| {
						(s.work_board.view.positions.clone(), s.work_board.view.pan)
					});
					let end = start + gpui::point(gpui::px(18.), gpui::px(12.));

					visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
					visual.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
					visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
					surface.read_with(visual, |s, _| {
						assert_eq!(s.work_board.view.positions, positions, "{mode:?} at {scale}");
						assert_eq!(s.work_board.view.pan, (pan.0 + 18., pan.1 + 12.));
						assert!(s.work_board.view.picked_agent.is_none());
					});
					visual.update(crate::ui_frame::draw_frame);
				}

				let identity = visual.debug_bounds("relation-number-release").unwrap();

				visual.simulate_click(identity.center(), Default::default());
				surface.read_with(visual, |s, cx| {
					assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("release"));
					assert_eq!(s.selected, main);
					assert_eq!(s.composer.read(cx).content(), "Keep the Main draft");
				});
				visual.update(crate::ui_frame::draw_frame);

				let avatar = visual.debug_bounds("relation-avatar-release").unwrap();
				let pan = surface.read_with(visual, |s, _| s.work_board.view.pan);
				let end = avatar.center() + gpui::point(gpui::px(18.), gpui::px(12.));

				visual.simulate_mouse_down(avatar.center(), MouseButton::Left, Default::default());
				visual.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
				visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
				surface.read_with(visual, |s, _| {
					assert_eq!(s.work_board.view.pan, pan, "portrait drag keeps the camera still");
					assert!(s.work_board.view.positions.contains_key("release"));
				});
			}
		}
	}

	#[gpui::test]
	fn zoom_controls_reach_selectable_and_draggable_points_in_every_layout(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = tests::fixture(cx);

			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;

			s.composer.update(cx, |input, cx| input.set_content("Keep the Main draft", cx));

			s
		});

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_000.)));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();

		let main = surface
			.read_with(visual, |s, _| (s.selected.clone(), s.native_agents.selected.clone()));

		for mode in layout::Mode::ALL {
			surface.update(visual, |s, cx| {
				s.switch_relation_layout(mode, cx);

				s.work_board.view.motion = None;

				s.visual_relation_zoom(0.25, cx);
			});

			for _ in 0..5 {
				visual.update(crate::ui_frame::draw_frame);

				let out = visual.debug_bounds("relations-out").unwrap();

				visual.simulate_click(out.center(), Default::default());
			}

			thread::sleep(std::time::Duration::from_millis(230));

			surface.update(visual, |s, _| {
				assert_eq!(s.relation_scale(), 0.05);

				s.work_board.view.picked_agent = None;

				let graph = s.relation_graph();
				let node = graph.nodes.iter().find(|n| n.key == "release").unwrap();
				let view = &mut s.work_board.view;

				view.pan = (
					view.viewport.0 / 2. - (node.x + node.width() / 2.) * 0.05,
					view.viewport.1 / 2. - (node.y + node.height() / 2.) * 0.05,
				);
			});

			visual.update(crate::ui_frame::draw_frame);

			let dot = visual.debug_bounds("relation-point-release").expect("point at 5%");

			assert_eq!(dot.size, gpui::size(gpui::px(3.), gpui::px(3.)));

			visual.simulate_click(dot.center(), Default::default());
			surface.read_with(visual, |s, cx| {
				assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("release"));
				assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), main);
				assert_eq!(s.composer.read(cx).content(), "Keep the Main draft");
			});

			for _ in 0..2 {
				visual.update(crate::ui_frame::draw_frame);

				let dot = visual.debug_bounds("relation-point-release").unwrap();
				let start = dot.center();
				let end = start + gpui::point(gpui::px(16.), gpui::px(16.));
				let before = surface.read_with(visual, |s, _| {
					let node =
						s.relation_graph().nodes.into_iter().find(|n| n.key == "release").unwrap();

					node.connection_center()
				});

				visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
				visual.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
				visual.simulate_mouse_up(end, MouseButton::Left, Default::default());

				surface.read_with(visual, |s, _| {
					let graph = s.relation_graph();
					let node = graph.nodes.iter().find(|n| n.key == "release").unwrap();
					let expected =
						(relations::snap(before.0 + 320.), relations::snap(before.1 + 320.));
					let center = node.connection_center();

					assert!(
						(center.0 - expected.0).abs() < 0.01
							&& (center.1 - expected.1).abs() < 0.01,
						"{mode:?}: drag from {before:?} to {expected:?}, got ({}, {}), picked {:?}",
						node.x,
						node.y,
						s.work_board.view.picked_agent
					);
				});
			}
		}
	}

	#[gpui::test]
	fn point_routes_attach_to_the_visible_dot_and_keep_centers_across_zoom(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, cx| {
			s.visual_relation_density(103, cx);

			for mode in layout::Mode::ALL {
				s.work_board.view.layout_mode = mode;
				s.work_board.view.zoom = Some(0.2);

				let centers: BTreeMap<_, _> = s
					.relation_graph()
					.nodes
					.into_iter()
					.map(|n| (n.key.clone(), (n.x + n.width() / 2., n.y + n.height() / 2.)))
					.collect();

				for zoom in [0.16, 0.15, 0.05] {
					s.work_board.view.zoom = Some(zoom);

					let graph = s.relation_graph();

					for node in &graph.nodes {
						let center = centers[&node.key];

						assert!((node.x + node.width() / 2. - center.0).abs() < 0.01);
						assert!((node.y + node.height() / 2. - center.1).abs() < 0.01);
					}
					for edge in graph.connections() {
						let path = &graph.paths[&edge.id];

						for (key, point) in [(&edge.from, path.at(0.)), (&edge.to, path.at(1.))] {
							let center = centers[key];
							let radius = card::Layout::new(zoom).point_size() / 2.;

							assert!(
								((point.0 - center.0).hypot(point.1 - center.1) - radius).abs()
									< 0.01
							);
						}
					}
				}
			}
		});
	}

	#[gpui::test]
	fn point_card_morph_keeps_centers_routes_and_manual_positions(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, cx| {
			s.visual_relation_density(103, cx);
			s.work_board.view.positions.insert("density-006".into(), (144., -180.));

			let now = std::time::Instant::now();

			for mode in layout::Mode::ALL {
				s.work_board.view.layout_mode = mode;

				let baseline = s.relation_graph_with_layout(now, card::Layout::new(0.25));

				for (zoom, presence) in [
					(0.05, 0.),
					(0.15, 0.),
					(0.2, 0.),
					(0.2, 0.2),
					(0.2, 0.5),
					(0.2, 0.8),
					(0.2, 1.),
					(0.24, 1.),
					(0.2, 0.3),
				] {
					let graph = s.relation_graph_with_layout(
						now,
						card::Layout::new(zoom)
							.with_presence(presence, presence > 0. && presence < 1.),
					);

					for (node, original) in graph.nodes.iter().zip(&baseline.nodes) {
						assert_eq!(node.key, original.key);
						assert!(
							(node.x + node.width() / 2. - original.x - original.width() / 2.).abs()
								< 0.01
						);
						assert!(
							(node.y + node.height() / 2. - original.y - original.height() / 2.)
								.abs()
								< 0.01
						);
					}
					for edge in graph.connections() {
						let path = &graph.paths[&edge.id];

						for (key, point) in [(&edge.from, path.at(0.)), (&edge.to, path.at(1.))] {
							let node = graph.nodes.iter().find(|n| &n.key == key).unwrap();
							let (x, y) = (point.0 - node.x, point.1 - node.y);

							assert!(x >= -0.01 && x <= node.width() + 0.01);
							assert!(y >= -0.01 && y <= node.height() + 0.01);

							let [left, top, right, bottom] = node.connection_bounds();

							assert!(
								(point.0 - left).abs() < 0.01
									|| (point.0 - right).abs() < 0.01
									|| (point.1 - top).abs() < 0.01
									|| (point.1 - bottom).abs() < 0.01,
								"the morph must attach to the visible character or dot"
							);
						}
					}

					assert_eq!(s.work_board.view.positions["density-006"], (144., -180.));
				}
			}
		});
	}

	#[gpui::test]
	fn point_card_morph_reverses_from_its_current_frame_without_moving_the_camera(
		cx: &mut gpui::TestAppContext,
	) {
		for mode in layout::Mode::ALL {
			let surface = cx.new(super::super::tests::fixture);

			surface.update(cx, |s, _| {
				s.work_board.view.layout_mode = mode;
				s.work_board.view.zoom = Some(0.25);
				s.work_board.view.pan = (40., 70.);

				let main = s.selected.clone();
				let center = (230., 180.);
				let now = std::time::Instant::now();

				s.zoom_relations_at(0.25, 0.15, Some(center), now);

				let pan = s.work_board.view.pan;

				assert!((pan.0 - (center.0 - (center.0 - 40.) * 0.15 / 0.25)).abs() < 0.01);
				assert!((pan.1 - (center.1 - (center.1 - 70.) * 0.15 / 0.25)).abs() < 0.01);

				let mut previous = 1.;

				for ms in [0, 16, 32, 48, 64, 80] {
					let frame = s.relation_graph_at(now + std::time::Duration::from_millis(ms));
					let phase = frame.layout.card_presence();

					assert!(phase > 0. && phase <= previous);
					assert!(previous - phase < 0.25, "continuous shrink at {ms} ms");
					assert!(frame.layout.moving);

					previous = phase;

					assert_eq!(s.work_board.view.pan, pan);
				}

				let reverse = now + std::time::Duration::from_millis(80);

				s.zoom_relations_at(0.15, 0.25, Some(center), reverse);

				assert_eq!(s.relation_layout_at(reverse).card_presence(), previous);
				assert!((s.work_board.view.pan.0 - 40.).abs() < 0.01);
				assert!((s.work_board.view.pan.1 - 70.).abs() < 0.01);

				for ms in [96, 112, 160, 200, 300] {
					let frame = s.relation_graph_at(now + std::time::Duration::from_millis(ms));

					assert!(frame.layout.card_presence() >= previous);

					previous = frame.layout.card_presence();
				}

				assert_eq!(previous, 1.);
				assert!(!s.relation_layout_at(now + std::time::Duration::from_millis(300)).moving);
				assert_eq!(s.selected, main);
			});
		}
	}

	#[gpui::test]
	fn inspected_relation_record_labels_scale_with_nodes_and_stay_clear_of_them(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| tests::fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_800.), gpui::px(1_000.)));

		surface.update(visual, |s, _| {
			s.visual_relation_evidence();

			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;
			s.work_board.view.camera_fixed = true;
			s.work_board.view.picked_agent = Some("release".into());
			s.work_board.view.layout_viewport = (800., 900.);
		});

		for viewport in [(800., 900.), (1_600., 500.)] {
			surface.update(visual, |s, _| s.work_board.view.layout_viewport = viewport);

			for scale in [1., 0.9, 0.8, 0.75, 0.7, 0.9, 1.] {
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

					let bounds = graph.connection_label_bounds(&link.fact, &link.path).unwrap();

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

					s.work_board.view.pan = (
						400. - (bounds[0] + bounds[2]) / 2. * scale,
						300. - (bounds[1] + bounds[3]) / 2. * scale,
					);

					let name = "relation-edge-reply:fixture-release:review-wait:fixture-reviewer";

					assert_eq!(format!("relation-edge-{}", link.fact.id), name);

					let inspected = link.fact.clone();

					drop(cache);

					s.work_board.view.edge = Some(inspected);

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
	fn independent_trees_keep_their_lanes_across_readable_zoom(cx: &mut gpui::TestAppContext) {
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

			for zoom in [0.95, 0.9, 0.83, 0.75, 0.7, 0.83, 1.] {
				s.work_board.view.zoom = Some(zoom);

				let graph = s.relation_graph();

				assert_eq!(lanes(&graph), original, "roots changed lanes at {zoom}");
				assert_eq!(geometry(&graph), world, "zoom changed world geometry at {zoom}");
			}
		});
	}

	#[gpui::test]
	fn speech_growth_keeps_characters_and_connection_anchors_stationary(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, cx| {
			s.visual_relation_density(3, cx);

			s.work_board.view.zoom = Some(1.);
			s.work_board.view.layout_viewport = (1_200., 900.);

			for mode in layout::Mode::ALL {
				s.work_board.view.layout_mode = mode;

				for presentation in [layout::Presentation::Links, layout::Presentation::Together] {
					s.work_board.view.presentation = presentation;

					s.work_board.view.layout.replace(Default::default());
					s.work_board.view.activity.borrow_mut().clear();
					s.visual_speech_sample(0, cx);

					let now = std::time::Instant::now();
					let first = s.relation_graph_at(now);
					let anchors: Vec<_> = first.nodes.iter().map(Node::connection_center).collect();

					for step in 1..5 {
						s.visual_speech_sample(step, cx);

						let expected_text = s.work_board.briefs["agent"]
							.preview
							.as_ref()
							.unwrap()
							.recent
							.last()
							.unwrap()
							.text
							.clone();

						for ms in [0, 35, 70, 105, 140, 210, 280, 500] {
							let graph = s.relation_graph_at(
								now + std::time::Duration::from_millis(step as u64 * 1_000 + ms),
							);
							let speaker =
								graph.nodes.iter().find(|node| node.key == "agent").unwrap();

							assert_eq!(speaker.activity.lines.last().unwrap().text, expected_text);

							for (node, expected) in graph.nodes.iter().zip(&anchors) {
								let center = node.connection_center();

								assert!(
									(center.0 - expected.0).abs() < 0.01
										&& (center.1 - expected.1).abs() < 0.01,
									"{mode:?}/{presentation:?}: {} moved during message {step}: {center:?} vs {expected:?}",
									node.key
								);
							}
						}
					}
				}
			}
		});
	}

	#[gpui::test]
	fn readable_zoom_keeps_the_pointed_agent_stationary_and_returns_without_drift(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, _| {
			s.work_board.view.layout_viewport = (1_200., 700.);
			s.work_board.view.viewport = (1_200., 700.);
			s.work_board.view.zoom = Some(1.);
			s.work_board.view.pan = (45., -19.);

			let graph = s.relation_graph();
			let node = graph.nodes.iter().find(|node| node.key == "release").unwrap();
			let center = (node.x + node.width() / 2. + 45., node.y + node.height() / 2. - 19.);

			for scale in [0.9, 0.85, 0.8, 0.75, 0.7, 0.8, 0.9, 1.] {
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
	fn archived_coordinator_keeps_the_remaining_forest_readable(cx: &mut gpui::TestAppContext) {
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
			assert!((card::MIN_ZOOM..=card::MAX_ZOOM).contains(&scale));

			for node in graph.nodes.iter().filter(|_| scale > card::MIN_ZOOM) {
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
	fn routes_follow_character_edges_on_all_sides_and_share_branch_origins(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, _| {
			let initial = s.relation_graph();
			let mut parent = initial.nodes.iter().find(|node| node.row.is_some()).unwrap().clone();

			parent.key = "parent".into();
			parent.x = 0.;
			parent.y = 0.;

			let (w, h) = (parent.width(), parent.height());
			let [left, top, right, bottom] = parent.connection_bounds();
			let mut edge = initial.edges.iter().find(|edge| edge.kind.hierarchy()).unwrap().clone();

			edge.id = "parent-child".into();
			edge.from = "parent".into();
			edge.to = "child".into();

			for position in [(w + 100., 0.), (-w - 100., 0.), (0., h + 100.), (0., -h - 100.)] {
				let child =
					Node { key: "child".into(), x: position.0, y: position.1, ..parent.clone() };
				let nodes = vec![parent.clone(), child];
				let paths = routing::Cache::default().paths(&nodes, slice::from_ref(&edge));
				let mut graph = Graph {
					layout: card::Layout::new(1.),
					scopes: vec![],
					nodes,
					edges: vec![edge.clone()],
					paths,
				};
				let (lines, _) = visibility::Cache::default().sample(
					&graph,
					None,
					None,
					false,
					std::time::Instant::now(),
				);

				for key in ["parent", "child"] {
					let node = graph.nodes.iter().find(|node| node.key == key).unwrap();
					let link = lines
						.iter()
						.find(|line| line.base.from == key || line.base.to == key)
						.unwrap();
					let point = if link.base.from == key {
						link.path.curves.first().unwrap().start
					} else {
						link.path.curves.last().unwrap().end
					};
					let port = (point.0 - node.x, point.1 - node.y);

					assert!(port.0 >= left && port.0 <= right && port.1 >= top && port.1 <= bottom);
					assert!(port.0 == left || port.0 == right || port.1 == top || port.1 == bottom);
					assert!(port.0 >= left && port.0 <= right, "attach to the centered character");
				}

				let start = lines[0].path.curves.first().unwrap().start;

				if position.1 > 0. {
					let mut sibling = graph.nodes[1].clone();

					sibling.key = "sibling".into();
					sibling.x += w + 24.;

					let mut branch = edge.clone();

					branch.id = "parent-sibling".into();
					branch.to = "sibling".into();

					graph.nodes.push(sibling);
					graph.edges.push(branch);

					graph.paths = routing::Cache::default().paths(&graph.nodes, &graph.edges);

					let (lines, _) = visibility::Cache::default().sample(
						&graph,
						None,
						None,
						false,
						std::time::Instant::now(),
					);

					assert!(
						lines.iter().all(|line| line.path.curves.first().unwrap().start == start)
					);
				}

				for edge in &mut graph.edges {
					edge.kind = Kind::Message;
				}

				let (hidden, _) = visibility::Cache::default().sample(
					&graph,
					None,
					None,
					false,
					std::time::Instant::now(),
				);

				assert!(hidden.is_empty(), "unfocused collaboration stays hidden");
			}
		});
	}

	fn long_metrics_fixture(cx: &mut Context<AgentSurface>) -> AgentSurface {
		let mut s = tests::fixture(cx);

		s.workspace.graph_visible = true;
		s.workspace.graph_expanded = true;
		s.work_board.view.camera_fixed = true;

		let row = s.board_rows().into_iter().find(|row| row.key == "release").unwrap();

		s.snapshot.as_mut().unwrap().agent_slots.insert(row.key.clone(), 1_234);

		let purpose = "Please verify that the graph preserves long metric values and the ending of each complete native answer.";

		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == row.work)
			.unwrap()
			.title = purpose.chars().take(64).collect();

		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				review: None,
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: Some(super::super::activity::Preview {
					turn: "readable-type".into(),
					task: purpose.into(),
					text: "Inspect the resulting graph".into(),
					running: None,
					recent: (0..3)
						.map(|index| super::super::activity::Operation {
							id: format!("readable-{index}"),
							text: "Simmer canned tomatoes, white beans, and vegetable broth for 10 minutes, then stir in a pinch of dried basil.".into(),
							kind: super::super::activity::OperationKind::Result,
						})
						.collect(),
				}),
				question: None,
				resources: None,
				relations: vec![],
				stamp: s.brief_stamp(&row),
				read_at: std::time::Instant::now(),
				metrics: crate::ui_work_metrics::Sample {
					tokens: Some(999_900_000),
					stats: Some(decodex_protocol::AgentThreadMetrics {
						completed_ms: Some(999 * 3_600_000 + 59 * 60_000),
						active_ms: None,
						generating: false,
						rate: Some(decodex_protocol::AgentOutputRate {
							estimated: false,
							active: false,
							output_tokens: 999_900,
							duration_ms: 1_000,
							started_at_ms: 0,
							completed_at_ms: Some(10_000),
						}),
					}),
					..Default::default()
				},
			},
		);

		s
	}

	fn assert_complete_message(
		surface: &gpui::Entity<AgentSurface>,
		visual: &mut gpui::VisualTestContext,
		layout: card::Layout,
		card: gpui::Bounds<gpui::Pixels>,
		scale: f32,
		density: f32,
	) {
		if let Some(preview) = visual.debug_bounds("relation-preview-release") {
			let metrics = visual.debug_bounds("relation-metrics-release").unwrap();

			assert!(preview.left() >= card.left() && preview.right() <= card.right());
			assert!(metrics.left() > card.left() && metrics.right() < card.right());
			assert!(preview.bottom() <= metrics.top(), "zoom {scale}: {preview:?}, {metrics:?}");
			assert!(metrics.bottom() < card.bottom(), "zoom {scale}: {metrics:?}, {card:?}");

			let message = visual.debug_bounds("relation-message-body-release").unwrap();
			let assignment = visual.debug_bounds("relation-assignment-release").unwrap();

			assert!(metrics.bottom() < assignment.top(), "purpose stays below the character HUD");

			let avatar = visual.debug_bounds("relation-avatar-release").unwrap();

			assert!(preview.top() < avatar.top(), "speech floats above its speaker");
			assert!(
				message.bottom() <= preview.bottom() + gpui::px(0.5)
					&& preview.bottom() < card.bottom(),
				"complete speech bounds at zoom {scale}, density {density}: message {message:?}, preview {preview:?}, card {card:?}"
			);

			let text = surface.read_with(visual, |s, _| {
				s.relation_graph()
					.nodes
					.into_iter()
					.find(|node| node.key == "release")
					.unwrap()
					.activity
					.lines
					.last()
					.unwrap()
					.text
					.clone()
			});
			let shaped_height = visual.update(|window, _| {
				let run = gpui::TextRun {
					len: text.len(),
					font: gpui::font(crate::ui_theme::FONT_FAMILY),
					color: ui_theme::rgb(TEXT).into(),
					background_color: None,
					underline: None,
					strikethrough: None,
				};

				window
					.text_system()
					.shape_text(
						text.into(),
						gpui::px(layout.activity_size()),
						&[run],
						Some(message.size.width),
						None,
					)
					.unwrap()
					.iter()
					.map(|line| f32::from(line.size(gpui::px(layout.activity_line())).height))
					.sum::<f32>()
			});

			assert!(
				(shaped_height - f32::from(message.size.height)).abs() <= 0.5,
				"the bubble uses the painted line count at zoom {scale} on display {density}: shaped {shaped_height}, measured {message:?}, estimated width {}, preview {preview:?}",
				layout.text_width()
			);
		}
	}

	fn assert_character_hud(
		visual: &mut gpui::VisualTestContext,
		layout: card::Layout,
		card: gpui::Bounds<gpui::Pixels>,
		scale: f32,
		density: f32,
	) {
		let tokens = visual.debug_bounds("relation-tokens-release").unwrap();
		let identity = visual.debug_bounds("relation-identity-release").unwrap();
		let metrics = visual.debug_bounds("relation-metrics-release").unwrap();

		assert!(
			identity.left() > card.left() && identity.right() <= card.right(),
			"zoom {scale}: {identity:?}, {card:?}"
		);
		assert!(
			f32::from(metrics.size.height) <= layout.metrics_height() + 0.5,
			"metrics at zoom {scale}, display {density}: {metrics:?}, reserved {}; values {:?} {:?} {:?}",
			layout.metrics_height(),
			tokens,
			visual.debug_bounds("relation-time-release"),
			visual.debug_bounds("relation-rate-release")
		);

		let time = visual.debug_bounds("relation-time-release").unwrap();

		assert!(time.left() >= metrics.left() && time.right() <= metrics.right());
		assert!(time.top() >= metrics.top() && time.bottom() <= metrics.bottom());

		let rate = visual.debug_bounds("relation-rate-release").unwrap();

		assert!(
			rate.left() >= metrics.left() && rate.right() <= metrics.right(),
			"zoom {scale}, display {density}: tokens {tokens:?}, time {time:?}, rate {rate:?}, metrics {metrics:?}"
		);
		assert!(rate.top() >= metrics.top() && rate.bottom() <= metrics.bottom());

		for (upper, lower) in [(tokens, time), (time, rate)] {
			assert_eq!(upper.left(), lower.left(), "the status tracks share one left edge");
			assert_eq!(upper.right(), lower.right(), "the status tracks share one right edge");
			assert!(lower.top() >= upper.bottom(), "the three stats stack vertically");
			assert!(f32::from(lower.top() - upper.bottom()) <= layout.spacing(1.) + 0.5);
		}

		let number = visual.debug_bounds("relation-number-release").unwrap();

		assert!(number.left() >= identity.left() && number.right() <= identity.right());
		assert_eq!(number.top(), identity.top());

		let avatar = visual.debug_bounds("relation-avatar-release").unwrap();
		let preview = visual.debug_bounds("relation-preview-release");

		assert_eq!(preview.is_some(), layout.activity_visible());

		if let Some(preview) = preview {
			assert!(
				(avatar.center().x - preview.center().x).abs() <= gpui::px(0.5),
				"speech is centered on its speaker within display rounding"
			);

			let (tip_x, tip_y) = layout.speech_tip(f32::from(preview.size.height));
			let speech_tip = preview.bottom_left() + gpui::point(gpui::px(tip_x), gpui::px(tip_y));

			assert!((speech_tip.x - avatar.center().x).abs() <= gpui::px(0.5));
			assert!(speech_tip.y >= avatar.top() && speech_tip.y - avatar.top() <= gpui::px(8.));
			assert!(
				visual.debug_bounds("relation-assignment-release").unwrap().left() > number.right(),
				"the task title follows the compact agent number"
			);
			assert!(
				number.top() > metrics.bottom(),
				"identity stays with the nameplate below the character"
			);
			assert_eq!(
				number.top(),
				visual.debug_bounds("relation-assignment-release").unwrap().top()
			);
		}

		assert!(
			visual.debug_bounds("relation-assignment-release").is_some()
				== layout.activity_visible(),
			"the current assignment stays distinct from recent activity"
		);
		assert!(number.top() > metrics.bottom() && avatar.bottom() < metrics.top());

		if let Some(title) = visual.debug_bounds("relation-assignment-release") {
			assert!(
				((identity.left() + title.right()) / 2. - avatar.center().x).abs() <= gpui::px(0.5)
			);
		} else {
			assert!((identity.center().x - avatar.center().x).abs() <= gpui::px(0.5));
		}

		assert!(
			f32::from(metrics.top() - avatar.bottom()) <= layout.gap() + 0.5,
			"only the HUD sits immediately below the portrait"
		);
		assert!(
			f32::from(metrics.center().x - avatar.center().x).abs() <= 0.5,
			"the status block stays centered under the character"
		);
		assert!(
			f32::from(metrics.size.width) <= 80.,
			"long native counters still form a small block"
		);

		let status = visual.debug_bounds("relation-status-release").unwrap();

		assert!(status.left() >= identity.left());
		assert!(status.right() < number.left());
		assert!(status.top() > metrics.bottom());

		let selection = visual.debug_bounds("relation-selection-release").unwrap();

		assert_eq!(selection, avatar, "selection belongs only to the portrait");
		assert!(selection.bottom() < metrics.top());
		assert!(metrics.top() > card.top() && metrics.bottom() < card.bottom());
		assert!(
			metrics.size.width < card.size.width - gpui::px(2. * layout.spacing(card::INSET_X)),
			"the HUD uses its content width, not the camera footprint"
		);
	}

	#[gpui::test]
	fn card_content_fits_across_zoom_levels_below_the_toolbar(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| long_metrics_fixture(cx));
		let handle = visual.update(|window, _| window.window_handle());

		for density in [2., 1., 2.] {
			visual.simulate_window_scale_factor_change(handle, density);

			let mut previous: Option<gpui::Size<gpui::Pixels>> = None;
			let mut full_portrait = None;

			for scale in
				[1., 0.95, 0.9, 0.85, 0.8, 0.75, 0.71, 0.7, 0.69, 0.6, 0.55, 0.51, 0.5, 0.46, 0.45]
			{
				let world_height = surface.update(visual, |s, cx| {
					s.work_board.view.zoom = Some(scale);

					let graph = s.relation_graph();
					let node = graph.nodes.iter().find(|node| node.key == "release").unwrap();

					for a in graph.nodes.iter().filter(|node| node.row.is_some()) {
						assert_eq!(a.width(), card::WIDTH);
						assert!(a.height() > 0.);

						for b in graph
							.nodes
							.iter()
							.filter(|node| node.row.is_some() && node.key != a.key)
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

					node.height()
				});

				visual.update(crate::ui_frame::draw_frame);

				let card = visual.debug_bounds("relation-node-release").unwrap();
				let layout = card::Layout::new(scale).on_display(density);
				let portrait = visual.debug_bounds("relation-avatar-release").unwrap().size;
				let full_portrait = *full_portrait.get_or_insert(portrait);

				for (size, full_size) in
					[(portrait.width, full_portrait.width), (portrait.height, full_portrait.height)]
				{
					assert!(
						(f32::from(size) - f32::from(full_size) * scale).abs() <= 0.5,
						"character stopped following the camera at zoom {scale}, display {density}: {portrait:?} vs {full_portrait:?}"
					);
				}

				if let Some(previous) = previous {
					assert!(
						card.size.width < previous.width,
						"card stopped narrowing at {scale}: {:?} vs {previous:?}",
						card.size
					);

					if layout.full_detail() {
						assert!(
							card.size.height <= previous.height,
							"full-detail footprint grows at {scale}"
						);
					}
				}

				previous = Some(card.size);

				// The native renderer rounds bounds to the display's pixel grid.
				assert!((f32::from(card.size.width) - card::WIDTH * scale).abs() <= 0.5);
				assert!((f32::from(card.size.height) - world_height * scale).abs() <= 0.5);

				let toolbar = visual.debug_bounds("relations-toolbar").unwrap();

				assert!(card.top() > toolbar.bottom());
				assert!(layout.title_size() >= 10.);
				assert_eq!(layout.metric_size(), layout.title_size());
				assert_eq!(layout.activity_size(), layout.title_size());
				assert_eq!((layout.title_size() * density).fract(), 0.);

				let task = visual
					.debug_bounds("relation-identity-release")
					.expect("work remains visible at every zoom");
				let metrics = visual.debug_bounds("relation-metrics-release").unwrap();
				let tokens = visual.debug_bounds("relation-tokens-release").unwrap();

				assert!(tokens.left() >= metrics.left() && tokens.right() <= metrics.right());
				assert!(tokens.top() >= metrics.top() && tokens.bottom() <= metrics.bottom());
				assert!(task.left() > card.left() && task.right() <= card.right());
				assert!(task.top() >= card.top() && task.bottom() < card.bottom());

				assert_complete_message(&surface, visual, layout, card, scale, density);
				assert_character_hud(visual, layout, card, scale, density);
			}
		}
	}

	#[gpui::test]
	fn selected_team_reveals_descendants_and_ancestors_without_foreign_branches(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, _| {
			let graph = s.relation_graph();
			let all = groups::focus(&graph, Some("agent"));
			let branch = groups::focus(&graph, Some("mira"));

			for edge in graph.edges.iter().filter(|edge| edge.kind.hierarchy()) {
				if edge.from == "agent" || edge.from == "mira" {
					assert!(all.contains_key(&edge.id), "root includes deeper ownership");
				}
				if edge.from == "mira" || (edge.from == "agent" && edge.to == "mira") {
					assert!(
						branch.contains_key(&edge.id),
						"selected branch and ancestor remain visible"
					);
				}
				if edge.from == "sage" {
					assert!(!branch.contains_key(&edge.id));
				}
			}

			assert!(groups::focus(&graph, None).is_empty());
		});
	}

	#[gpui::test]
	fn archived_coordinator_keeps_visible_team_boundaries(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = tests::fixture(cx);

			s.visual_workspace_page("workbench", cx);
			s.snapshot.as_mut().unwrap().archived_work_ids.insert("agent".into());

			s.workspace.graph_expanded = true;
			s.work_board.view.presentation = layout::Presentation::Together;
			s.work_board.view.camera_fixed = true;
			s.work_board.view.zoom = Some(1.);

			s
		});

		visual.simulate_resize(gpui::size(gpui::px(1_600.), gpui::px(1_000.)));
		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("relation-team-mira").is_none());
		assert!(visual.debug_bounds("relation-team-sage").is_none());
		assert!(visual.debug_bounds("relation-team-agent").is_none());

		for zoom in [0.35, 0.5, 0.75, 1.] {
			surface.update(visual, |s, cx| {
				s.visual_relation_zoom(zoom, cx);
				s.select_workbench_agent("mira", cx);
			});
			visual.update(crate::ui_frame::draw_frame);

			let team = visual.debug_bounds("relation-team-mira").unwrap();
			let icon = visual.debug_bounds("relation-team-icon-mira").unwrap();
			let count = visual.debug_bounds("relation-team-count-mira").unwrap();

			assert_eq!(icon.center().y, team.center().y);
			assert_eq!(count.center().y, team.center().y);
			assert!(icon.right() < count.left());
			assert!(icon.top() >= team.top() && icon.bottom() <= team.bottom());
			assert!(visual.debug_bounds("relation-team-sage").is_none());
		}

		surface.read_with(visual, |s, _| {
			assert!(s.snapshot.as_ref().unwrap().archived_work_ids.contains("agent"));
			assert_eq!(s.selected.as_deref(), Some("agent"));
		});
	}

	#[gpui::test]
	fn explicit_child_selection_reveals_its_recorded_parent_without_changing_the_chat(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = tests::fixture(cx);

			s.visual_workspace_page("workbench", cx);

			s.workspace.graph_expanded = true;
			s.work_board.view.camera_fixed = true;
			s.work_board.view.zoom = Some(1.);
			s.work_board.view.picked_agent = None;

			s.composer.update(cx, |input, cx| input.set_content("Keep this Main draft", cx));

			s
		});

		visual.simulate_resize(gpui::size(gpui::px(1_600.), gpui::px(1_000.)));

		let child = "native:mira:ref-nora";
		let parent = "mira";
		let before = surface.read_with(visual, |s, _| {
			assert!(
				s.relation_graph()
					.edges
					.iter()
					.any(|edge| edge.kind.hierarchy() && edge.from == parent && edge.to == child)
			);

			(s.selected.clone(), s.native_agents.selected.clone())
		});

		for mode in layout::Mode::ALL {
			for presentation in [layout::Presentation::Links, layout::Presentation::Together] {
				surface.update(visual, |s, cx| {
					s.work_board.view.layout_mode = mode;
					s.work_board.view.presentation = presentation;
					s.work_board.view.picked_agent = None;

					cx.notify();
				});

				visual.update(crate::ui_frame::draw_frame);

				assert!(visual.debug_bounds("relation-owner-hint-native:mira:ref-nora").is_none());

				let shape = surface.update(visual, |s, cx| {
					let graph = s.relation_graph();
					let shape: Vec<_> =
						graph.nodes.iter().map(|n| (n.key.clone(), n.x, n.y, n.size)).collect();

					s.select_workbench_agent(child, cx);

					shape
				});

				visual.update(crate::ui_frame::draw_frame);

				assert!(visual.debug_bounds("relation-owner-hint-native:mira:ref-nora").is_some());
				assert!(visual.debug_bounds("relation-owner-hint-mira").is_none());

				surface.read_with(visual, |s, cx| {
					let graph = s.relation_graph();

					assert_eq!(
						graph
							.nodes
							.iter()
							.map(|n| (n.key.clone(), n.x, n.y, n.size))
							.collect::<Vec<_>>(),
						shape
					);

					let owner = graph.nodes.iter().find(|n| n.key == parent).unwrap();

					assert!(
						s.relation_node_emphasis(owner, &graph, &s.relation_neighborhood(&graph)).0
					);
					assert_eq!((s.selected.clone(), s.native_agents.selected.clone()), before);
					assert_eq!(s.composer.read(cx).content(), "Keep this Main draft");
				});
			}
		}
	}

	#[gpui::test]
	fn hovering_work_keeps_card_bounds_and_camera_unchanged(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| tests::fixture(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_200.), gpui::px(900.)));

		surface.update(visual, |s, cx| {
			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;
			s.work_board.view.camera_fixed = true;
			s.work_board.view.zoom = Some(0.85);

			let graph = s.relation_graph();
			let node = graph.nodes.iter().find(|node| node.key == "release").unwrap();

			s.work_board.view.pan = (40. - node.x * 0.85, 80. - node.y * 0.85);

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(250));

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

		assert!(
			surface.read_with(visual, |s, _| s.relation_context().is_none()),
			"hover does not select, reveal a branch, or open a floating card"
		);
		assert_eq!(
			visual.debug_bounds("relation-node-release").unwrap().size,
			card.size,
			"passing over a node does not expand it immediately"
		);

		thread::sleep(std::time::Duration::from_millis(200));

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(230));

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

		thread::sleep(std::time::Duration::from_millis(140));

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(230));

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

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(250));

		visual.update(crate::ui_frame::draw_current_frame);

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

			visual.update(crate::ui_frame::draw_current_frame);
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
			s.work_board.view.zoom = Some(0.82);

			s.select_workbench_agent("agent", cx);

			assert_eq!(s.work_board.view.pan, (80., -30.));
			assert_eq!(s.work_board.view.zoom, Some(0.82));
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
			s.work_board.view.zoom = Some(0.82);

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
			assert_eq!(s.work_board.view.zoom, Some(0.82));

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
			s.work_board.view.viewport = (1_200., 900.);
			s.work_board.view.layout_viewport = (1_200., 900.);
			s.work_board.view.zoom = Some(0.75);
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
					gpui::size(gpui::px(1_200.), gpui::px(900.)),
				),
				0.75,
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
				0.75,
				count,
				cx,
			);

			let graph = s.relation_graph();

			assert_eq!(s.work_board.view.zoom, Some(0.75));
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

			s.work_board.view.positions.insert(graph.nodes[0].key.clone(), (9_000., 8_000.));
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
	fn initial_camera_frames_agents_without_shrinking_below_readable_size(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, _| {
			for viewport in [(900., 300.), (680., 210.), (440., 600.)] {
				s.work_board.view.viewport = viewport;

				s.frame_relations();

				let scale = s.relation_scale();

				assert!((card::MIN_ZOOM..=card::MAX_ZOOM).contains(&scale));

				for node in s.relation_graph().nodes.into_iter().filter(|_| scale > card::MIN_ZOOM)
				{
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

			let before = (s.work_board.view.zoom, s.work_board.view.pan);

			for brief in s.work_board.briefs.values_mut() {
				brief.resources = None;
			}

			s.snapshot.as_mut().unwrap().context_references.clear();
			s.fit_relations();

			assert_eq!(
				before,
				(s.work_board.view.zoom, s.work_board.view.pan),
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

				snapshot.projects = vec![
					decodex_protocol::ProjectDto {
						native_project_id: None,
						lead_agent_id: None,
						id: "reading".into(),
						name: "Reading".into(),
						directory: "/tmp/reading".into(),
						agent_ids: vec!["a-reading".into(), "c-reading".into()],
					},
					decodex_protocol::ProjectDto {
						native_project_id: None,
						lead_agent_id: None,
						id: "project".into(),
						name: "Project".into(),
						directory: "/tmp/project".into(),
						agent_ids: project,
					},
				];

				let graph = s.relation_graph();
				let node = |key: &str| graph.nodes.iter().find(|n| n.key == key).unwrap();
				let first = node("a-reading");
				let last = node("c-reading");

				assert!(
					(last.x - first.x).abs() + (last.y - first.y).abs()
						<= first.width().max(first.height()) + 2. * GRID,
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

			snapshot.projects = vec![decodex_protocol::ProjectDto {
				native_project_id: None,
				lead_agent_id: None,
				id: "project".into(),
				name: "Project".into(),
				directory: "/tmp/project".into(),
				agent_ids: works,
			}];
			s.work_board.view.layout_viewport = (1_600., 600.);

			let graph = s.relation_graph();
			let root = graph.nodes.iter().find(|n| n.key == "agent").unwrap();
			let child = graph.nodes.iter().find(|n| n.key == "unassigned").unwrap();
			let project = graph.nodes.iter().find(|n| n.key == "release").unwrap();

			assert!(
				(root.y + root.height() < child.y && child.y == project.y)
					|| (root.x + root.width() < child.x && child.x == project.x)
			);
			assert_eq!(graph.scopes.len(), 1);
			assert!(graph.scopes.iter().all(|scope| {
				!scope.members.contains("agent") && !scope.members.contains("unassigned")
			}));
		});
	}

	#[gpui::test]
	fn workspace_frames_preserve_global_graph_and_do_not_overlap(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::super::tests::fixture);

		surface.update(cx, |s, _| {
			let original = s.relation_graph();
			let works: Vec<_> =
				s.snapshot.as_ref().unwrap().work_items.iter().map(|w| w.id.clone()).collect();

			s.snapshot.as_mut().unwrap().projects = vec![
				decodex_protocol::ProjectDto {
					native_project_id: None,
					lead_agent_id: None,
					id: "scope-a".into(),
					name: "Workspace A".into(),
					directory: "/tmp/a".into(),
					agent_ids: vec![works[0].clone()],
				},
				decodex_protocol::ProjectDto {
					native_project_id: None,
					lead_agent_id: None,
					id: "scope-b".into(),
					name: "Workspace B".into(),
					directory: "/tmp/b".into(),
					agent_ids: vec![works[1].clone()],
				},
			];
			s.workspace.workspace_filter = Some("scope-a".into());

			for mode in layout::Mode::ALL {
				for presentation in [layout::Presentation::Links, layout::Presentation::Together] {
					s.work_board.view.layout_mode = mode;
					s.work_board.view.presentation = presentation;

					let graph = s.relation_graph();

					// Native point geometry follows animated content heights. Workspace
					// frames and explicitly dragged positions retain the world lattice.
					for scope in &graph.scopes {
						for coordinate in scope.bounds {
							assert_eq!(coordinate, relations::snap(coordinate));
						}
					}

					assert_eq!(graph.nodes.len(), original.nodes.len());
					assert_eq!(graph.edges.len(), original.edges.len());
					assert_eq!(graph.scopes.len(), 2);

					let a = &graph.scopes[0].bounds;
					let b = &graph.scopes[1].bounds;

					assert!(a[3] <= b[1] || b[3] <= a[1] || a[2] <= b[0] || b[2] <= a[0]);

					for scope in &graph.scopes {
						for node in graph.nodes.iter().filter(|n| {
							n.row
								.as_ref()
								.is_some_and(|r| r.workspace.as_deref() == Some(scope.id.as_str()))
						}) {
							assert!(
								node.x >= scope.bounds[0]
									&& node.x + node.width() <= scope.bounds[2]
							);
							assert!(
								node.y >= scope.bounds[1] + 30.
									&& node.y + node.height() <= scope.bounds[3]
							);
						}
					}
				}
			}

			s.work_board.view.viewport = (1_200., 900.);

			s.fit_relations();

			let graph = s.relation_graph();
			let zoom = s.work_board.view.zoom.unwrap();
			let pan = s.work_board.view.pan;

			for scope in &graph.scopes {
				assert!(scope.bounds[0] * zoom + pan.0 >= 0.);
				assert!(scope.bounds[1] * zoom + pan.1 >= 0.);
				assert!(scope.bounds[2] * zoom + pan.0 <= 1_200.);
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
			let stride = relations::grid_spacing(scale) / (GRID * scale);

			assert_eq!(stride, stride.round());
			assert!(relations::grid_spacing(scale) >= 8.);
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
			assignment: String::new(),
			status: String::new(),
			color: 0,
			row: None,
			activity: Default::default(),
			size: (224., 48.),
			attachment: None,
			identity: None,
			speech: None,
			base_height: 48.,
			layout_height: 48.,
			navigation: (120., 80.),
			x: 120.,
			y: 80.,
		};
		let graph = Graph {
			layout: card::Layout::new(1.),
			scopes: vec![],
			nodes: vec![node],
			edges: vec![],
			paths: BTreeMap::new(),
		};

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
			assignment: String::new(),
			status: String::new(),
			color: 0,
			row: None,
			activity: Default::default(),
			size: (224., 48.),
			attachment: None,
			identity: None,
			speech: None,
			base_height: 48.,
			layout_height: 48.,
			navigation: (80., 64.),
			x: 80.,
			y: 64.,
		};

		for (x, y) in [(80., 176.), (80., -48.), (400., 64.), (-240., 64.)] {
			let b = Node { key: "kant".into(), title: "Kant".into(), x, y, ..a.clone() };
			let path = relations::route(&a, &b);
			let start = path.curves[0].start;
			let end = path.curves[0].end;
			let reverse = relations::route(&b, &a);

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
