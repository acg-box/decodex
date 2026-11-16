//! Passive hints share the overlay material and geometry. GPUI owns their lifetime.
use std::rc::Rc;

use gpui::{
	AnyElement, AnyView, App, AppContext as _, Context, IntoElement, Pixels, Render, RenderOnce,
	SharedString, Window,
	prelude::{InteractiveElement as _, ParentElement as _, Styled as _},
};

use super::PopoverContent;
use crate::ui_theme;

pub(crate) trait HoverHint {
	fn hover_hint(self, text: impl Into<SharedString>) -> Self;
}

impl<T: gpui::StatefulInteractiveElement + gpui::ParentElement> HoverHint for T {
	fn hover_hint(self, text: impl Into<SharedString>) -> Self {
		let text = text.into();
		let anchor = Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
		let measured = anchor.clone();
		self.tooltip(move |_, cx| {
			let text = text.clone();
			build_hint(cx, anchor.clone(), move |_, _| HintText(text.clone()).into_any_element())
		})
		.child(
			gpui::canvas(move |bounds, _, _| measured.set(bounds), |_, _, _, _| {})
				.absolute()
				.inset_0(),
		)
	}
}

#[derive(IntoElement)]
struct HintText(SharedString);

impl RenderOnce for HintText {
	fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
		let style = window.text_style();
		let font_size = style.font_size.to_pixels(window.rem_size());
		let runs = [style.to_run(self.0.len())];
		let rows = |width: Pixels| {
			window
				.text_system()
				.shape_text(self.0.clone(), font_size, &runs, Some(width), None)
				.map(|lines| {
					lines.iter().map(|line| line.wrap_boundaries().len() + 1).sum::<usize>()
				})
		};
		let max = gpui::px(343.);
		let mut width = max;
		if let Ok(target) = rows(max) {
			let mut lower = gpui::px(1.);
			while width - lower > gpui::px(1.) {
				let candidate = (lower + width) / 2.;
				if rows(candidate).is_ok_and(|count| count <= target) {
					width = candidate;
				} else {
					lower = candidate;
				}
			}
		}
		// GPUI reports the wrapping constraint as the line width. Fit the constraint
		// itself to the smallest width that retains the original number of rows.
		let width = gpui::px((f32::from(width).ceil() + 1.).min(f32::from(max)));
		gpui::div().w(width).whitespace_normal().child(self.0)
	}
}

pub(crate) fn tooltip_at(
	cx: &mut App,
	anchor: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
	content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
) -> AnyView {
	build_hint(cx, anchor, content)
}

fn build_hint(
	cx: &mut App,
	anchor: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
	content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
) -> AnyView {
	cx.new(|_cx| {
		#[cfg(all(target_os = "macos", not(test)))]
		_cx.on_release(|hint: &mut Hint, cx| {
			if let Some(parent) = hint.parent {
				super::native_popover::hide_hint(parent, hint.id.clone(), cx);
			}
		})
		.detach();
		Hint {
			anchor,
			#[cfg(all(target_os = "macos", not(test)))]
			id: gpui::ElementId::View(_cx.entity_id()),
			#[cfg(all(target_os = "macos", not(test)))]
			parent: None,
			content: Rc::new(move |window, cx| {
				ui_theme::menu_outline(gpui::div())
					.debug_selector(|| "hover-hint-surface".into())
					.font_family(ui_theme::FONT_FAMILY)
					.text_color(ui_theme::rgb(ui_theme::TEXT))
					.text_size(gpui::px(ui_theme::COMPACT_BODY_SIZE))
					.line_height(gpui::px(ui_theme::COMPACT_BODY_LINE_HEIGHT))
					.px(gpui::px(8.))
					.py(gpui::px(6.))
					.max_w(gpui::px(360.))
					.child(content(window, cx))
					.into_any_element()
			}),
		}
	})
	.into()
}

struct Hint {
	anchor: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
	#[cfg(all(target_os = "macos", not(test)))]
	id: gpui::ElementId,
	#[cfg(all(target_os = "macos", not(test)))]
	parent: Option<gpui::AnyWindowHandle>,
	content: Rc<PopoverContent>,
}

impl Render for Hint {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let hint = self.surface(window, cx);
		crate::ui_layout::hover_surface(self.anchor.get(), hint).into_any_element()
	}
}

impl Hint {
	fn surface(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		#[cfg(all(target_os = "macos", not(test)))]
		if super::native_popover::available() {
			self.parent = Some(window.window_handle());
			return super::native_popover::render_hint(
				self.id.clone(),
				self.content.clone(),
				window,
				cx,
			);
		}
		super::popover_elevation(
			gpui::div().rounded(gpui::px(ui_theme::POPOVER_RADIUS)).bg(ui_theme::popover_surface()),
		)
		.child((self.content)(window, cx))
		.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::HoverHint as _;
	use gpui::{
		Context, IntoElement, Render, Window,
		prelude::{
			InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _,
			Styled as _,
		},
	};
	use std::time::Duration;

	struct HintOwner {
		mounted: bool,
		clicks: usize,
		text: &'static str,
	}
	impl Render for HintOwner {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			let text = self.text;
			gpui::div().size_full().children(self.mounted.then(|| {
				gpui::div()
					.id("hint-trigger")
					.debug_selector(|| "hint-trigger".into())
					.w(gpui::px(80.))
					.h(gpui::px(28.))
					.child("Copy")
					.hover_hint(text)
					.on_click(cx.listener(|owner, _, _, _| owner.clicks += 1))
			}))
		}
	}

