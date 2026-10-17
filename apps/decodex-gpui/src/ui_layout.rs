//! Fixed panel bounds isolate intrinsic measurement without replaying old scenes.
use gpui::{
	AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
	Pixels, Window, prelude::Styled as _,
};

pub(crate) fn panel(child: impl IntoElement) -> impl IntoElement {
	Panel {
		slot: gpui::div().size_full().flex_1().min_w_0().min_h_0().into_any_element(),
		child: child.into_any_element(),
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
