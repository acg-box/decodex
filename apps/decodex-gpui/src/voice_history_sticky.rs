//! Keep each voice disclosure reachable within its own transcript bounds.
use std::{
	cell::{Cell, RefCell},
	rc::Rc,
	time::Instant,
};

use crate::shell::agent_surface::AgentSurface;
use gpui::{
	AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
	InspectorElementId, IntoElement, LayoutId, ParentElement, Pixels, ScrollHandle, Styled,
	WeakEntity, Window, point, px, size,
};

pub(super) const HEADER_HEIGHT: f32 = 28.;
type Position = Rc<Cell<(Pixels, Pixels)>>;

pub(crate) struct ReadingSpace {
	height: RefCell<crate::ui_motion::Tween>,
	reveal: RefCell<Option<(crate::ui_motion::Tween, bool)>>,
}
impl Default for ReadingSpace {
	fn default() -> Self {
		Self { height: RefCell::new(crate::ui_motion::Tween::new(0.)), reveal: RefCell::default() }
	}
}
impl ReadingSpace {
	pub(crate) fn get(&self) -> f32 {
		self.height.borrow().sample(Instant::now())
	}

	pub(crate) fn set(&self, height: f32) {
		*self.height.borrow_mut() = crate::ui_motion::Tween::new(height);
		self.reveal.borrow_mut().take();
	}

	fn collapse(&self, height: f32) {
		self.set(height);
		self.height.borrow_mut().target(0., Instant::now());
	}

	fn reveal(&self, from: f32, to: f32) {
		self.set(0.);
		let mut motion = crate::ui_motion::Tween::new(from);
		motion.target(to.min(0.), Instant::now());
		*self.reveal.borrow_mut() = Some((motion, false));
	}

	pub(crate) fn advance(&self, scroll: &ScrollHandle, window: &mut Window, cx: &mut App) -> bool {
		let now = Instant::now();
		let mut moving = self.height.borrow().moving(now);
		let mut reveal = self.reveal.borrow_mut();
		if let Some((motion, finishing)) = reveal.as_mut() {
			scroll.set_offset(point(
				scroll.offset().x,
				px(motion.sample(now).clamp(-f32::from(scroll.max_offset().y).max(0.), 0.)),
			));
			if motion.moving(now) || !*finishing {
				moving = true;
				// Apply the final offset once more after the disclosure's final
				// height has reached layout, including a slow first frame.
				*finishing = !motion.moving(now);
			} else {
				*reveal = None;
			}
		}
		if moving {
			crate::ui_motion::request_frame(window, cx);
		}
		moving
	}
}

#[derive(Clone)]
pub(crate) struct Viewport {
	pub scroll: ScrollHandle,
	pub min_height: Rc<ReadingSpace>,
	pub owner: Option<WeakEntity<AgentSurface>>,
}
impl AgentSurface {
	pub(in crate::shell::agent_surface) fn animate_voice_reading_space(
		&mut self,
		scroll: &ScrollHandle,
		window: &mut Window,
		cx: &mut gpui::Context<Self>,
	) -> f32 {
		let space = self
			.timeline
			.voice_min_height
			.entry(self.selected.clone().unwrap_or_default())
			.or_default();
		if space.advance(scroll, window, cx) {
			cx.notify();
		}
		space.get()
	}

	pub(in crate::shell::agent_surface) fn voice_history_viewport(
		&self,
		cx: &gpui::Context<Self>,
	) -> Viewport {
		Viewport {
			scroll: self.timeline.scroll_for(self.selected.as_deref()),
			min_height: self
				.timeline
				.voice_min_height
				.get(self.selected.as_deref().unwrap_or_default())
				.cloned()
				.unwrap_or_default(),
			owner: Some(cx.entity().downgrade()),
		}
	}
}

