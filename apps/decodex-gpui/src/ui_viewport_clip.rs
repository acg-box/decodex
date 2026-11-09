//! Reveal a fixed layout through the actual viewport, without a text animation.
use gpui::{
	AnyElement, App, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId,
	IntoElement, LayoutId, Pixels, Window, prelude::*,
};
use std::{cell::Cell, panic::Location, rc::Rc};

#[derive(Clone, Default)]
pub(crate) struct Viewport(Rc<Cell<Bounds<Pixels>>>);
impl Viewport {
	pub(crate) fn from_bounds(bounds: Bounds<Pixels>) -> Self {
		Self(Rc::new(Cell::new(bounds)))
	}

	pub(crate) fn anchor(&self) -> impl IntoElement {
		let viewport = self.0.clone();
		gpui::div().absolute().size_full().child(
			gpui::canvas(move |bounds, _, _| viewport.set(bounds), |_, _, _, _| {}).size_full(),
		)
	}

	pub(crate) fn clip(&self, right_inset: f32, child: impl IntoElement) -> AnyElement {
		Clip { viewport: self.0.clone(), right_inset, child: child.into_any_element() }
			.into_any_element()
	}
}

struct Clip {
	viewport: Rc<Cell<Bounds<Pixels>>>,
	right_inset: f32,
	child: AnyElement,
}

impl Clip {
	fn mask(&self) -> ContentMask<Pixels> {
		let mut bounds = self.viewport.get();
		bounds.size.width = (bounds.size.width - gpui::px(self.right_inset)).max(gpui::px(0.));
		ContentMask { bounds }
	}
}

impl IntoElement for Clip {
	type Element = Self;

	fn into_element(self) -> Self {
		self
	}
}

impl Element for Clip {
	type PrepaintState = ();
	type RequestLayoutState = ();

	fn id(&self) -> Option<ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (LayoutId, ()) {
		(self.child.request_layout(window, cx), ())
	}

	fn prepaint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		_: Bounds<Pixels>,
		_: &mut (),
		window: &mut Window,
		cx: &mut App,
	) {
		window.with_content_mask(Some(self.mask()), |window| self.child.prepaint(window, cx));
	}

	fn paint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		_: Bounds<Pixels>,
		_: &mut (),
		_: &mut (),
		window: &mut Window,
		cx: &mut App,
	) {
		window.with_content_mask(Some(self.mask()), |window| self.child.paint(window, cx));
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::{Context, Render, TestAppContext};
	struct Probe {
		width: f32,
		observed: Rc<Cell<(Bounds<Pixels>, Bounds<Pixels>)>>,
	}
	impl Render for Probe {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			let viewport = Viewport::default();
			let observed = self.observed.clone();
			gpui::div()
				.w(gpui::px(self.width))
				.h(gpui::px(60.))
				.relative()
				.overflow_hidden()
				.child(viewport.anchor())
				.child(
					gpui::div().w(gpui::px(240.)).h(gpui::px(28.)).child(
						viewport.clip(
							6.,
							gpui::canvas(
								|_, _, _| (),
								move |bounds, _, window, _| {
									observed.set((bounds, window.content_mask().bounds))
								},
							)
							.w(gpui::px(240.))
							.h(gpui::px(28.)),
						),
					),
				)
		}
	}
	#[gpui::test]
	fn a_fixed_canvas_follows_the_actual_edge_without_advancing_time(cx: &mut TestAppContext) {
		let observed = Rc::new(Cell::new((Bounds::default(), Bounds::default())));
		let (view, visual) =
			cx.add_window_view(|_, _| Probe { width: 80., observed: observed.clone() });
		let mut origin = None;
		for width in [80., 100., 60., 160.] {
			view.update(visual, |view, cx| {
				view.width = width;
				cx.notify();
			});
			visual.update(crate::ui_frame::draw_current_frame);
			let (canvas, mask) = observed.get();
			assert_eq!(canvas.size.width, gpui::px(240.));
			assert_eq!(*origin.get_or_insert(canvas.origin), canvas.origin);
			assert_eq!(mask.right() - canvas.left(), gpui::px(width - 6.));
		}
	}

	struct CoveredPlane(Rc<Cell<usize>>);
	impl Render for CoveredPlane {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			let covered = self.0.clone();
			let uncovered = self.0.clone();
			Viewport::from_bounds(Bounds::new(
				gpui::point(gpui::px(80.), gpui::px(0.)),
				gpui::size(gpui::px(160.), gpui::px(60.)),
			))
			.clip(
				0.,
				gpui::div()
					.debug_selector(|| "covered-plane".into())
					.w(gpui::px(240.))
					.h(gpui::px(60.))
					.flex()
					.child(
						gpui::div()
							.id("covered-action")
							.w(gpui::px(80.))
							.h_full()
							.on_click(move |_, _, _| covered.set(covered.get() + 1)),
					)
					.child(
						gpui::div()
							.id("uncovered-action")
							.w(gpui::px(160.))
							.h_full()
							.on_click(move |_, _, _| uncovered.set(uncovered.get() + 1)),
					),
			)
		}
	}

	#[gpui::test]
	fn clipping_covered_controls_preserves_layout_and_uncovered_input(cx: &mut TestAppContext) {
		let hits = Rc::new(Cell::new(0));
		let (_, visual) = cx.add_window_view(|_, _| CoveredPlane(hits.clone()));
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("covered-plane").unwrap().size.width, gpui::px(240.));
		visual.simulate_click(gpui::point(gpui::px(40.), gpui::px(30.)), Default::default());
		assert_eq!(hits.get(), 0);
		visual.simulate_click(gpui::point(gpui::px(160.), gpui::px(30.)), Default::default());
		assert_eq!(hits.get(), 1);
	}
}
