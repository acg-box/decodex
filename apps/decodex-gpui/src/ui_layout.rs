//! Fixed panel bounds isolate intrinsic measurement without replaying old scenes.
use gpui::{
	AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
	Pixels, Window, prelude::Styled as _,
};

/// The pinned GPUI cache does not replay accessibility nodes. Keep live semantic
/// trees when assistive technology is active, with the same fixed layout bounds.
pub(crate) fn pane<T: gpui::Render>(view: gpui::Entity<T>, window: &Window) -> AnyElement {
	if window.is_a11y_active() {
		panel(view).into_any_element()
	} else {
		view.cached(gpui::StyleRefinement::default().size_full().flex_1().min_w_0().min_h_0())
			.into_any_element()
	}
}

pub(crate) fn panel(child: impl IntoElement) -> impl IntoElement {
	Panel {
		slot: gpui::div().size_full().flex_1().min_w_0().min_h_0().into_any_element(),
		child: child.into_any_element(),
	}
}

/// Keep exact layout heights while excluding distant content from paint and hit testing.
pub(crate) fn visible(
	child: impl IntoElement,
	keep_alive: impl Fn(&Window, &App) -> bool + 'static,
) -> impl IntoElement {
	Visible { child: child.into_any_element(), keep_alive: Box::new(keep_alive) }
}

type VisibilityGuard = dyn Fn(&Window, &App) -> bool;

struct Visible {
	child: AnyElement,
	keep_alive: Box<VisibilityGuard>,
}
impl IntoElement for Visible {
	type Element = Self;

	fn into_element(self) -> Self {
		self
	}
}
impl Element for Visible {
	type PrepaintState = bool;
	type RequestLayoutState = ();

	fn id(&self) -> Option<gpui::ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
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
		bounds: Bounds<Pixels>,
		_: &mut (),
		window: &mut Window,
		cx: &mut App,
	) -> bool {
		// Match native history windowing: keep the focused turn and overscan
		// ready as scrolling approaches the viewport edge, including with AX active.
		let visible = (self.keep_alive)(window, cx)
			|| !bounds.intersect(&window.content_mask().bounds.dilate(gpui::px(160.))).is_empty();
		if visible {
			self.child.prepaint(window, cx);
		}
		visible
	}

	fn paint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		_: Bounds<Pixels>,
		_: &mut (),
		visible: &mut bool,
		window: &mut Window,
		cx: &mut App,
	) {
		if *visible {
			self.child.paint(window, cx);
		}
	}
}

struct Panel {
	slot: AnyElement,
	child: AnyElement,
}
impl IntoElement for Panel {
	type Element = Self;

	fn into_element(self) -> Self {
		self
	}
}
impl Element for Panel {
	type PrepaintState = ();
	type RequestLayoutState = ();

	fn id(&self) -> Option<gpui::ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (LayoutId, ()) {
		(self.slot.request_layout(window, cx), ())
	}

	fn prepaint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		_: &mut (),
		window: &mut Window,
		cx: &mut App,
	) {
		// Measure only after the parent has resolved its available panel size.
		// Fresh prepaint preserves scroll offsets, hit targets, and accessibility.
		self.child.layout_as_root(bounds.size.into(), window, cx);
		self.child.prepaint_at(bounds.origin, window, cx);
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
		self.child.paint(window, cx);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::{
		Context, Render,
		prelude::{InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _},
	};

	struct ScrollFixture {
		scroll: gpui::ScrollHandle,
		counts: std::rc::Rc<Vec<std::cell::Cell<usize>>>,
		focus: gpui::FocusHandle,
	}
	impl Render for ScrollFixture {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			gpui::div()
				.id("rows")
				.w(gpui::px(200.))
				.h(gpui::px(200.))
				.flex()
				.flex_col()
				.overflow_y_scroll()
				.track_scroll(&self.scroll)
				.children((0..self.counts.len()).map(|index| {
					let counts = self.counts.clone();
					let focus = self.focus.clone();
					let mut row = gpui::div()
						.w_full()
						.h(gpui::px(50.))
						.flex_none()
						.child(gpui::div().size_full())
						.on_children_prepainted(move |_, _, _| {
							counts[index].set(counts[index].get() + 1)
						})
						.id(gpui::SharedString::from(format!("row-{index}")));
					if index == 0 {
						row = row.track_focus(&focus);
					}
					super::visible(row, move |window, cx| {
						index == 0 && focus.contains_focused(window, cx)
					})
				}))
		}
	}

	#[gpui::test]
	fn distant_content_skips_prepaint_without_changing_scroll_height_or_selection(
		cx: &mut gpui::TestAppContext,
	) {
		let counts = std::rc::Rc::new((0..20).map(|_| std::cell::Cell::new(0)).collect::<Vec<_>>());
		let scroll = gpui::ScrollHandle::new();
		let (view, visual) = cx.add_window_view(|_, cx| ScrollFixture {
			scroll: scroll.clone(),
			counts: counts.clone(),
			focus: cx.focus_handle(),
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(counts[0].get() > 0);
		assert_eq!(counts[19].get(), 0);
		assert_eq!(scroll.max_offset().y, gpui::px(800.));
		let first = counts[0].get();
		view.update(visual, |_, cx| {
			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-750.)));
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert_eq!(counts[0].get(), first);
		assert!(counts[19].get() > 0);
		assert_eq!(scroll.max_offset().y, gpui::px(800.));
		view.update(visual, |_, cx| {
			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		let focused_count = counts[0].get();
		let focus = view.read_with(visual, |s, _| s.focus.clone());
		visual.update(|window, cx| focus.focus(window, cx));
		view.update(visual, |_, cx| {
			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-750.)));
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(
			counts[0].get() > focused_count,
			"selected content must retain its focus and handlers"
		);
		assert_eq!(scroll.max_offset().y, gpui::px(800.));
	}
	struct Fixture {
		width: f32,
		height: f32,
		clicked: bool,
	}
	impl Render for Fixture {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			gpui::div().w(gpui::px(self.width)).h(gpui::px(self.height)).p_2().child(panel(
				gpui::div()
					.id("content")
					.debug_selector(|| "panel-content".into())
					.size_full()
					.on_click(cx.listener(|s, _, _, _| s.clicked = true)),
			))
		}
	}
	#[gpui::test]
	fn resizing_rebuilds_geometry_and_hit_targets(cx: &mut gpui::TestAppContext) {
		let (fixture, visual) =
			cx.add_window_view(|_, _| Fixture { width: 200., height: 160., clicked: false });
		visual.update(|window, cx| window.draw(cx).clear(cx));
		let first = visual.debug_bounds("panel-content").unwrap();
		assert_eq!(first.origin, gpui::point(gpui::px(8.), gpui::px(8.)));
		assert_eq!(first.size, gpui::size(gpui::px(184.), gpui::px(144.)));
		fixture.update(visual, |s, cx| {
			s.width = 300.;
			s.height = 220.;
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		let next = visual.debug_bounds("panel-content").unwrap();
		assert_eq!(next.size, gpui::size(gpui::px(284.), gpui::px(204.)));
		visual.simulate_click(gpui::point(gpui::px(280.), gpui::px(190.)), Default::default());
		assert!(fixture.read_with(visual, |s, _| s.clicked));
	}
}
