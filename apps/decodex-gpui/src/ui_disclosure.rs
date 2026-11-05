//! Reveal intrinsic content without feeding last frame's height back into layout.
use std::{cell::Cell, panic::Location, rc::Rc};

use gpui::{
	AnyElement, App, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId,
	IntoElement, LayoutId, Pixels, RenderOnce, Window,
	prelude::{ParentElement as _, Styled as _},
};

use super::{Tween, request_frame};

#[derive(IntoElement)]
pub(crate) struct Disclosure {
	id: ElementId,
	visible: bool,
	child: Box<dyn FnOnce(&mut App) -> AnyElement>,
}

impl RenderOnce for Disclosure {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| State {
			width: Rc::default(),
			progress: Tween::new(if self.visible { 1. } else { 0. }),
		});
		let now = cx.background_executor().now();
		let (progress, moving, width) = state.update(cx, |s, _| {
			s.progress.target(if self.visible { 1. } else { 0. }, now);
			(s.progress.sample(now), s.progress.moving(now), s.width.clone())
		});
		if moving {
			request_frame(window, cx);
		}
		Reveal {
			width,
			progress,
			collapsed: !self.visible && !moving,
			child: (self.visible || moving).then(|| {
				// One flow box includes child margins in both measured and settled layout.
				gpui::div()
					.w_full()
					.min_w_0()
					.flex_none()
					.flex()
					.flex_col()
					.child((self.child)(cx))
					.into_any_element()
			}),
		}
	}
}

struct State {
	width: Rc<Cell<Pixels>>,
	progress: Tween,
}

struct Reveal {
	width: Rc<Cell<Pixels>>,
	progress: f32,
	collapsed: bool,
	child: Option<AnyElement>,
}

impl IntoElement for Reveal {
	type Element = Self;

	fn into_element(self) -> Self {
		self
	}
}

impl Element for Reveal {
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
		let mut style = gpui::Style {
			display: if self.collapsed { gpui::Display::None } else { gpui::Display::Block },
			size: gpui::size(gpui::relative(1.).into(), gpui::Length::Auto),
			flex_shrink: 0.,
			..Default::default()
		};
		let mut child_layout = None;
		if let Some(child) = &mut self.child {
			if self.progress == 1. {
				// First entry and settled details participate in normal layout.
				child_layout = Some(child.request_layout(window, cx));
			} else if self.progress > 0. {
				// The opening frame resolves the slot's width at zero height. Measure
				// fresh content before laying out siblings, outside a constrained height.
				let size = child.layout_as_root(
					gpui::size(self.width.get().into(), gpui::AvailableSpace::MinContent),
					window,
					cx,
				);
				style.size.height = (size.height * self.progress).into();
			} else {
				style.size.height = gpui::px(0.).into();
			}
		} else {
			style.size.height = gpui::px(0.).into();
		}
		(window.request_layout(style, child_layout, cx), ())
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
		if !self.collapsed {
			self.width.set(bounds.size.width);
		}
		if let Some(child) = &mut self.child {
			window.with_content_mask(Some(ContentMask { bounds }), |window| {
				if self.progress == 1. {
					child.prepaint(window, cx);
				} else {
					if self.progress == 0. {
						child.layout_as_root(
							gpui::size(bounds.size.width.into(), gpui::AvailableSpace::MinContent),
							window,
							cx,
						);
					}
					child.prepaint_at(bounds.origin, window, cx);
				}
			});
		}
	}

	fn paint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		bounds: Bounds<Pixels>,
		_: &mut (),
		_: &mut (),
		window: &mut Window,
		cx: &mut App,
	) {
		if let Some(child) = &mut self.child {
			window
				.with_content_mask(Some(ContentMask { bounds }), |window| child.paint(window, cx));
		}
	}
}

pub(crate) fn disclosure(
	id: impl Into<ElementId>,
	visible: bool,
	child: impl IntoElement,
) -> Disclosure {
	let child = child.into_any_element();
	disclosure_lazy(id, visible, move |_| child)
}

/// Do not construct Markdown and tool details for fully collapsed processes.
pub(crate) fn disclosure_lazy(
	id: impl Into<ElementId>,
	visible: bool,
	child: impl FnOnce(&mut App) -> AnyElement + 'static,
) -> Disclosure {
	Disclosure { id: id.into(), visible, child: Box::new(child) }
}