pub(super) trait Toggle {
	fn toggle(&self, open: bool, position: &Position, cx: &mut App);
}
impl Toggle for Option<Viewport> {
	fn toggle(&self, open: bool, position: &Position, cx: &mut App) {
		let Some(viewport) = self else {
			return;
		};
		if let Some(owner) = &viewport.owner {
			let _ = owner.update(cx, |surface, cx| {
				surface.preserve_history_selection();
				surface.timeline.latest_follow_work = None;
				surface.timeline.navigation = None;
				surface.timeline.wheel_scroll = None;
				surface.timeline.older_scroll_anchor = None;
				surface.cancel_native_scroll_anchor();
				if let Some(work) = &surface.selected {
					surface.timeline.follow_paused.insert(work.clone());
				}
				surface.set_voice_follow(false);
				cx.notify();
			});
		}
		if open {
			let (group_top, header_top) = position.get();
			let offset = viewport.scroll.offset();
			let target = offset.y + header_top - group_top;
			// Reserve the first collapse frame, then release it with the disclosure.
			// The last record settles at the natural bottom without a permanent gap.
			viewport.min_height.collapse(f32::from(viewport.scroll.bounds().size.height - target));
			viewport.scroll.set_offset(point(offset.x, target));
		} else {
			let (group_top, _) = position.get();
			let from = viewport.scroll.offset().y;
			viewport
				.min_height
				.reveal(from.into(), (from + viewport.scroll.bounds().top() - group_top).into());
		}
	}
}

pub(super) struct Group {
	pub body: AnyElement,
	pub header: Option<AnyElement>,
	pub open: bool,
	pub position: Position,
}
impl IntoElement for Group {
	type Element = Self;

	fn into_element(self) -> Self {
		self
	}
}
impl Element for Group {
	type PrepaintState = bool;
	type RequestLayoutState = ();

	fn id(&self) -> Option<ElementId> {
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
		(self.body.request_layout(window, cx), ())
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
		let mask = window.content_mask();
		self.body.prepaint(window, cx);
		let top = if self.open {
			mask.bounds.top().max(bounds.top()).min(bounds.bottom() - px(HEADER_HEIGHT))
		} else {
			bounds.top()
		};
		self.position.set((bounds.top(), top));
		let sticky = top > bounds.top();
		let header = self.header.take().expect("voice header prepaints once");
		let mut header = if sticky {
			gpui::div().w_full().bg(crate::ui_theme::rgb(0x111113)).child(header).into_any_element()
		} else {
			header
		};
		header.layout_as_root(
			size(
				AvailableSpace::Definite(bounds.size.width),
				AvailableSpace::Definite(px(HEADER_HEIGHT)),
			),
			window,
			cx,
		);
		if sticky {
			window.defer_draw(header, point(bounds.left(), top), 0, Some(mask));
		} else {
			header.prepaint_at(point(bounds.left(), top), window, cx);
			self.header = Some(header);
		}
		sticky
	}

