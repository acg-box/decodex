//! Static graph geometry, animated actors, and foreground controls have separate paint ownership.
use super::{AgentSurface, Context, Graph, IntoElement, ParentElement, Styled, mascot, visibility};
use gpui::{AnyElement, Bounds, Entity, InteractiveElement as _, Pixels};
use std::{
	cell::{Cell, RefCell},
	rc::Rc,
};

#[derive(Default)]
pub(super) struct Layers {
	pub graph: RefCell<Option<Rc<Graph>>>,
	pub links: RefCell<Vec<visibility::Link>>,
	pub actors: mascot::Scene,
	bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
	revision: Rc<Cell<u64>>,
	painted_revision: Rc<Cell<u64>>,
}
impl Layers {
	pub(super) fn bounds_recorder(&self) -> impl IntoElement {
		let bounds = self.bounds.clone();
		gpui::canvas(move |value, _, _| bounds.set(Some(value)), |_, _, _, _| {})
			.absolute()
			.inset_0()
	}

	fn element<T: gpui::Render>(
		&self,
		actors: Entity<T>,
		foreground: Entity<T>,
	) -> impl IntoElement {
		let bounds = self.bounds.clone();
		let revision = self.revision.clone();
		let painted_revision = self.painted_revision.clone();
		gpui::canvas(
			move |slot, window, cx| {
				// Prepaint follows the base graph, which publishes this frame's bounds.
				// Camera motion can rebuild the base without notifying the model.
				// Bypass replay for that frame while retaining stable element IDs
				// for pointer capture and focus. The next cached frame captures it.
				let changed = painted_revision.replace(revision.get()) != revision.get();
				let actors = if changed {
					crate::ui_layout::panel(actors).into_any_element()
				} else {
					actors.cached(gpui::StyleRefinement::default().size_full()).into_any_element()
				};
				let mut actors =
					gpui::div().id("graph-actors").size_full().child(actors).into_any_element();
				actors.layout_as_root(slot.size.into(), window, cx);
				actors.prepaint_at(slot.origin, window, cx);
				let foreground = bounds.get().map(|bounds| {
					let foreground = if changed {
						crate::ui_layout::panel(foreground).into_any_element()
					} else {
						crate::ui_layout::pane(foreground, window)
					};
					let mut foreground = gpui::div()
						.id("graph-foreground")
						.size_full()
						.child(foreground)
						.into_any_element();
					foreground.layout_as_root(bounds.size.into(), window, cx);
					foreground.prepaint_at(bounds.origin, window, cx);
					foreground
				});
				(actors, foreground)
			},
			|_, (mut actors, foreground), window, cx| {
				actors.paint(window, cx);
				if let Some(mut foreground) = foreground {
					foreground.paint(window, cx);
				}
			},
		)
		.absolute()
		.inset_0()
	}
}

impl AgentSurface {
	pub(in super::super::super) fn render_work_board(&self, cx: &mut Context<Self>) -> AnyElement {
		let graph = Rc::new(self.relation_graph());
		self.work_board.view.layers.graph.replace(Some(graph.clone()));
		self.relation_canvas(&graph, cx)
	}

	pub(in super::super::super) fn prepare_graph_layers(&self) {
		let layers = &self.work_board.view.layers;
		layers.actors.clear();
		layers.graph.replace(None);
		layers.links.borrow_mut().clear();
		layers.bounds.set(None);
		layers.revision.set(layers.revision.get().wrapping_add(1));
	}

	pub(in super::super::super) fn graph_layers(&self) -> AnyElement {
		self.work_board
			.view
			.layers
			.element(
				self.graph_actors_presentation.clone(),
				self.graph_foreground_presentation.clone(),
			)
			.into_any_element()
	}

	pub(in super::super::super) fn stop_graph_actors(&self) {
		self.work_board.view.layers.actors.clear();
	}

	pub(in super::super::super) fn render_graph_actors(&self) -> AnyElement {
		crate::frame_trace::surface_render("actors");
		self.work_board.view.layers.actors.element().into_any_element()
	}

	pub(in super::super::super) fn render_graph_foreground(
		&self,
		cx: &mut Context<Self>,
	) -> AnyElement {
		crate::frame_trace::surface_render("graph_foreground");
		let layers = &self.work_board.view.layers;
		let Some(graph) = layers.graph.borrow().clone() else {
			return gpui::div().into_any_element();
		};
		let canvas =
			gpui::div().id("relations-foreground").relative().size_full().overflow_hidden();
		self.relation_foreground(canvas, &graph, &layers.links.borrow(), cx).into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[gpui::test]
	fn actor_frames_reuse_graph_layout_and_camera_changes_rebuild_it(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = super::super::super::tests::fixture(cx);
			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;
			s
		});
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_frame);
		surface.update(visual, |s, cx| s.visual_relation_zoom(1., cx));
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(400));
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		let (revision, actors) = surface.read_with(visual, |s, _| {
			let layers = &s.work_board.view.layers;
			assert!(layers.bounds.get().is_some());
			let graph = layers.graph.borrow();
			let _node = graph
				.as_ref()
				.expect("painted graph")
				.nodes
				.iter()
				.find(|n| n.row.is_some())
				.expect("visible actor");
			(layers.revision.get(), s.graph_actors_presentation.clone())
		});
		let selector = "relation-avatar-release";
		let before = visual.debug_bounds(selector).expect("actor layout slot");
		let notifications = Rc::new(Cell::new(0));
		visual.update(|_, cx| {
			let notifications = notifications.clone();
			cx.observe(&surface, move |_, _| notifications.set(notifications.get() + 1)).detach();
		});
		for _ in 0..5 {
			visual.update(|_, cx| cx.notify(actors.entity_id()));
			visual.run_until_parked();
			visual.update(|window, cx| window.draw(cx).clear(cx));
		}
		assert_eq!(notifications.get(), 0, "decorative frames must not notify model observers");
		surface.read_with(visual, |s, _| {
			assert_eq!(
				s.work_board.view.layers.revision.get(),
				revision,
				"actor frames must reuse node, route, and text layout"
			)
		});
		assert_eq!(visual.debug_bounds(selector), Some(before));
		surface.update(visual, |s, cx| {
			s.work_board.view.pan.0 += 64.;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		surface
			.read_with(visual, |s, _| assert!(s.work_board.view.layers.revision.get() > revision));
		assert_ne!(
			visual.debug_bounds(selector),
			Some(before),
			"camera input must update layout immediately"
		);
	}
}