/// Settings share intrinsic disclosure motion and keep their existing row layout.
pub(crate) fn settings_disclosure(
	id: impl Into<ElementId>,
	expanded: bool,
	content: impl FnOnce() -> AnyElement,
) -> Disclosure {
	disclosure(id, expanded, content())
}

#[cfg(test)]
mod tests {
	use std::{cell::Cell, rc::Rc, time::Duration};

	use gpui::{Bounds, Context, IntoElement, Pixels, Render, Window, prelude::*};

	struct Section {
		visible: bool,
		height: f32,
		built: Rc<Cell<usize>>,
		mask: Rc<Cell<Bounds<Pixels>>>,
	}
	impl Render for Section {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			let height = self.height;
			let built = self.built.clone();
			let mask = self.mask.clone();
			gpui::div()
				.w(gpui::px(300.))
				.flex()
				.flex_col()
				.child(super::disclosure_lazy("details", self.visible, move |_| {
					built.set(built.get() + 1);
					gpui::div()
						.id("body")
						.debug_selector(|| "body".into())
						.w_full()
						.mt(gpui::px(8.))
						.h(gpui::px(height))
						.child(
							gpui::canvas(
								|_, _, _| (),
								move |_, _, window, _| mask.set(window.content_mask().bounds),
							)
							.size_full(),
						)
						.into_any_element()
				}))
				.child(gpui::div().id("after").debug_selector(|| "after".into()).h(gpui::px(20.)))
		}
	}

	#[gpui::test]
	fn disclosure_measures_live_content_and_clips_each_interruptible_frame(
		cx: &mut gpui::TestAppContext,
	) {
		let built = Rc::new(Cell::new(0));
		let mask = Rc::new(Cell::new(Bounds::default()));
		let (section, visual) = cx.add_window_view(|_, _| Section {
			visible: false,
			height: 160.,
			built: built.clone(),
			mask: mask.clone(),
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(built.get(), 0, "closed details do not construct their content");
		section.update(visual, |s, cx| {
			s.visible = true;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("after").unwrap().top(), gpui::px(0.));
		assert_eq!(visual.debug_bounds("body").unwrap().size.width, gpui::px(300.));
		assert_eq!(mask.get().size.height, gpui::px(0.), "opening content starts fully clipped");
		visual.executor().advance_clock(Duration::from_millis(50));
		visual.update(crate::ui_frame::draw_frame);
		let opening = visual.debug_bounds("after").unwrap().top();
		assert!((opening - gpui::px(97.125)).abs() <= gpui::px(1.));
		assert_eq!(visual.debug_bounds("body").unwrap().size.height, gpui::px(160.));
		assert_eq!(mask.get().bottom(), opening, "content is clipped above its next sibling");
		section.update(visual, |s, cx| {
			s.height = 240.;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let resized = visual.debug_bounds("after").unwrap().top();
		assert!((resized - gpui::px(143.375)).abs() <= gpui::px(1.));
		assert_eq!(mask.get().bottom(), resized, "new content height applies in this frame");
		section.update(visual, |s, cx| {
			s.visible = false;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("after").unwrap().top(), resized);
		visual.executor().advance_clock(Duration::from_millis(50));
		visual.update(crate::ui_frame::draw_frame);
		let closing = visual.debug_bounds("after").unwrap().top();
		assert!(closing > gpui::px(0.) && closing < resized);
		section.update(visual, |s, cx| {
			s.visible = true;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(visual.debug_bounds("after").unwrap().top(), closing);
		visual.executor().advance_clock(Duration::from_millis(200));
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("after").unwrap().top(), gpui::px(248.));
		visual.update(|window, cx| assert_eq!(window.simulate_next_frame(cx), 0));
		section.update(visual, |s, cx| {
			s.visible = false;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		visual.executor().advance_clock(Duration::from_millis(200));
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("after").unwrap().top(), gpui::px(0.));
		assert!(visual.debug_bounds("body").is_none());
		let closed = built.get();
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(built.get(), closed);
		visual.update(|window, cx| assert_eq!(window.simulate_next_frame(cx), 0));
	}
}