	fn paint(
		&mut self,
		_: Option<&GlobalElementId>,
		_: Option<&InspectorElementId>,
		_: Bounds<Pixels>,
		_: &mut (),
		sticky: &mut bool,
		window: &mut Window,
		cx: &mut App,
	) {
		self.body.paint(window, cx);
		if !*sticky && let Some(header) = self.header.as_mut() {
			header.paint(window, cx);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{super::VoiceBlock, *};
	use gpui::{Context, Render, TestAppContext, VisualTestContext, prelude::*};

	struct Fixture {
		min_height: Rc<ReadingSpace>,
		tail: f32,
		scroll: ScrollHandle,
	}
	impl Render for Fixture {
		fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			if self.min_height.advance(&self.scroll, window, cx) {
				cx.notify();
			}
			gpui::div().size_full().p_4().child(
				gpui::div()
					.id("test-voice-scroll")
					.size_full()
					.overflow_y_scroll()
					.track_scroll(&self.scroll)
					.child(
						gpui::div()
							.w_full()
							.flex()
							.flex_col()
							.min_h(px(self.min_height.get()))
							.gap_2()
							.children(["first", "second"].into_iter().map(|key| VoiceBlock {
								key: key.into(),
								title: "Voice conversation".into(),
								text: "user: Question\nassistant: A long answer.\n".repeat(12),
								viewport: Some(Viewport {
									scroll: self.scroll.clone(),
									min_height: self.min_height.clone(),
									owner: None,
								}),
							}))
							.child(gpui::div().h(px(self.tail))),
					),
			)
		}
	}
	fn draw(visual: &mut VisualTestContext) {
		visual.update(|window, cx| window.draw(cx).clear(cx));
	}
	fn settle(visual: &mut VisualTestContext) {
		draw(visual);
		std::thread::sleep(std::time::Duration::from_millis(250));
		draw(visual);
		std::thread::sleep(std::time::Duration::from_millis(250));
		draw(visual);
	}
	#[gpui::test]
	fn independent_groups_stick_within_their_bounds_and_collapse_at_the_visible_header(
		cx: &mut TestAppContext,
	) {
		let scroll = ScrollHandle::new();
		let (_, visual) = cx.add_window_view(|_, _| Fixture {
			scroll: scroll.clone(),
			min_height: Rc::default(),
			tail: 900.,
		});
		visual.simulate_resize(size(px(700.), px(480.)));
		draw(visual);
		let first = visual.debug_bounds("voice-history-block-first").unwrap();
		assert_eq!(first.size.height, px(HEADER_HEIGHT));
		visual.simulate_click(first.center(), Default::default());
		settle(visual);
		assert!(visual.debug_bounds("voice-history-transcript-first").is_some());
		scroll.set_offset(point(px(0.), px(-250.)));
		draw(visual);
		let pinned = visual.debug_bounds("voice-history-block-first").unwrap();
		assert_eq!(pinned.top(), scroll.bounds().top());
		// Locate and open the second group without closing the first.
		let second = visual.debug_bounds("voice-history-block-second").unwrap();
		scroll.set_offset(point(px(0.), scroll.offset().y + scroll.bounds().top() - second.top()));
		draw(visual);
		let second = visual.debug_bounds("voice-history-block-second").unwrap();
		visual.simulate_click(second.center(), Default::default());
		settle(visual);
		assert!(visual.debug_bounds("voice-history-transcript-first").is_some());
		assert!(visual.debug_bounds("voice-history-transcript-second").is_some());
		let first = visual.debug_bounds("voice-history-block-first").unwrap();
		assert!(
			first.bottom() <= scroll.bounds().top(),
			"the previous group must leave the viewport"
		);
		scroll.set_offset(point(px(0.), scroll.offset().y - px(250.)));
		draw(visual);
		let pinned = visual.debug_bounds("voice-history-block-second").unwrap();
		assert_eq!(pinned.top(), scroll.bounds().top());
		visual.simulate_click(pinned.center(), Default::default());
		settle(visual);
		let collapsed = visual.debug_bounds("voice-history-block-second").unwrap();
		assert!((collapsed.top() - pinned.top()).abs() < px(1.));
		assert!(visual.debug_bounds("voice-history-transcript-second").is_none());
		assert!(visual.debug_bounds("voice-history-transcript-first").is_some());
	}
	#[gpui::test]
	fn last_group_releases_reading_space_after_collapse(cx: &mut TestAppContext) {
		let scroll = ScrollHandle::new();
		let (_, visual) = cx.add_window_view(|_, _| Fixture {
			scroll: scroll.clone(),
			min_height: Rc::default(),
			tail: 0.,
		});
		visual.simulate_resize(size(px(700.), px(480.)));
		draw(visual);
		let header = visual.debug_bounds("voice-history-block-second").unwrap();
		visual.simulate_click(header.center(), Default::default());
		settle(visual);
		assert_eq!(
			visual.debug_bounds("voice-history-block-second").unwrap().top(),
			scroll.bounds().top(),
			"expanding a long call scrolls its header to the top"
		);
		scroll.set_offset(point(px(0.), px(-350.)));
		draw(visual);
		let pinned = visual.debug_bounds("voice-history-block-second").unwrap();
		assert_eq!(pinned.top(), scroll.bounds().top());
		let speech = visual.debug_bounds("voice-transcript-turn-10").unwrap();
		visual.simulate_click(pinned.center(), Default::default());
		draw(visual);
		let closing_speech = visual.debug_bounds("voice-transcript-turn-10").unwrap();
		assert!(
			(closing_speech.top() - speech.top()).abs() < px(1.),
			"the first collapsing frame must not flash the start of the transcript"
		);
		std::thread::sleep(std::time::Duration::from_millis(140));
		draw(visual);
		let fading_speech = visual.debug_bounds("voice-transcript-turn-10").unwrap();
		assert!(
			(fading_speech.top() - speech.top()).abs() < px(1.),
			"fading speech must not move while the last group's scroll range contracts"
		);
		settle(visual);
		let collapsed = visual.debug_bounds("voice-history-block-second").unwrap();
		assert!(collapsed.bottom() < scroll.bounds().bottom());
		assert_eq!(scroll.max_offset().y, px(0.), "collapsed history must not retain blank space");
		assert!(visual.debug_bounds("voice-history-transcript-second").is_none());
	}
}
