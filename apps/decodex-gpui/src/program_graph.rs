//! Private host-owned Program and Domain Pack graph projection.

use std::{
	collections::{BTreeMap, BTreeSet, HashMap, HashSet},
	sync::Arc,
};

use gpui::{
	AnyElement, App, Bounds, Context, CursorStyle, Element, ElementId, Entity, EventEmitter,
	FocusHandle, FontWeight, GlobalElementId, InspectorElementId, KeyBinding, LayoutId,
	MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, Render,
	Role, ScrollDelta, ScrollWheelEvent, Size, Style, Subscription, Window, actions, canvas, div,
	point, prelude::*, px, relative, rgb, rgba, size,
};

use decodex_protocol::{
	DomainEntityDto, DomainPackCapabilityStatus, EntityId, ProgramCycleDto, ProgramNodeDto,
	ProgramNodeKind, ProgramRelationKind,
};

use crate::ui_theme;

const NODE_WIDTH: f32 = 168.0;
const NODE_HEIGHT: f32 = 104.0;
const LAYER_GAP: f32 = 76.0;
const ROW_GAP: f32 = 32.0;
const WORLD_PADDING: f32 = 44.0;
const VIEW_PADDING: f32 = 24.0;
const MIN_ZOOM: f32 = 0.16;
const MAX_ZOOM: f32 = 1.8;
const ZOOM_STEP: f32 = 1.2;
const MIN_VISIBLE_WORLD: f32 = 72.0;

const TEXT: u32 = ui_theme::TEXT;
const TEXT_MUTED: u32 = ui_theme::TEXT_MUTED;
const TEXT_FAINT: u32 = ui_theme::TEXT_FAINT;
const BLUE: u32 = ui_theme::BLUE;
const GREEN: u32 = ui_theme::GREEN;
const AMBER: u32 = ui_theme::AMBER;
const LINE: u32 = ui_theme::LINE;

actions!(
	program_graph,
	[
		GraphMoveLeft,
		GraphMoveRight,
		GraphMoveUp,
		GraphMoveDown,
		GraphActivate,
		GraphFit,
		GraphZoomIn,
		GraphZoomOut,
		GraphReset,
	]
);