	fn settle(visual: &mut gpui::VisualTestContext, delay: Duration) {
		visual.run_until_parked();
		visual.executor().advance_clock(delay);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);
	}

	#[gpui::test]
	fn hints_wait_for_dwell_and_leave_the_trigger_interactive(cx: &mut gpui::TestAppContext) {
		let (owner, visual) =
			cx.add_window_view(|_, _| HintOwner { mounted: true, clicks: 0, text: "Copy (⌘C)" });
		visual.update(crate::ui_frame::draw_current_frame);
		let trigger = visual.debug_bounds("hint-trigger").expect("trigger");
		visual.simulate_mouse_move(trigger.center(), gpui::MouseButton::Left, Default::default());
		settle(visual, Duration::from_millis(499));
		assert!(visual.debug_bounds("hover-hint-surface").is_none());
		settle(visual, Duration::from_millis(1));
		let hint = visual.debug_bounds("hover-hint-surface").expect("hint after dwell");
		assert_eq!(hint.top(), trigger.bottom() + gpui::px(8.));
		visual.simulate_mouse_move(
			gpui::point(trigger.right() - gpui::px(2.), trigger.center().y),
			None,
			Default::default(),
		);
		settle(visual, Duration::from_millis(500));
		assert_eq!(
			visual.debug_bounds("hover-hint-surface"),
			Some(hint),
			"movement inside a control does not move its hint"
		);
		assert!(hint.size.width > gpui::px(30.) && hint.size.height >= gpui::px(18.));
		assert_eq!(
			visual.debug_bounds("hint-trigger"),
			Some(trigger),
			"a hint must not reflow its owner"
		);
		visual.simulate_click(trigger.center(), Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		assert_eq!(owner.read_with(visual, |owner, _| owner.clicks), 1);
	}

	#[gpui::test]
	fn leaving_or_removing_an_owner_cancels_its_hint(cx: &mut gpui::TestAppContext) {
		let (owner, visual) =
			cx.add_window_view(|_, _| HintOwner { mounted: true, clicks: 0, text: "Copy (⌘C)" });
		visual.update(crate::ui_frame::draw_current_frame);
		let trigger = visual.debug_bounds("hint-trigger").expect("trigger");
		let away = gpui::point(gpui::px(300.), gpui::px(300.));
		visual.simulate_mouse_move(trigger.center(), gpui::MouseButton::Left, Default::default());
		settle(visual, Duration::from_millis(200));
		visual.simulate_mouse_move(away, gpui::MouseButton::Left, Default::default());
		settle(visual, Duration::from_secs(1));
		assert!(visual.debug_bounds("hover-hint-surface").is_none());
		visual.simulate_mouse_move(trigger.center(), gpui::MouseButton::Left, Default::default());
		settle(visual, Duration::from_millis(500));
		assert!(visual.debug_bounds("hover-hint-surface").is_some());
		owner.update(visual, |owner, cx| {
			owner.mounted = false;
			cx.notify();
		});
		settle(visual, Duration::from_secs(1));
		assert!(
			visual.debug_bounds("hover-hint-surface").is_none(),
			"an unmounted control cannot leave a hint"
		);
	}
	#[gpui::test]
	fn long_details_wrap_inside_the_shared_hint_width(cx: &mut gpui::TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, _| HintOwner {
			mounted: true, clicks: 0,
			text: "API equivalent: $0.123 USD\nObserved responses: 12\nPrices checked: 2026-11-14\nThis estimate includes text tokens. Some recorded usage could not be priced.",
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let trigger = visual.debug_bounds("hint-trigger").expect("trigger");
		visual.simulate_mouse_move(trigger.center(), gpui::MouseButton::Left, Default::default());
		settle(visual, Duration::from_millis(500));
		let hint = visual.debug_bounds("hover-hint-surface").expect("details");
		assert!(hint.size.width <= gpui::px(360.));
		assert!(hint.size.height > gpui::px(60.));
	}

	#[gpui::test]
	fn a_wrapped_path_fits_its_text_without_an_empty_trailing_column(
		cx: &mut gpui::TestAppContext,
	) {
		let (_, visual) = cx.add_window_view(|_, _| HintOwner {
			mounted: true,
			clicks: 0,
			text: "/Users/x/.codex/diagnostics/decodex-history-english-qa-20261014/English-QA",
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let trigger = visual.debug_bounds("hint-trigger").expect("trigger");
		visual.simulate_mouse_move(trigger.center(), gpui::MouseButton::Left, Default::default());
		settle(visual, Duration::from_millis(500));
		let hint = visual.debug_bounds("hover-hint-surface").expect("path hint");
		assert!(
			hint.size.width < gpui::px(320.),
			"a two-line path must not reserve the maximum width: {hint:?}"
		);
		assert!(hint.size.height <= gpui::px(50.), "fitting must not add a third line: {hint:?}");
	}
}