pub(crate) fn bind_keys(cx: &mut App) {
	cx.bind_keys([
		KeyBinding::new("left", GraphMoveLeft, Some("ProgramGraphNode")),
		KeyBinding::new("right", GraphMoveRight, Some("ProgramGraphNode")),
		KeyBinding::new("up", GraphMoveUp, Some("ProgramGraphNode")),
		KeyBinding::new("down", GraphMoveDown, Some("ProgramGraphNode")),
		KeyBinding::new("enter", GraphActivate, Some("ProgramGraphNode")),
		KeyBinding::new("space", GraphActivate, Some("ProgramGraphNode")),
		KeyBinding::new("f", GraphFit, Some("ProgramGraphNode")),
		KeyBinding::new("=", GraphZoomIn, Some("ProgramGraphNode")),
		KeyBinding::new("-", GraphZoomOut, Some("ProgramGraphNode")),
		KeyBinding::new("0", GraphReset, Some("ProgramGraphNode")),
	]);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProgramGraphEvent {
	SelectionChanged,
	OpenConversation(EntityId),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum GraphLens {
	Domain,
	Program,
}

impl GraphLens {
	const fn label(self) -> &'static str {
		match self {
			Self::Domain => "Domain Pack",
			Self::Program => "Program causal",
		}
	}

	const fn element_id(self) -> &'static str {
		match self {
			Self::Domain => "domain",
			Self::Program => "program",
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct EdgeIdentity {
	from: String,
	relation: String,
	to: String,
}

#[derive(Clone, Debug)]
struct GraphNode {
	id: EntityId,
	kind: String,
	title: String,
	summary: String,
	state: String,
	color: u32,
	conversation_id: Option<EntityId>,
}

#[derive(Clone, Debug)]
struct GraphEdge {
	identity: EdgeIdentity,
	label: String,
	explicit_feedback: bool,
}

#[derive(Clone, Debug)]
struct GraphInput {
	nodes: Vec<GraphNode>,
	edges: Vec<GraphEdge>,
}

impl GraphInput {
	fn structure_key(&self) -> GraphStructureKey {
		let mut nodes =
			self.nodes.iter().map(|node| node.id.as_str().to_owned()).collect::<Vec<_>>();
		nodes.sort();
		let mut edges = self
			.edges
			.iter()
			.map(|edge| (edge.identity.clone(), edge.explicit_feedback))
			.collect::<Vec<_>>();
		edges.sort();
		GraphStructureKey { nodes, edges }
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GraphStructureKey {
	nodes: Vec<String>,
	edges: Vec<(EdgeIdentity, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WorldPoint {
	x: f32,
	y: f32,
}

#[derive(Clone, Debug)]
struct GraphLayout {
	positions: BTreeMap<String, WorldPoint>,
	#[cfg(test)]
	ranks: BTreeMap<String, usize>,
	feedback_edges: BTreeSet<EdgeIdentity>,
	world_size: Size<f32>,
}

#[derive(Clone, Debug)]
struct GraphScene {
	nodes: Vec<GraphNode>,
	edges: Vec<GraphEdge>,
	layout: Arc<GraphLayout>,
}

impl GraphScene {
	fn contains(&self, id: &EntityId) -> bool {
		self.nodes.iter().any(|node| node.id == *id)
	}

	fn node(&self, id: &EntityId) -> Option<&GraphNode> {
		self.nodes.iter().find(|node| node.id == *id)
	}

	fn node_by_key(&self, id: &str) -> Option<&GraphNode> {
		self.nodes.iter().find(|node| node.id.as_str() == id)
	}

	fn relation_readout(&self, selected: Option<&EntityId>) -> Vec<String> {
		let Some(selected) = selected.filter(|selected| self.contains(selected)) else {
			return vec!["Select a node to read its incoming and outgoing relations.".to_owned()];
		};
		let mut readout = Vec::new();
		for edge in &self.edges {
			let feedback = self.layout.feedback_edges.contains(&edge.identity);
			if edge.identity.from == selected.as_str() {
				let target = self
					.node_by_key(&edge.identity.to)
					.map_or(edge.identity.to.as_str(), |node| node.title.as_str());
				readout.push(format!(
					"OUT{} · {} → {target}",
					if feedback { " · FEEDBACK" } else { "" },
					edge.label
				));
			}
			if edge.identity.to == selected.as_str() {
				let source = self
					.node_by_key(&edge.identity.from)
					.map_or(edge.identity.from.as_str(), |node| node.title.as_str());
				readout.push(format!(
					"IN{} · {source} → {}",
					if feedback { " · FEEDBACK" } else { "" },
					edge.label
				));
			}
		}
		if readout.is_empty() {
			readout.push("No projected relations for this identity.".to_owned());
		}
		readout.sort();
		readout
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GraphLayoutError {
	Empty,
	DuplicateNode,
	DuplicateEdge,
	DanglingEdge,
	CycleBreakFailed,
}

#[derive(Default)]
struct GraphCache {
	key: Option<GraphStructureKey>,
	layout: Option<Arc<GraphLayout>>,
	scene: Option<Arc<GraphScene>>,
	computations: usize,
}

impl GraphCache {
	fn update(&mut self, input: GraphInput) -> Result<bool, GraphLayoutError> {
		let key = input.structure_key();
		let changed = self.key.as_ref() != Some(&key);
		let layout = if changed {
			self.computations = self.computations.saturating_add(1);
			Arc::new(compute_layout(&input)?)
		} else {
			self.layout.clone().ok_or(GraphLayoutError::CycleBreakFailed)?
		};
		self.key = Some(key);
		self.layout = Some(Arc::clone(&layout));
		self.scene = Some(Arc::new(GraphScene { nodes: input.nodes, edges: input.edges, layout }));
		Ok(changed)
	}

	fn clear(&mut self) {
		self.key = None;
		self.layout = None;
		self.scene = None;
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ViewportMode {
	Fit,
	Manual { zoom: f32, pan: Point<f32> },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GraphViewport {
	mode: ViewportMode,
	bounds: Option<Bounds<Pixels>>,
	drag_anchor: Option<Point<Pixels>>,
}

impl Default for GraphViewport {
	fn default() -> Self {
		Self { mode: ViewportMode::Fit, bounds: None, drag_anchor: None }
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ViewTransform {
	zoom: f32,
	pan: Point<f32>,
	view_size: Size<f32>,
	world_size: Size<f32>,
}

impl ViewTransform {
	fn for_viewport(
		viewport: GraphViewport,
		bounds: Bounds<Pixels>,
		world_size: Size<f32>,
	) -> Self {
		let view_size = size(f32::from(bounds.size.width), f32::from(bounds.size.height));
		let (zoom, pan) = match viewport.mode {
			ViewportMode::Fit => {
				let width = (view_size.width - VIEW_PADDING * 2.0).max(1.0) / world_size.width;
				let height = (view_size.height - VIEW_PADDING * 2.0).max(1.0) / world_size.height;
				(width.min(height).clamp(MIN_ZOOM, 1.0), point(0.0, 0.0))
			},
			ViewportMode::Manual { zoom, pan } => (zoom.clamp(MIN_ZOOM, MAX_ZOOM), pan),
		};
		let mut transform = Self { zoom, pan, view_size, world_size };
		transform.pan = transform.clamp_pan(transform.pan);
		transform
	}

	fn clamp_pan(self, pan: Point<f32>) -> Point<f32> {
		let horizontal = (self.world_size.width * self.zoom / 2.0 + self.view_size.width / 2.0
			- MIN_VISIBLE_WORLD)
			.max(0.0);
		let vertical = (self.world_size.height * self.zoom / 2.0 + self.view_size.height / 2.0
			- MIN_VISIBLE_WORLD)
			.max(0.0);
		point(pan.x.clamp(-horizontal, horizontal), pan.y.clamp(-vertical, vertical))
	}

	fn world_to_screen(self, value: WorldPoint) -> Point<f32> {
		point(
			self.view_size.width / 2.0
				+ (value.x - self.world_size.width / 2.0) * self.zoom
				+ self.pan.x,
			self.view_size.height / 2.0
				+ (value.y - self.world_size.height / 2.0) * self.zoom
				+ self.pan.y,
		)
	}
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct FocusKey {
	lens: GraphLens,
	id: String,
}

#[derive(Clone, Copy)]
enum MoveDirection {
	Left,
	Right,
	Up,
	Down,
}

#[derive(Clone, Copy)]
enum ViewportCommand {
	Fit,
	ZoomIn,
	ZoomOut,
	Reset,
}

pub(crate) struct ProgramGraphSurface {
	program: GraphCache,
	domain: GraphCache,
	domain_title: Option<String>,
	domain_status: Option<String>,
	selected: Option<EntityId>,
	active_lens: GraphLens,
	focused: Option<FocusKey>,
	program_viewport: GraphViewport,
	domain_viewport: GraphViewport,
	focus_handles: BTreeMap<FocusKey, FocusHandle>,
	focus_subscriptions: Vec<Subscription>,
	focus_bindings_dirty: bool,
	projection_error: Option<String>,
}

impl EventEmitter<ProgramGraphEvent> for ProgramGraphSurface {}

impl ProgramGraphSurface {
	pub(crate) fn new(_: &mut Context<Self>) -> Self {
		Self {
			program: GraphCache::default(),
			domain: GraphCache::default(),
			domain_title: None,
			domain_status: None,
			selected: None,
			active_lens: GraphLens::Program,
			focused: None,
			program_viewport: GraphViewport::default(),
			domain_viewport: GraphViewport::default(),
			focus_handles: BTreeMap::new(),
			focus_subscriptions: Vec::new(),
			focus_bindings_dirty: true,
			projection_error: None,
		}
	}

	pub(crate) fn selected(&self) -> Option<&EntityId> {
		self.selected.as_ref()
	}

	fn selected_conversation_id(&self) -> Option<EntityId> {
		let selected = self.selected.as_ref()?;
		self.program
			.scene
			.as_ref()
			.and_then(|scene| scene.node(selected))
			.and_then(|node| node.conversation_id.clone())
	}

	pub(crate) fn set_cycle(&mut self, cycle: Option<ProgramCycleDto>, cx: &mut Context<Self>) {
		let Some(cycle) = cycle else {
			self.program.clear();
			self.domain.clear();
			self.domain_title = None;
			self.domain_status = None;
			self.selected = None;
			self.projection_error = None;
			self.sync_focus_handles(cx);
			cx.notify();
			return;
		};

		let program = program_input(&cycle);
		let domain = cycle.domain_pack.as_ref().map(|pack| domain_input(&cycle, pack));
		let program_changed = self.program.update(program);
		let domain_changed = match domain {
			Some(input) => self.domain.update(input),
			None => {
				self.domain.clear();
				Ok(true)
			},
		};
		self.projection_error = match (program_changed, domain_changed) {
			(Ok(_), Ok(_)) => None,
			(Err(error), _) | (_, Err(error)) =>
				Some(format!("Invalid graph projection: {error:?}")),
		};
		if program_changed == Ok(true) {
			self.program_viewport = GraphViewport::default();
		}
		if domain_changed == Ok(true) {
			self.domain_viewport = GraphViewport::default();
		}
		self.domain_title =
			cycle.domain_pack.as_ref().map(|pack| pack.descriptor.name.as_str().to_owned());
		self.domain_status = cycle.domain_pack.as_ref().map(|pack| {
			pack.descriptor
				.capabilities
				.iter()
				.map(|capability| {
					format!(
						"{} · {}",
						capability.id.as_str(),
						match capability.status {
							DomainPackCapabilityStatus::Granted => "GRANTED",
							DomainPackCapabilityStatus::Unavailable => "UNAVAILABLE",
						}
					)
				})
				.collect::<Vec<_>>()
				.join(" · ")
		});

		let selection_valid = self.selected.as_ref().is_some_and(|selected| {
			self.program.scene.as_ref().is_some_and(|scene| scene.contains(selected))
				|| self.domain.scene.as_ref().is_some_and(|scene| scene.contains(selected))
		});
		if !selection_valid {
			self.selected = cycle.nodes.first().map(|node| node.id.clone()).or_else(|| {
				self.program
					.scene
					.as_ref()
					.and_then(|scene| scene.nodes.first())
					.map(|node| node.id.clone())
			});
		}
		self.sync_focus_handles(cx);
		cx.notify();
	}

	pub(crate) fn select(&mut self, id: EntityId, cx: &mut Context<Self>) -> bool {
		let lens = if self.program.scene.as_ref().is_some_and(|scene| scene.contains(&id)) {
			Some(GraphLens::Program)
		} else if self.domain.scene.as_ref().is_some_and(|scene| scene.contains(&id)) {
			Some(GraphLens::Domain)
		} else {
			None
		};
		let Some(lens) = lens else { return false };
		self.set_selection(lens, id, cx);
		true
	}

	fn set_selection(&mut self, lens: GraphLens, id: EntityId, cx: &mut Context<Self>) {
		self.active_lens = lens;
		if self.selected.as_ref() == Some(&id) {
			return;
		}
		self.selected = Some(id);
		cx.emit(ProgramGraphEvent::SelectionChanged);
		cx.notify();
	}

	fn sync_focus_handles(&mut self, cx: &mut Context<Self>) {
		let mut keys = BTreeSet::new();
		for (lens, scene) in [
			(GraphLens::Domain, self.domain.scene.as_ref()),
			(GraphLens::Program, self.program.scene.as_ref()),
		] {
			if let Some(scene) = scene {
				for node in &scene.nodes {
					keys.insert(FocusKey { lens, id: node.id.as_str().to_owned() });
				}
			}
		}
		if keys.len() == self.focus_handles.len()
			&& keys.iter().all(|key| self.focus_handles.contains_key(key))
		{
			return;
		}
		self.focus_handles = keys
			.into_iter()
			.enumerate()
			.map(|(index, key)| {
				let handle = cx.focus_handle().tab_index(index as isize).tab_stop(true);
				(key, handle)
			})
			.collect();
		self.focus_subscriptions.clear();
		self.focus_bindings_dirty = true;
	}

	fn bind_focus_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if !self.focus_bindings_dirty {
			return;
		}
		self.focus_subscriptions.clear();
		for (key, handle) in &self.focus_handles {
			let key = key.clone();
			self.focus_subscriptions.push(cx.on_focus(handle, window, move |surface, _, cx| {
				surface.focused = Some(key.clone());
				if let Some(id) = surface.entity_id_for_key(&key) {
					surface.set_selection(key.lens, id, cx);
				}
			}));
		}
		self.focus_bindings_dirty = false;
	}

	fn entity_id_for_key(&self, key: &FocusKey) -> Option<EntityId> {
		self.scene(key.lens)
			.and_then(|scene| scene.node_by_key(&key.id))
			.map(|node| node.id.clone())
	}

	fn scene(&self, lens: GraphLens) -> Option<&Arc<GraphScene>> {
		match lens {
			GraphLens::Domain => self.domain.scene.as_ref(),
			GraphLens::Program => self.program.scene.as_ref(),
		}
	}

	fn viewport(&self, lens: GraphLens) -> GraphViewport {
		match lens {
			GraphLens::Domain => self.domain_viewport,
			GraphLens::Program => self.program_viewport,
		}
	}

	fn viewport_mut(&mut self, lens: GraphLens) -> &mut GraphViewport {
		match lens {
			GraphLens::Domain => &mut self.domain_viewport,
			GraphLens::Program => &mut self.program_viewport,
		}
	}

	fn update_bounds(&mut self, lens: GraphLens, bounds: Bounds<Pixels>) {
		let viewport = self.viewport_mut(lens);
		if viewport.bounds != Some(bounds) {
			viewport.bounds = Some(bounds);
		}
	}

	fn viewport_command(
		&mut self,
		lens: GraphLens,
		command: ViewportCommand,
		cx: &mut Context<Self>,
	) {
		let Some(scene) = self.scene(lens).cloned() else { return };
		let viewport = self.viewport(lens);
		let bounds = viewport
			.bounds
			.unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(640.0), px(280.0))));
		let current = ViewTransform::for_viewport(viewport, bounds, scene.layout.world_size);
		let mode = match command {
			ViewportCommand::Fit => ViewportMode::Fit,
			ViewportCommand::Reset => ViewportMode::Manual { zoom: 1.0, pan: point(0.0, 0.0) },
			ViewportCommand::ZoomIn => ViewportMode::Manual {
				zoom: (current.zoom * ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM),
				pan: current.pan,
			},
			ViewportCommand::ZoomOut => ViewportMode::Manual {
				zoom: (current.zoom / ZOOM_STEP).clamp(MIN_ZOOM, MAX_ZOOM),
				pan: current.pan,
			},
		};
		let viewport = self.viewport_mut(lens);
		if viewport.mode != mode {
			viewport.mode = mode;
			viewport.drag_anchor = None;
			cx.notify();
		}
	}

	fn handle_scroll(
		&mut self,
		lens: GraphLens,
		event: &ScrollWheelEvent,
		bounds: Bounds<Pixels>,
		cx: &mut Context<Self>,
	) {
		let Some(scene) = self.scene(lens).cloned() else { return };
		self.active_lens = lens;
		let viewport = self.viewport(lens);
		let current = ViewTransform::for_viewport(viewport, bounds, scene.layout.world_size);
		let pixel_delta = match event.delta {
			ScrollDelta::Pixels(delta) => point(f32::from(delta.x), f32::from(delta.y)),
			ScrollDelta::Lines(delta) => point(delta.x * 18.0, delta.y * 18.0),
		};
		let (zoom, pan) = if event.modifiers.control || event.modifiers.platform {
			let factor = if pixel_delta.y > 0.0 {
				1.0 + pixel_delta.y.abs() * 0.01
			} else {
				1.0 / (1.0 + pixel_delta.y.abs() * 0.01)
			};
			let zoom = (current.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
			let center = point(
				f32::from(event.position.x - bounds.origin.x) - current.view_size.width / 2.0,
				f32::from(event.position.y - bounds.origin.y) - current.view_size.height / 2.0,
			);
			let ratio = zoom / current.zoom;
			let pan = point(
				current.pan.x + (center.x - current.pan.x) * (1.0 - ratio),
				current.pan.y + (center.y - current.pan.y) * (1.0 - ratio),
			);
			(zoom, current.clamp_pan(pan))
		} else {
			(current.zoom, current.clamp_pan(current.pan + pixel_delta))
		};
		let mode = ViewportMode::Manual { zoom, pan };
		if self.viewport(lens).mode != mode {
			self.viewport_mut(lens).mode = mode;
			cx.notify();
		}
	}

	fn handle_mouse_down(
		&mut self,
		lens: GraphLens,
		event: &MouseDownEvent,
		cx: &mut Context<Self>,
	) {
		if matches!(event.button, MouseButton::Left | MouseButton::Middle) {
			self.active_lens = lens;
			let viewport = self.viewport_mut(lens);
			if viewport.drag_anchor != Some(event.position) {
				viewport.drag_anchor = Some(event.position);
				cx.notify();
			}
		}
	}

	fn handle_mouse_move(
		&mut self,
		lens: GraphLens,
		event: &MouseMoveEvent,
		cx: &mut Context<Self>,
	) {
		let Some(anchor) = self.viewport(lens).drag_anchor else { return };
		let delta = event.position - anchor;
		if delta.x == px(0.0) && delta.y == px(0.0) {
			return;
		}
		let Some(scene) = self.scene(lens).cloned() else { return };
		let viewport = self.viewport(lens);
		let bounds = viewport
			.bounds
			.unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(640.0), px(280.0))));
		let current = ViewTransform::for_viewport(viewport, bounds, scene.layout.world_size);
		let pan = current.clamp_pan(point(
			current.pan.x + f32::from(delta.x),
			current.pan.y + f32::from(delta.y),
		));
		let viewport = self.viewport_mut(lens);
		viewport.mode = ViewportMode::Manual { zoom: current.zoom, pan };
		viewport.drag_anchor = Some(event.position);
		cx.notify();
	}

	fn handle_mouse_up(&mut self, lens: GraphLens, _: &MouseUpEvent, cx: &mut Context<Self>) {
		let viewport = self.viewport_mut(lens);
		if viewport.drag_anchor.take().is_some() {
			cx.notify();
		}
	}

	fn move_selection(
		&mut self,
		direction: MoveDirection,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let key = self.focused.clone().or_else(|| {
			self.selected.as_ref().map(|selected| FocusKey {
				lens: self.active_lens,
				id: selected.as_str().to_owned(),
			})
		});
		let Some(key) = key else { return };
		let Some(scene) = self.scene(key.lens) else { return };
		let Some(origin) = scene.layout.positions.get(&key.id).copied() else { return };
		let candidate = scene
			.layout
			.positions
			.iter()
			.filter(|(id, _)| id.as_str() != key.id)
			.filter_map(|(id, position)| {
				let dx = position.x - origin.x;
				let dy = position.y - origin.y;
				let (primary, secondary) = match direction {
					MoveDirection::Left if dx < 0.0 => (-dx, dy.abs()),
					MoveDirection::Right if dx > 0.0 => (dx, dy.abs()),
					MoveDirection::Up if dy < 0.0 => (-dy, dx.abs()),
					MoveDirection::Down if dy > 0.0 => (dy, dx.abs()),
					_ => return None,
				};
				Some((primary * 1_000.0 + secondary, id.clone()))
			})
			.min_by(|left, right| left.0.total_cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
			.map(|(_, id)| id);
		let Some(id) = candidate else { return };
		let target = FocusKey { lens: key.lens, id };
		if let Some(handle) = self.focus_handles.get(&target).cloned()
			&& let Some(entity_id) = self.entity_id_for_key(&target)
		{
			self.focused = Some(target);
			self.set_selection(key.lens, entity_id, cx);
			window.focus(&handle, cx);
		}
	}

	fn activate_selected(&mut self, _: &GraphActivate, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected.is_none() {
			return;
		}
		let conversation = self.selected_conversation_id();
		if let Some(conversation) = conversation {
			cx.emit(ProgramGraphEvent::OpenConversation(conversation));
		}
	}

	fn render_lens(
		&self,
		lens: GraphLens,
		title: String,
		status: String,
		fixed_height: Option<f32>,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let scene = self.scene(lens).cloned();
		let selected = self.selected.clone();
		let relations = scene.as_ref().map_or_else(
			|| vec!["Projection unavailable.".to_owned()],
			|scene| scene.relation_readout(selected.as_ref()),
		);
		let zoom = scene
			.as_ref()
			.and_then(|scene| {
				self.viewport(lens).bounds.map(|bounds| {
					ViewTransform::for_viewport(
						self.viewport(lens),
						bounds,
						scene.layout.world_size,
					)
					.zoom
				})
			})
			.unwrap_or(1.0);
		let relation_accessibility =
			format!("{} relations. {}", lens.label(), relations.join(". "));
		let surface = cx.entity();
		let mut panel = div()
			.id(format!("bounded-graph-lens/{}", lens.element_id()))
			.role(Role::Group)
			.aria_label(format!("{} graph lens", lens.label()))
			.min_h_0()
			.flex()
			.flex_col()
			.border_1()
			.border_color(rgba(0xffffff16))
			.rounded(px(11.0))
			.overflow_hidden()
			.bg(rgba(ui_theme::SURFACE_MATERIAL))
			.child(graph_lens_header(lens, title, status, zoom, surface.downgrade()))
			.child(div().flex_1().min_h_0().relative().child(GraphContentElement { surface, lens }))
			.child(graph_relation_path(lens, relation_accessibility, relations));
		if let Some(height) = fixed_height {
			panel = panel.h(px(height)).min_h(px(height));
		} else {
			panel = panel.flex_1();
		}
		panel.into_any_element()
	}

	fn render_unbound_domain(&self) -> AnyElement {
		div()
			.id("program-domain-unbound")
			.role(Role::Group)
			.aria_label("Legacy Program Domain Pack is unbound")
			.h(px(74.0))
			.min_h(px(74.0))
			.px_4()
			.flex()
			.items_center()
			.justify_between()
			.border_1()
			.border_color(rgba(0xf0a64a35))
			.rounded(px(10.0))
			.bg(rgba(0xf0a64a0a))
			.child(
				div()
					.flex()
					.flex_col()
					.gap_1()
					.child(
						div()
							.font_family("SF Mono")
							.text_size(px(8.0))
							.text_color(rgb(AMBER))
							.child("LEGACY PROGRAM · DOMAIN PACK UNBOUND"),
					)
					.child(
						div().text_size(px(9.0)).text_color(rgb(TEXT_MUTED)).child(
							"Bind one built-in Pack once to enable its distinct domain lens.",
						),
					),
			)
			.into_any_element()
	}

	fn on_move_left(&mut self, _: &GraphMoveLeft, window: &mut Window, cx: &mut Context<Self>) {
		self.move_selection(MoveDirection::Left, window, cx);
	}

	fn on_move_right(&mut self, _: &GraphMoveRight, window: &mut Window, cx: &mut Context<Self>) {
		self.move_selection(MoveDirection::Right, window, cx);
	}

	fn on_move_up(&mut self, _: &GraphMoveUp, window: &mut Window, cx: &mut Context<Self>) {
		self.move_selection(MoveDirection::Up, window, cx);
	}

	fn on_move_down(&mut self, _: &GraphMoveDown, window: &mut Window, cx: &mut Context<Self>) {
		self.move_selection(MoveDirection::Down, window, cx);
	}

	fn on_fit(&mut self, _: &GraphFit, _: &mut Window, cx: &mut Context<Self>) {
		self.viewport_command(self.active_lens, ViewportCommand::Fit, cx);
	}

	fn on_zoom_in(&mut self, _: &GraphZoomIn, _: &mut Window, cx: &mut Context<Self>) {
		self.viewport_command(self.active_lens, ViewportCommand::ZoomIn, cx);
	}

	fn on_zoom_out(&mut self, _: &GraphZoomOut, _: &mut Window, cx: &mut Context<Self>) {
		self.viewport_command(self.active_lens, ViewportCommand::ZoomOut, cx);
	}

	fn on_reset(&mut self, _: &GraphReset, _: &mut Window, cx: &mut Context<Self>) {
		self.viewport_command(self.active_lens, ViewportCommand::Reset, cx);
	}
}

impl Render for ProgramGraphSurface {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.bind_focus_updates(window, cx);
		let domain = if self.domain.scene.is_some() {
			self.render_lens(
				GraphLens::Domain,
				self.domain_title.clone().unwrap_or_else(|| "Domain projection".to_owned()),
				self.domain_status.clone().unwrap_or_else(|| "BOUNDED PROJECTION".to_owned()),
				Some(190.0),
				cx,
			)
		} else {
			self.render_unbound_domain()
		};
		let program_status = self.program.scene.as_ref().map_or_else(
			|| "PROJECTION UNAVAILABLE".to_owned(),
			|scene| format!("{} NODES · {} RELATIONS", scene.nodes.len(), scene.edges.len()),
		);
		div()
			.id("program-graph-surface")
			.role(Role::Group)
			.aria_label("Bounded Program Graph Surface")
			.on_action(cx.listener(Self::on_move_left))
			.on_action(cx.listener(Self::on_move_right))
			.on_action(cx.listener(Self::on_move_up))
			.on_action(cx.listener(Self::on_move_down))
			.on_action(cx.listener(Self::activate_selected))
			.on_action(cx.listener(Self::on_fit))
			.on_action(cx.listener(Self::on_zoom_in))
			.on_action(cx.listener(Self::on_zoom_out))
			.on_action(cx.listener(Self::on_reset))
			.flex_1()
			.min_h_0()
			.flex()
			.flex_col()
			.gap_2()
			.child(domain)
			.when_some(self.projection_error.clone(), |surface, error| {
				surface.child(
					div()
						.id("program-graph-projection-error")
						.role(Role::Alert)
						.aria_label(error.clone())
						.px_3()
						.py_2()
						.text_size(px(8.0))
						.text_color(rgb(AMBER))
						.child(error),
				)
			})
			.child(self.render_lens(
				GraphLens::Program,
				"Accepted Program lineage".to_owned(),
				program_status,
				None,
				cx,
			))
	}
}

fn graph_lens_header(
	lens: GraphLens,
	title: String,
	status: String,
	zoom: f32,
	surface: gpui::WeakEntity<ProgramGraphSurface>,
) -> AnyElement {
	let controls = [
		("FIT", "Fit graph to viewport", ViewportCommand::Fit),
		("−", "Zoom out graph", ViewportCommand::ZoomOut),
		("+", "Zoom in graph", ViewportCommand::ZoomIn),
		("1:1", "Reset graph to 100 percent", ViewportCommand::Reset),
	]
	.into_iter()
	.enumerate()
	.map(|(index, (label, aria, command))| {
		viewport_button(lens, index, label, aria, command, surface.clone())
	});
	div()
		.h(px(38.0))
		.min_h(px(38.0))
		.px_3()
		.flex()
		.items_center()
		.justify_between()
		.border_b_1()
		.border_color(rgba(0xffffff10))
		.child(
			div()
				.min_w_0()
				.flex()
				.items_center()
				.gap_2()
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(7.0))
						.text_color(rgb(if lens == GraphLens::Domain { BLUE } else { GREEN }))
						.child(lens.label().to_uppercase()),
				)
				.child(
					div()
						.max_w(px(300.0))
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.text_size(px(9.0))
						.font_weight(FontWeight::SEMIBOLD)
						.child(title),
				)
				.child(
					div()
						.max_w(px(250.0))
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.font_family("SF Mono")
						.text_size(px(6.5))
						.text_color(rgb(TEXT_FAINT))
						.child(status),
				),
		)
		.child(
			div()
				.flex()
				.items_center()
				.gap_1()
				.child(
					div()
						.w(px(40.0))
						.text_right()
						.font_family("SF Mono")
						.text_size(px(6.5))
						.text_color(rgb(TEXT_FAINT))
						.child(format!("{:.0}%", zoom * 100.0)),
				)
				.children(controls),
		)
		.into_any_element()
}

fn graph_relation_path(
	lens: GraphLens,
	accessibility: String,
	relations: Vec<String>,
) -> AnyElement {
	let relation_chips = relations.into_iter().enumerate().map(|(index, relation)| {
		div()
			.id(format!("graph-relation-readout/{}/{index}", lens.element_id()))
			.role(Role::Label)
			.px_2()
			.py_1()
			.rounded(px(5.0))
			.bg(rgba(0xffffff08))
			.font_family("SF Mono")
			.text_size(px(7.0))
			.text_color(rgb(TEXT_FAINT))
			.child(relation)
	});
	div()
		.id(format!("graph-relation-path/{}", lens.element_id()))
		.role(Role::Group)
		.aria_label(accessibility)
		.h(px(27.0))
		.min_h(px(27.0))
		.px_2()
		.flex()
		.items_center()
		.gap_1()
		.overflow_x_scroll()
		.border_t_1()
		.border_color(rgba(0xffffff0c))
		.children(relation_chips)
		.into_any_element()
}

struct GraphContentElement {
	surface: Entity<ProgramGraphSurface>,
	lens: GraphLens,
}

impl IntoElement for GraphContentElement {
	type Element = Self;

	fn into_element(self) -> Self::Element {
		self
	}
}

impl Element for GraphContentElement {
	type PrepaintState = Option<AnyElement>;
	type RequestLayoutState = ();

	fn id(&self) -> Option<ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (LayoutId, Self::RequestLayoutState) {
		(
			window.request_layout(
				Style {
					size: size(relative(1.0).into(), relative(1.0).into()),
					..Default::default()
				},
				[],
				cx,
			),
			(),
		)
	}

	fn prepaint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		_: &mut Self::RequestLayoutState,
		window: &mut Window,
		cx: &mut App,
	) -> Self::PrepaintState {
		let (scene, selected, viewport, handles) = {
			let surface = self.surface.read(cx);
			(
				surface.scene(self.lens).cloned(),
				surface.selected.clone(),
				surface.viewport(self.lens),
				surface.focus_handles.clone(),
			)
		};
		self.surface.update(cx, |surface, _| surface.update_bounds(self.lens, bounds));

		let weak = self.surface.downgrade();
		let dragging = viewport.drag_anchor.is_some();
		let mut content = div()
			.id(format!("graph-content/{}", self.lens.element_id()))
			.role(Role::Group)
			.aria_label(format!(
				"{} graph viewport. Drag or scroll to pan. Command-scroll zooms. Arrow keys move between nodes.",
				self.lens.label()
			))
			.size_full()
			.relative()
			.overflow_hidden()
			.cursor(if dragging { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
			.on_scroll_wheel({
				let weak = weak.clone();
				let lens = self.lens;
				move |event, _, cx| {
					weak.update(cx, |surface, cx| surface.handle_scroll(lens, event, bounds, cx))
						.ok();
				}
			})
			.on_mouse_down(MouseButton::Left, {
				let weak = weak.clone();
				let lens = self.lens;
				move |event, _, cx| {
					weak.update(cx, |surface, cx| surface.handle_mouse_down(lens, event, cx))
						.ok();
				}
			})
			.on_mouse_down(MouseButton::Middle, {
				let weak = weak.clone();
				let lens = self.lens;
				move |event, _, cx| {
					weak.update(cx, |surface, cx| surface.handle_mouse_down(lens, event, cx))
						.ok();
				}
			})
			.on_mouse_move({
				let weak = weak.clone();
				let lens = self.lens;
				move |event, _, cx| {
					weak.update(cx, |surface, cx| surface.handle_mouse_move(lens, event, cx))
						.ok();
				}
			})
			.on_mouse_up(MouseButton::Left, {
				let weak = weak.clone();
				let lens = self.lens;
				move |event, _, cx| {
					weak.update(cx, |surface, cx| surface.handle_mouse_up(lens, event, cx))
						.ok();
				}
			})
			.on_mouse_up(MouseButton::Middle, {
				let weak = weak.clone();
				let lens = self.lens;
				move |event, _, cx| {
					weak.update(cx, |surface, cx| surface.handle_mouse_up(lens, event, cx))
						.ok();
				}
			});

		if let Some(scene) = scene {
			let transform = ViewTransform::for_viewport(viewport, bounds, scene.layout.world_size);
			content = content.child(graph_canvas(Arc::clone(&scene), transform, selected.clone()));
			for (index, edge) in scene.edges.iter().enumerate() {
				if let Some(label) =
					edge_label_element(index, edge, &scene, transform, selected.as_ref())
				{
					content = content.child(label);
				}
			}
			for node in &scene.nodes {
				let key = FocusKey { lens: self.lens, id: node.id.as_str().to_owned() };
				if let Some(handle) = handles.get(&key) {
					content = content.child(graph_node_element(
						node,
						&scene,
						transform,
						selected.as_ref() == Some(&node.id),
						handle.clone(),
						self.lens,
						weak.clone(),
					));
				}
			}
		} else {
			content = content.child(
				div()
					.absolute()
					.inset_0()
					.flex()
					.items_center()
					.justify_center()
					.text_size(px(9.0))
					.text_color(rgb(TEXT_FAINT))
					.child("Projection unavailable"),
			);
		}

		let mut content = content.into_any_element();
		content.prepaint_as_root(bounds.origin, bounds.size.into(), window, cx);
		Some(content)
	}

	fn paint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		_: Bounds<Pixels>,
		_: &mut Self::RequestLayoutState,
		prepaint: &mut Self::PrepaintState,
		window: &mut Window,
		cx: &mut App,
	) {
		if let Some(mut content) = prepaint.take() {
			content.paint(window, cx);
		}
	}
}

fn viewport_button(
	lens: GraphLens,
	index: usize,
	label: &'static str,
	aria: &'static str,
	command: ViewportCommand,
	surface: gpui::WeakEntity<ProgramGraphSurface>,
) -> AnyElement {
	div()
		.id(format!("graph-viewport-control/{}/{index}", lens.element_id()))
		.role(Role::Button)
		.aria_label(format!("{aria} for {}", lens.label()))
		.tab_index(index as isize)
		.w(px(if index == 0 { 35.0 } else { 25.0 }))
		.h(px(23.0))
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(5.0))
		.border_1()
		.border_color(rgba(0xffffff14))
		.font_family("SF Mono")
		.text_size(px(7.0))
		.text_color(rgb(TEXT_MUTED))
		.cursor_pointer()
		.focus_visible(|style| style.border_color(rgb(BLUE)).text_color(rgb(TEXT)))
		.hover(|style| style.bg(rgba(0xffffff0c)).text_color(rgb(TEXT)))
		.on_click({
			let surface = surface.clone();
			move |_, _, cx| {
				surface.update(cx, |surface, cx| surface.viewport_command(lens, command, cx)).ok();
			}
		})
		.on_key_down(move |event, _, cx| {
			if matches!(event.keystroke.key.as_str(), "enter" | "space") {
				cx.stop_propagation();
				surface.update(cx, |surface, cx| surface.viewport_command(lens, command, cx)).ok();
			}
		})
		.child(label)
		.into_any_element()
}

fn graph_node_element(
	node: &GraphNode,
	scene: &GraphScene,
	transform: ViewTransform,
	selected: bool,
	focus: FocusHandle,
	lens: GraphLens,
	surface: gpui::WeakEntity<ProgramGraphSurface>,
) -> AnyElement {
	let Some(world) = scene.layout.positions.get(node.id.as_str()).copied() else {
		return div().into_any_element();
	};
	let screen = transform.world_to_screen(world);
	let width = NODE_WIDTH * transform.zoom;
	let height = NODE_HEIGHT * transform.zoom;
	let compact = transform.zoom < 0.58;
	let node_id = node.id.clone();
	let relations = scene.relation_readout(Some(&node.id)).join(". ");
	let accessibility = format!(
		"{} node, {}, {}, state {}. Identity {}. {}",
		node.kind,
		node.title,
		node.summary,
		node.state,
		node.id.as_str(),
		relations
	);
	div()
		.id(format!("{}-graph-node/{}", lens.element_id(), node.id.as_str()))
		.role(Role::Button)
		.aria_label(accessibility)
		.aria_selected(selected)
		.aria_value(node.state.clone())
		.track_focus(&focus)
		.key_context("ProgramGraphNode")
		.absolute()
		.left(px(screen.x - width / 2.0))
		.top(px(screen.y - height / 2.0))
		.w(px(width.max(22.0)))
		.h(px(height.max(18.0)))
		.p(px((9.0 * transform.zoom).max(3.0)))
		.flex()
		.flex_col()
		.gap(px((5.0 * transform.zoom).max(1.5)))
		.overflow_hidden()
		.border_1()
		.border_color(if selected { rgb(node.color) } else { rgba(0xffffff20) })
		.rounded(px((9.0 * transform.zoom).max(3.0)))
		.bg(if selected { rgba(ui_theme::SURFACE_RAISED_MATERIAL) } else { rgba(0x15131cda) })
		.cursor_pointer()
		.focus_visible(|style| style.border_color(rgb(BLUE)))
		.hover(|style| style.border_color(rgba(0xffffff40)))
		.on_mouse_down(MouseButton::Left, |_, window, cx| {
			window.prevent_default();
			cx.stop_propagation();
		})
		.on_click(move |_, _, cx| {
			surface.update(cx, |surface, cx| surface.set_selection(lens, node_id.clone(), cx)).ok();
		})
		.child(
			div()
				.flex()
				.items_center()
				.justify_between()
				.font_family("SF Mono")
				.text_size(px((7.0 * transform.zoom).max(5.0)))
				.text_color(rgb(node.color))
				.child(node.kind.clone())
				.when(!compact, |row| {
					row.child(div().text_color(rgb(TEXT_FAINT)).child(node.state.clone()))
				}),
		)
		.child(
			div()
				.max_h(px((30.0 * transform.zoom).max(10.0)))
				.overflow_hidden()
				.text_size(px((10.0 * transform.zoom).max(6.0)))
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(rgb(TEXT))
				.child(node.title.clone()),
		)
		.when(!compact, |card| {
			card.child(
				div()
					.max_h(px((43.0 * transform.zoom).max(16.0)))
					.overflow_hidden()
					.text_size(px((7.8 * transform.zoom).max(5.5)))
					.text_color(rgb(TEXT_MUTED))
					.child(node.summary.clone()),
			)
		})
		.into_any_element()
}

fn edge_label_element(
	index: usize,
	edge: &GraphEdge,
	scene: &GraphScene,
	transform: ViewTransform,
	selected: Option<&EntityId>,
) -> Option<AnyElement> {
	if transform.zoom < 0.34 {
		return None;
	}
	let from = scene.layout.positions.get(&edge.identity.from).copied()?;
	let to = scene.layout.positions.get(&edge.identity.to).copied()?;
	let from = transform.world_to_screen(from);
	let to = transform.world_to_screen(to);
	let feedback = scene.layout.feedback_edges.contains(&edge.identity);
	let touches = selected.is_some_and(|selected| {
		edge.identity.from == selected.as_str() || edge.identity.to == selected.as_str()
	});
	let x = (from.x + to.x) / 2.0;
	let y = if feedback {
		from.y.min(to.y) - (34.0 + (index % 3) as f32 * 7.0) * transform.zoom
	} else {
		(from.y + to.y) / 2.0 - 7.0
	};
	let label = if feedback { format!("↶ {}", edge.label) } else { edge.label.clone() };
	Some(
		div()
			.id(("graph-edge-label", index))
			.role(Role::Label)
			.aria_label(format!(
				"Relation from {} to {}: {}{}",
				edge.identity.from,
				edge.identity.to,
				edge.label,
				if feedback { ", feedback path" } else { "" }
			))
			.absolute()
			.left(px(x - 32.0))
			.top(px(y))
			.max_w(px(84.0))
			.px_1()
			.py(px(1.0))
			.overflow_hidden()
			.whitespace_nowrap()
			.text_ellipsis()
			.rounded(px(3.0))
			.bg(rgba(0x0b0a0fdc))
			.font_family("SF Mono")
			.text_size(px((6.6 * transform.zoom).max(5.4)))
			.text_color(rgb(if touches { TEXT_MUTED } else { TEXT_FAINT }))
			.child(label)
			.into_any_element(),
	)
}

fn graph_canvas(
	scene: Arc<GraphScene>,
	transform: ViewTransform,
	selected: Option<EntityId>,
) -> AnyElement {
	canvas(
		|_, _, _| (),
		move |bounds, _, window, _| {
			paint_grid(window, bounds, transform);
			for edge in &scene.edges {
				paint_edge(window, bounds, &scene, edge, transform, selected.as_ref());
			}
			if let Some(selected) = selected.as_ref()
				&& let Some(world) = scene.layout.positions.get(selected.as_str()).copied()
			{
				paint_selection(window, bounds, transform.world_to_screen(world), transform.zoom);
			}
		},
	)
	.absolute()
	.size_full()
	.into_any_element()
}

fn paint_grid(window: &mut Window, bounds: Bounds<Pixels>, transform: ViewTransform) {
	let spacing = (48.0 * transform.zoom).max(24.0);
	let world_origin = transform.world_to_screen(WorldPoint { x: 0.0, y: 0.0 });
	let mut builder = PathBuilder::stroke(px(0.55));
	let mut x = world_origin.x.rem_euclid(spacing);
	while x <= transform.view_size.width {
		builder.move_to(point(bounds.origin.x + px(x), bounds.origin.y));
		builder.line_to(point(bounds.origin.x + px(x), bounds.bottom()));
		x += spacing;
	}
	let mut y = world_origin.y.rem_euclid(spacing);
	while y <= transform.view_size.height {
		builder.move_to(point(bounds.origin.x, bounds.origin.y + px(y)));
		builder.line_to(point(bounds.right(), bounds.origin.y + px(y)));
		y += spacing;
	}
	if let Ok(path) = builder.build() {
		window.paint_path(path, rgba(0xffffff07));
	}
}

fn paint_edge(
	window: &mut Window,
	bounds: Bounds<Pixels>,
	scene: &GraphScene,
	edge: &GraphEdge,
	transform: ViewTransform,
	selected: Option<&EntityId>,
) {
	let Some(from) = scene.layout.positions.get(&edge.identity.from).copied() else { return };
	let Some(to) = scene.layout.positions.get(&edge.identity.to).copied() else { return };
	let from = transform.world_to_screen(from);
	let to = transform.world_to_screen(to);
	let feedback = scene.layout.feedback_edges.contains(&edge.identity);
	let touches = selected.is_some_and(|selected| {
		edge.identity.from == selected.as_str() || edge.identity.to == selected.as_str()
	});
	let color = if touches {
		BLUE
	} else if feedback {
		GREEN
	} else {
		LINE
	};
	let mut builder = PathBuilder::stroke(px(if touches { 1.7 } else { 1.0 }));
	if feedback {
		builder = builder.dash_array(&[px(6.0), px(4.0)]);
	}
	let half_width = NODE_WIDTH * transform.zoom / 2.0;
	let half_height = NODE_HEIGHT * transform.zoom / 2.0;
	let (start, end, control_a, control_b) = if feedback {
		let start = point(from.x, from.y - half_height);
		let end = point(to.x, to.y - half_height);
		let rail = start.y.min(end.y) - 42.0 * transform.zoom;
		(start, end, point(start.x, rail), point(end.x, rail))
	} else {
		let start = point(from.x + half_width, from.y);
		let end = point(to.x - half_width, to.y);
		let control = ((end.x - start.x).abs() * 0.48).max(22.0 * transform.zoom);
		(start, end, point(start.x + control, start.y), point(end.x - control, end.y))
	};
	builder.move_to(offset(bounds, start));
	builder.cubic_bezier_to(
		offset(bounds, end),
		offset(bounds, control_a),
		offset(bounds, control_b),
	);
	if let Ok(path) = builder.build() {
		window.paint_path(path, rgb(color));
	}
	paint_arrow_head(window, bounds, control_b, end, color, touches);
}

fn paint_arrow_head(
	window: &mut Window,
	bounds: Bounds<Pixels>,
	from: Point<f32>,
	to: Point<f32>,
	color: u32,
	selected: bool,
) {
	let dx = to.x - from.x;
	let dy = to.y - from.y;
	let length = (dx * dx + dy * dy).sqrt().max(0.001);
	let ux = dx / length;
	let uy = dy / length;
	let size = if selected { 7.0 } else { 5.5 };
	let left = point(to.x - ux * size - uy * size * 0.6, to.y - uy * size + ux * size * 0.6);
	let right = point(to.x - ux * size + uy * size * 0.6, to.y - uy * size - ux * size * 0.6);
	let mut builder = PathBuilder::stroke(px(if selected { 1.6 } else { 1.0 }));
	builder.move_to(offset(bounds, left));
	builder.line_to(offset(bounds, to));
	builder.line_to(offset(bounds, right));
	if let Ok(path) = builder.build() {
		window.paint_path(path, rgb(color));
	}
}

fn paint_selection(window: &mut Window, bounds: Bounds<Pixels>, center: Point<f32>, zoom: f32) {
	let half_width = NODE_WIDTH * zoom / 2.0 + 5.0;
	let half_height = NODE_HEIGHT * zoom / 2.0 + 5.0;
	let mut builder = PathBuilder::stroke(px(1.6));
	let top_left = point(center.x - half_width, center.y - half_height);
	let top_right = point(center.x + half_width, center.y - half_height);
	let bottom_right = point(center.x + half_width, center.y + half_height);
	let bottom_left = point(center.x - half_width, center.y + half_height);
	builder.move_to(offset(bounds, top_left));
	builder.line_to(offset(bounds, top_right));
	builder.line_to(offset(bounds, bottom_right));
	builder.line_to(offset(bounds, bottom_left));
	builder.close();
	if let Ok(path) = builder.build() {
		window.paint_path(path, rgba(0x8baaf770));
	}
}

fn offset(bounds: Bounds<Pixels>, value: Point<f32>) -> Point<Pixels> {
	point(bounds.origin.x + px(value.x), bounds.origin.y + px(value.y))
}

fn program_input(cycle: &ProgramCycleDto) -> GraphInput {
	let mut nodes = Vec::with_capacity(cycle.nodes.len().saturating_add(1));
	nodes.push(GraphNode {
		id: cycle.program.program_id.clone(),
		kind: "PROGRAM".to_owned(),
		title: cycle.program.name.as_str().to_owned(),
		summary: cycle.program.purpose.as_str().to_owned(),
		state: cycle.program.state.as_str().to_owned(),
		color: TEXT,
		conversation_id: None,
	});
	nodes.extend(cycle.nodes.iter().map(program_node));
	let edges = cycle
		.edges
		.iter()
		.map(|edge| {
			let label = relation_label(edge.kind).to_owned();
			GraphEdge {
				identity: EdgeIdentity {
					from: edge.from.as_str().to_owned(),
					relation: label.clone(),
					to: edge.to.as_str().to_owned(),
				},
				label,
				explicit_feedback: edge.kind == ProgramRelationKind::Validates,
			}
		})
		.collect();
	GraphInput { nodes, edges }
}

fn domain_input(
	cycle: &ProgramCycleDto,
	pack: &decodex_protocol::DomainPackProjectionDto,
) -> GraphInput {
	let mut nodes = Vec::with_capacity(pack.entities.len().saturating_add(1));
	nodes.push(GraphNode {
		id: cycle.program.program_id.clone(),
		kind: "PROGRAM".to_owned(),
		title: cycle.program.name.as_str().to_owned(),
		summary: "Root Program for this bounded Domain Pack projection.".to_owned(),
		state: cycle.program.state.as_str().to_owned(),
		color: TEXT,
		conversation_id: None,
	});
	nodes.extend(pack.entities.iter().map(domain_node));
	let edges = pack
		.relations
		.iter()
		.map(|relation| {
			let label = domain_relation_label(relation.kind.as_str());
			GraphEdge {
				identity: EdgeIdentity {
					from: relation.from.as_str().to_owned(),
					relation: label.clone(),
					to: relation.to.as_str().to_owned(),
				},
				label,
				explicit_feedback: false,
			}
		})
		.collect();
	GraphInput { nodes, edges }
}

fn program_node(node: &ProgramNodeDto) -> GraphNode {
	GraphNode {
		id: node.id.clone(),
		kind: node_kind_label(node.kind).to_owned(),
		title: node.title.as_str().to_owned(),
		summary: node.summary.as_str().to_owned(),
		state: node.state.as_str().to_owned(),
		color: program_node_color(node.kind),
		conversation_id: node.conversation_id.clone(),
	}
}

fn domain_node(entity: &DomainEntityDto) -> GraphNode {
	GraphNode {
		id: entity.id.clone(),
		kind: domain_kind_label(entity.kind.as_str()),
		title: entity.title.as_str().to_owned(),
		summary: entity.summary.as_str().to_owned(),
		state: entity.state.as_str().to_owned(),
		color: if entity.kind.as_str().starts_with("finance.") { GREEN } else { BLUE },
		conversation_id: None,
	}
}

fn compute_layout(input: &GraphInput) -> Result<GraphLayout, GraphLayoutError> {
	if input.nodes.is_empty() {
		return Err(GraphLayoutError::Empty);
	}
	let node_ids =
		input.nodes.iter().map(|node| node.id.as_str().to_owned()).collect::<BTreeSet<_>>();
	if node_ids.len() != input.nodes.len() {
		return Err(GraphLayoutError::DuplicateNode);
	}
	let edge_ids = input.edges.iter().map(|edge| edge.identity.clone()).collect::<BTreeSet<_>>();
	if edge_ids.len() != input.edges.len() {
		return Err(GraphLayoutError::DuplicateEdge);
	}
	if input.edges.iter().any(|edge| {
		!node_ids.contains(&edge.identity.from) || !node_ids.contains(&edge.identity.to)
	}) {
		return Err(GraphLayoutError::DanglingEdge);
	}

	let mut feedback_edges = input
		.edges
		.iter()
		.filter(|edge| edge.explicit_feedback)
		.map(|edge| edge.identity.clone())
		.collect::<BTreeSet<_>>();
	let order = loop {
		let order = topological_order(input, &feedback_edges);
		if order.len() == node_ids.len() {
			break order;
		}
		let ordered = order.iter().cloned().collect::<HashSet<_>>();
		let candidate = input
			.edges
			.iter()
			.filter(|edge| !feedback_edges.contains(&edge.identity))
			.filter(|edge| {
				!ordered.contains(&edge.identity.from) && !ordered.contains(&edge.identity.to)
			})
			.map(|edge| edge.identity.clone())
			.min();
		let Some(candidate) = candidate else { return Err(GraphLayoutError::CycleBreakFailed) };
		feedback_edges.insert(candidate);
	};

	let mut ranks = node_ids.iter().map(|id| (id.clone(), 0usize)).collect::<BTreeMap<_, _>>();
	let mut outgoing = HashMap::<String, Vec<&GraphEdge>>::new();
	let mut incoming = HashMap::<String, Vec<&GraphEdge>>::new();
	for edge in input.edges.iter().filter(|edge| !feedback_edges.contains(&edge.identity)) {
		outgoing.entry(edge.identity.from.clone()).or_default().push(edge);
		incoming.entry(edge.identity.to.clone()).or_default().push(edge);
	}
	for edges in outgoing.values_mut() {
		edges.sort_by(|left, right| left.identity.cmp(&right.identity));
	}
	for edges in incoming.values_mut() {
		edges.sort_by(|left, right| left.identity.cmp(&right.identity));
	}
	for id in &order {
		let rank = incoming.get(id).map_or(0, |edges| {
			edges
				.iter()
				.filter_map(|edge| {
					ranks.get(&edge.identity.from).map(|rank| rank.saturating_add(1))
				})
				.max()
				.unwrap_or(0)
		});
		ranks.insert(id.clone(), rank);
	}
	let maximum_rank = ranks.values().copied().max().unwrap_or(0);
	let mut layers = vec![Vec::<String>::new(); maximum_rank.saturating_add(1)];
	for (id, rank) in &ranks {
		layers[*rank].push(id.clone());
	}
	for layer in &mut layers {
		layer.sort();
	}
	for _ in 0..4 {
		reorder_layers(&mut layers, &incoming, true);
		reorder_layers(&mut layers, &outgoing, false);
	}

	let maximum_rows = layers.iter().map(Vec::len).max().unwrap_or(1);
	let world_width = WORLD_PADDING * 2.0
		+ layers.len() as f32 * NODE_WIDTH
		+ layers.len().saturating_sub(1) as f32 * LAYER_GAP;
	let world_height = WORLD_PADDING * 2.0
		+ maximum_rows as f32 * NODE_HEIGHT
		+ maximum_rows.saturating_sub(1) as f32 * ROW_GAP;
	let mut positions = BTreeMap::new();
	for (rank, layer) in layers.iter().enumerate() {
		let layer_height =
			layer.len() as f32 * NODE_HEIGHT + layer.len().saturating_sub(1) as f32 * ROW_GAP;
		let top = (world_height - layer_height) / 2.0 + NODE_HEIGHT / 2.0;
		for (row, id) in layer.iter().enumerate() {
			positions.insert(
				id.clone(),
				WorldPoint {
					x: WORLD_PADDING + NODE_WIDTH / 2.0 + rank as f32 * (NODE_WIDTH + LAYER_GAP),
					y: top + row as f32 * (NODE_HEIGHT + ROW_GAP),
				},
			);
		}
	}
	Ok(GraphLayout {
		positions,
		#[cfg(test)]
		ranks,
		feedback_edges,
		world_size: size(world_width, world_height),
	})
}

fn topological_order(input: &GraphInput, feedback: &BTreeSet<EdgeIdentity>) -> Vec<String> {
	let mut indegree = input
		.nodes
		.iter()
		.map(|node| (node.id.as_str().to_owned(), 0usize))
		.collect::<BTreeMap<_, _>>();
	let mut outgoing = BTreeMap::<String, Vec<&GraphEdge>>::new();
	for edge in input.edges.iter().filter(|edge| !feedback.contains(&edge.identity)) {
		if let Some(value) = indegree.get_mut(&edge.identity.to) {
			*value = value.saturating_add(1);
		}
		outgoing.entry(edge.identity.from.clone()).or_default().push(edge);
	}
	for edges in outgoing.values_mut() {
		edges.sort_by(|left, right| left.identity.cmp(&right.identity));
	}
	let mut ready = indegree
		.iter()
		.filter(|(_, degree)| **degree == 0)
		.map(|(id, _)| id.clone())
		.collect::<BTreeSet<_>>();
	let mut order = Vec::with_capacity(indegree.len());
	while let Some(id) = ready.pop_first() {
		order.push(id.clone());
		for edge in outgoing.get(&id).into_iter().flatten() {
			if let Some(degree) = indegree.get_mut(&edge.identity.to) {
				*degree = degree.saturating_sub(1);
				if *degree == 0 {
					ready.insert(edge.identity.to.clone());
				}
			}
		}
	}
	order
}

fn reorder_layers(
	layers: &mut [Vec<String>],
	neighbors: &HashMap<String, Vec<&GraphEdge>>,
	forward: bool,
) {
	let layer_positions = layers
		.iter()
		.flat_map(|layer| layer.iter().enumerate().map(|(index, id)| (id.clone(), index as f32)))
		.collect::<HashMap<_, _>>();
	let indices: Box<dyn Iterator<Item = usize>> = if forward {
		Box::new(1..layers.len())
	} else {
		Box::new((0..layers.len().saturating_sub(1)).rev())
	};
	for index in indices {
		layers[index].sort_by(|left, right| {
			let barycenter = |id: &str| {
				let values = neighbors.get(id).into_iter().flatten().filter_map(|edge| {
					let neighbor = if forward { &edge.identity.from } else { &edge.identity.to };
					layer_positions.get(neighbor).copied()
				});
				let values = values.collect::<Vec<_>>();
				if values.is_empty() {
					f32::INFINITY
				} else {
					values.iter().sum::<f32>() / values.len() as f32
				}
			};
			barycenter(left).total_cmp(&barycenter(right)).then_with(|| left.cmp(right))
		});
	}
}

const fn program_node_color(kind: ProgramNodeKind) -> u32 {
	match kind {
		ProgramNodeKind::Signal | ProgramNodeKind::Claim => BLUE,
		ProgramNodeKind::Proposal | ProgramNodeKind::Objective => AMBER,
		ProgramNodeKind::WorkItem | ProgramNodeKind::Run => 0x8d7cf6,
		ProgramNodeKind::Evidence | ProgramNodeKind::Review => GREEN,
	}
}

const fn node_kind_label(kind: ProgramNodeKind) -> &'static str {
	match kind {
		ProgramNodeKind::Signal => "SIGNAL",
		ProgramNodeKind::Claim => "CLAIM",
		ProgramNodeKind::Proposal => "PROPOSAL",
		ProgramNodeKind::Objective => "OBJECTIVE",
		ProgramNodeKind::WorkItem => "WORK ITEM",
		ProgramNodeKind::Run => "CODEX RUN",
		ProgramNodeKind::Evidence => "EVIDENCE",
		ProgramNodeKind::Review => "REVIEW",
	}
}

const fn relation_label(kind: ProgramRelationKind) -> &'static str {
	match kind {
		ProgramRelationKind::Continues => "continues",
		ProgramRelationKind::Observes => "observes",
		ProgramRelationKind::Supports => "supports",
		ProgramRelationKind::Justifies => "justifies",
		ProgramRelationKind::Proposes => "proposes",
		ProgramRelationKind::DecomposesTo => "decomposes",
		ProgramRelationKind::Executes => "executes",
		ProgramRelationKind::Produces => "produces",
		ProgramRelationKind::Validates => "validates",
	}
}

fn domain_kind_label(kind: &str) -> String {
	kind.rsplit('.').next().unwrap_or(kind).replace('_', " ").to_uppercase()
}

fn domain_relation_label(kind: &str) -> String {
	kind.rsplit('.').next().unwrap_or(kind).replace('_', " ")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(feature = "visual-capture")] use crate::programs::Programs;

	fn id(index: usize) -> EntityId {
		EntityId::new(format!("00000000-0000-4000-8000-{index:012}"))
			.expect("bounded graph identity")
	}

	fn node(index: usize) -> GraphNode {
		GraphNode {
			id: id(index),
			kind: "TEST".to_owned(),
			title: format!("Node {index}"),
			summary: "Deterministic graph node".to_owned(),
			state: "ready".to_owned(),
			color: BLUE,
			conversation_id: None,
		}
	}

	fn edge(from: usize, to: usize, relation: &str, feedback: bool) -> GraphEdge {
		GraphEdge {
			identity: EdgeIdentity {
				from: id(from).as_str().to_owned(),
				relation: relation.to_owned(),
				to: id(to).as_str().to_owned(),
			},
			label: relation.to_owned(),
			explicit_feedback: feedback,
		}
	}

	#[test]
	fn layered_layout_places_branches_together_and_merges_after_them() {
		let input = GraphInput {
			nodes: (1..=4).map(node).collect(),
			edges: vec![
				edge(1, 2, "branches", false),
				edge(1, 3, "branches", false),
				edge(2, 4, "merges", false),
				edge(3, 4, "merges", false),
			],
		};
		let layout = compute_layout(&input).expect("branch and merge layout");
		assert_eq!(layout.ranks[id(1).as_str()], 0);
		assert_eq!(layout.ranks[id(2).as_str()], 1);
		assert_eq!(layout.ranks[id(3).as_str()], 1);
		assert_eq!(layout.ranks[id(4).as_str()], 2);
		assert_ne!(layout.positions[id(2).as_str()].y, layout.positions[id(3).as_str()].y);
	}

	#[test]
	fn feedback_edges_preserve_forward_layers_across_three_cycles() {
		let input = GraphInput {
			nodes: (1..=7).map(node).collect(),
			edges: vec![
				edge(1, 2, "observes", false),
				edge(2, 3, "supports", false),
				edge(3, 1, "validates", true),
				edge(3, 4, "continues", false),
				edge(4, 5, "supports", false),
				edge(5, 1, "validates", true),
				edge(5, 6, "continues", false),
				edge(6, 7, "supports", false),
				edge(7, 1, "validates", true),
			],
		};
		let layout = compute_layout(&input).expect("three-cycle layout");
		assert!(layout.ranks[id(2).as_str()] < layout.ranks[id(4).as_str()]);
		assert!(layout.ranks[id(4).as_str()] < layout.ranks[id(6).as_str()]);
		assert_eq!(layout.feedback_edges.len(), 3);
	}

	#[test]
	fn stable_layout_does_not_depend_on_projection_vector_order() {
		let mut first = GraphInput {
			nodes: (1..=5).map(node).collect(),
			edges: vec![
				edge(1, 2, "a", false),
				edge(1, 3, "b", false),
				edge(2, 4, "c", false),
				edge(3, 4, "d", false),
				edge(4, 5, "e", false),
			],
		};
		let mut second = first.clone();
		second.nodes.reverse();
		second.edges.reverse();
		let first_layout = compute_layout(&first).expect("first stable layout");
		let second_layout = compute_layout(&second).expect("second stable layout");
		assert_eq!(first_layout.positions, second_layout.positions);
		assert_eq!(first_layout.ranks, second_layout.ranks);
		first.nodes.rotate_left(2);
		assert_eq!(first.structure_key(), second.structure_key());
	}

	#[test]
	fn undeclared_cycles_are_broken_deterministically() {
		let input = GraphInput {
			nodes: (1..=3).map(node).collect(),
			edges: vec![
				edge(1, 2, "one", false),
				edge(2, 3, "two", false),
				edge(3, 1, "three", false),
			],
		};
		let layout = compute_layout(&input).expect("cycle-break layout");
		assert_eq!(layout.feedback_edges.len(), 1);
		assert_eq!(layout.positions.len(), 3);
	}

	#[test]
	fn viewport_fit_reset_zoom_and_pan_stay_finite() {
		let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(600.0), px(300.0)));
		let world = size(1_800.0, 500.0);
		let fit = ViewTransform::for_viewport(GraphViewport::default(), bounds, world);
		assert!(fit.zoom >= MIN_ZOOM && fit.zoom < 1.0);
		let manual = GraphViewport {
			mode: ViewportMode::Manual { zoom: 1.0, pan: point(99_999.0, -99_999.0) },
			bounds: Some(bounds),
			drag_anchor: None,
		};
		let transform = ViewTransform::for_viewport(manual, bounds, world);
		assert!(transform.pan.x.is_finite() && transform.pan.y.is_finite());
		assert!(transform.pan.x < 99_999.0 && transform.pan.y > -99_999.0);
	}

	#[test]
	fn invalid_and_unavailable_projections_fail_closed() {
		assert_eq!(
			compute_layout(&GraphInput { nodes: Vec::new(), edges: Vec::new() }).unwrap_err(),
			GraphLayoutError::Empty
		);
		let duplicate = GraphInput { nodes: vec![node(1), node(1)], edges: Vec::new() };
		assert_eq!(compute_layout(&duplicate).unwrap_err(), GraphLayoutError::DuplicateNode);
		let dangling = GraphInput { nodes: vec![node(1)], edges: vec![edge(1, 2, "bad", false)] };
		assert_eq!(compute_layout(&dangling).unwrap_err(), GraphLayoutError::DanglingEdge);
	}

	#[test]
	fn layout_cache_reuses_positions_when_only_presentation_data_changes() {
		let mut cache = GraphCache::default();
		let input = GraphInput {
			nodes: vec![node(1), node(2)],
			edges: vec![edge(1, 2, "supports", false)],
		};
		assert_eq!(cache.update(input.clone()), Ok(true));
		let first_positions = cache.layout.as_ref().expect("layout").positions.clone();
		let mut changed = input;
		changed.nodes[1].state = "done".to_owned();
		assert_eq!(cache.update(changed), Ok(false));
		assert_eq!(cache.computations, 1);
		assert_eq!(cache.layout.as_ref().expect("layout").positions, first_positions);
	}

	#[cfg(feature = "visual-capture")]
	#[test]
	fn dto_mapping_preserves_exact_program_and_domain_relation_identities() {
		let cycle = Programs::visual_development_three_cycle()
			.snapshot()
			.cycle
			.expect("development visual cycle");
		let program = program_input(&cycle);
		assert_eq!(program.nodes.len(), cycle.nodes.len() + 1);
		assert_eq!(program.edges.len(), cycle.edges.len());
		for edge in &cycle.edges {
			let label = relation_label(edge.kind);
			assert!(program.edges.iter().any(|mapped| {
				mapped.identity.from == edge.from.as_str()
					&& mapped.identity.to == edge.to.as_str()
					&& mapped.identity.relation == label
			}));
		}
		let layout = compute_layout(&program).expect("mapped Program layout");
		assert_eq!(
			layout.feedback_edges.len(),
			cycle.edges.iter().filter(|edge| edge.kind == ProgramRelationKind::Validates).count()
		);

		let pack = cycle.domain_pack.as_ref().expect("development domain projection");
		let domain = domain_input(&cycle, pack);
		assert_eq!(domain.nodes.len(), pack.entities.len() + 1);
		assert_eq!(domain.edges.len(), pack.relations.len());
		for relation in &pack.relations {
			assert!(domain.edges.iter().any(|mapped| {
				mapped.identity.from == relation.from.as_str()
					&& mapped.identity.to == relation.to.as_str()
					&& mapped.identity.relation == domain_relation_label(relation.kind.as_str())
			}));
		}
	}

	#[cfg(feature = "visual-capture")]
	#[gpui::test]
	fn one_selection_maps_to_exact_conversation_and_rejects_unknown_identity(
		cx: &mut gpui::TestAppContext,
	) {
		let cycle = Programs::visual_development_three_cycle()
			.snapshot()
			.cycle
			.expect("development visual cycle");
		let run = cycle
			.nodes
			.iter()
			.rev()
			.find(|node| node.kind == ProgramNodeKind::Run)
			.expect("latest Run")
			.clone();
		let expected_conversation = run.conversation_id.clone();
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = ProgramGraphSurface::new(cx);
			surface.set_cycle(Some(cycle), cx);
			surface
		});
		surface.update(visual, |surface, cx| {
			assert!(surface.select(run.id.clone(), cx));
		});
		assert_eq!(
			surface.read_with(visual, |surface, _| surface.selected_conversation_id()),
			expected_conversation
		);
		let unknown = EntityId::new("ffffffff-ffff-4fff-8fff-ffffffffffff")
			.expect("bounded unknown identity");
		surface.update(visual, |surface, cx| {
			assert!(!surface.select(unknown, cx));
		});
		assert_eq!(
			surface.read_with(visual, |surface, _| surface.selected().cloned()),
			Some(run.id)
		);
	}

	#[cfg(feature = "visual-capture")]
	#[gpui::test]
	fn keyboard_arrow_moves_focus_and_shared_selection_by_layout(cx: &mut gpui::TestAppContext) {
		cx.update(bind_keys);
		let cycle = Programs::visual_development_three_cycle()
			.snapshot()
			.cycle
			.expect("development visual cycle");
		let first = cycle.nodes.first().expect("first Signal").id.clone();
		let first_key = FocusKey { lens: GraphLens::Program, id: first.as_str().to_owned() };
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = ProgramGraphSurface::new(cx);
			surface.set_cycle(Some(cycle), cx);
			surface
		});
		visual.update(|window, cx| window.draw(cx).clear());
		let focus = surface.read_with(visual, |surface, _| {
			surface.focus_handles.get(&first_key).expect("first node focus").clone()
		});
		visual.update(|window, cx| window.focus(&focus, cx));
		visual.simulate_keystrokes("right");
		let selected = surface.read_with(visual, |surface, _| surface.selected().cloned());
		assert!(selected.is_some());
		assert_ne!(selected, Some(first));
	}
}
