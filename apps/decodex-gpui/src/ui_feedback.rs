//! Operation feedback carries its meaning from the source to one compact presentation.
use std::{
	ops::Deref,
	time::{Duration, Instant},
};

use gpui::{
	self, AnyElement, App, Div, ElementId, InteractiveElement as _, IntoElement,
	ParentElement as _, RenderOnce, Role, SharedString, Stateful, StatefulInteractiveElement as _,
	Styled as _, Window, prelude::FluentBuilder as _,
};

use crate::{
	ui_motion::{self, Tween},
	ui_theme::{AMBER, CONVERSATION_INSET, ERROR, GREEN, METADATA_GAP, TEXT_MUTED},
};

const NOTICE_PADDING: f32 = 12.;
const NOTICE_FRAME: f32 = 2. * (NOTICE_PADDING + 1.);
const NOTICE_TEXT_SIZE: f32 = 12.;
const NOTICE_LINE_HEIGHT: f32 = 18.;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Tone {
	#[default]
	Neutral,
	Success,
	Warning,
	Error,
}

impl Tone {
	pub(crate) fn color(self) -> u32 {
		match self {
			Self::Neutral => TEXT_MUTED,
			Self::Success => GREEN,
			Self::Warning => AMBER,
			Self::Error => ERROR,
		}
	}

	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Neutral => "Status",
			Self::Success => "Success",
			Self::Warning => "Attention",
			Self::Error => "Error",
		}
	}

	pub(crate) fn icon(self) -> impl gpui::IntoElement {
		let data: &'static [u8] = match self {
			Self::Neutral => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round"><circle cx="8" cy="8" r="6"/><path d="M8 7v4M8 4.7v.1"/></g></svg>"#,
			Self::Success => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="6"/><path d="m5 8 2 2 4-4"/></g></svg>"#,
			Self::Warning | Self::Error => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round"><circle cx="8" cy="8" r="6"/><path d="M8 4.7v4M8 11v.1"/></g></svg>"#,
		};
		gpui::svg()
			.data(data)
			.size(gpui::px(14.))
			.flex_none()
			.text_color(crate::ui_theme::rgb(self.color()))
	}

	pub(crate) fn row(self, id: impl Into<ElementId>, message: &str) -> Stateful<Div> {
		self.row_with_icon(id, message, self.icon())
	}

	fn surface(self) -> Div {
		gpui::div()
			.rounded(gpui::px(12.))
			.border_1()
			.border_color(crate::ui_theme::rgba((self.color() << 8) | 0x26))
			.bg(crate::ui_theme::rgba((self.color() << 8) | 0x07))
			.px(gpui::px(NOTICE_PADDING))
			.py(gpui::px(6.))
			.text_size(gpui::px(NOTICE_TEXT_SIZE))
			.line_height(gpui::px(NOTICE_LINE_HEIGHT))
	}

	pub(crate) fn banner(self, content: impl IntoElement) -> Div {
		notice_rail(self.surface().max_w_full().child(content)).py(gpui::px(METADATA_GAP))
	}

	pub(crate) fn disclosure(
		self,
		id: impl Into<SharedString>,
		expanded: bool,
		header: impl IntoElement,
		detail: impl FnOnce(&mut App) -> AnyElement + 'static,
	) -> ExpandableNotice {
		ExpandableNotice {
			id: id.into(),
			tone: self,
			expanded,
			header: header.into_any_element(),
			detail: Box::new(detail),
		}
	}

	pub(crate) fn row_with_icon(
		self,
		id: impl Into<ElementId>,
		message: &str,
		icon: impl IntoElement,
	) -> Stateful<Div> {
		gpui::div()
			.id(id)
			.role(Role::Status)
			.aria_label(format!("{}: {message}", self.label()))
			.flex()
			.items_start()
			.gap(gpui::px(7.))
			.text_size(gpui::px(NOTICE_TEXT_SIZE))
			.line_height(gpui::px(NOTICE_LINE_HEIGHT))
			.text_color(crate::ui_theme::rgb(self.color()))
			.child(
				gpui::div()
					.h(gpui::px(NOTICE_LINE_HEIGHT))
					.flex_none()
					.flex()
					.items_center()
					.child(icon),
			)
			.child(gpui::div().min_w_0().child(message.to_owned()))
	}

	pub(crate) fn composer_row(
		self,
		id: impl Into<ElementId>,
		message: &str,
		icon: impl IntoElement,
	) -> Stateful<Div> {
		self.row_with_icon(id, message, icon)
			.debug_selector(|| "conversation-unavailable".into())
			.w_full()
			.min_w_0()
			.min_h(gpui::px(40.))
			.items_center()
			.justify_center()
	}
}

fn notice_rail(content: impl IntoElement) -> Div {
	gpui::div().w_full().flex().justify_center().px(gpui::px(CONVERSATION_INSET)).child(content)
}

#[derive(IntoElement)]
/// Expand one centered surface without reflowing its text during the transition.
pub(crate) struct ExpandableNotice {
	id: SharedString,
	tone: Tone,
	expanded: bool,
	header: AnyElement,
	detail: Box<dyn FnOnce(&mut App) -> AnyElement>,
}

#[derive(Default)]
struct NoticeGeometry {
	available: f32,
	header: f32,
	detail: f32,
}

impl RenderOnce for ExpandableNotice {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let geometry = window.use_keyed_state(
			SharedString::from(format!("notice-geometry-{}", self.id)),
			cx,
			|_, _| NoticeGeometry::default(),
		);
		let progress = ui_motion::value(
			SharedString::from(format!("notice-disclosure-{}", self.id)),
			if self.expanded { 1. } else { 0. },
			window,
			cx,
		);
		let measured = geometry.read(cx);
		let available = measured.available;
		let compact = (measured.header + NOTICE_FRAME).min(available);
		let width = compact + (available - compact) * progress;
		let height = measured.detail * progress;
		let header_geometry = geometry.clone();
		let detail_geometry = geometry.clone();
		let card = self
			.tone
			.surface()
			.max_w_full()
			.min_w_0()
			.overflow_hidden()
			.debug_selector(|| "expandable-notice-card".into())
			.when(available > 0., |d| d.w(gpui::px(width)))
			.child(
				gpui::div()
					.w_full()
					.flex()
					.justify_center()
					.on_children_prepainted(move |bounds, _, cx| {
						if let Some(bounds) = bounds.first() {
							let width = f32::from(bounds.size.width);
							header_geometry.update(cx, |s, cx| {
								if (s.header - width).abs() > 0.5 {
									s.header = width;
									cx.notify();
								}
							});
						}
					})
					.child(
						gpui::div()
							.flex_none()
							.h(gpui::px(NOTICE_LINE_HEIGHT))
							.flex()
							.items_center()
							.child(self.header),
					),
			)
			.child(
				gpui::div()
					.w_full()
					.h(gpui::px(height))
					.overflow_hidden()
					.flex()
					.justify_center()
					.opacity(((progress - 0.25) / 0.75).clamp(0., 1.))
					.when(self.expanded || progress > 0., |d| {
						d.child(
							gpui::div()
								.flex_none()
								// Measure at the final width; only the outer surface changes size.
								.when(available > 0., |d| {
									d.w(gpui::px((available - NOTICE_FRAME).max(0.)))
								})
								.debug_selector(|| "expandable-notice-detail".into())
								.on_children_prepainted(move |bounds, _, cx| {
									if let Some(bounds) = bounds.first() {
										let height = f32::from(bounds.size.height);
										detail_geometry.update(cx, |s, cx| {
											if (s.detail - height).abs() > 0.5 {
												s.detail = height;
												cx.notify();
											}
										});
									}
								})
								.child((self.detail)(cx)),
						)
					}),
			);
		notice_rail(gpui::div().w_full().flex().justify_center().child(card))
			.on_children_prepainted(move |bounds, _, cx| {
				if let Some(bounds) = bounds.first() {
					let width = f32::from(bounds.size.width);
					geometry.update(cx, |s, cx| {
						if (s.available - width).abs() > 0.5 {
							s.available = width;
							cx.notify();
						}
					});
				}
			})
	}
}

/// A compact execution state, shared by live history, saved history and Graph.
/// Keep the state name available to assistive technology without repeating it visually.
pub(crate) fn execution_icon(id: impl Into<SharedString>, status: &str) -> Stateful<Div> {
	let (tone, label, running) = match status {
		"running" | "inProgress" | "Running" => (Tone::Neutral, "Running", true),
		"completed" => (Tone::Success, "Completed", false),
		"failed" => (Tone::Error, "Failed", false),
		"declined" => (Tone::Warning, "Declined", false),
		"interrupted" => (Tone::Neutral, "Stopped", false),
		"exited" => (Tone::Neutral, "Exited", false),
		"waiting" => (Tone::Neutral, "Waiting", false),
		"recorded" => (Tone::Neutral, "Recorded", false),
		_ => (Tone::Neutral, "Unconfirmed", false),
	};
	gpui::div()
		.id(id.into())
		.role(Role::Status)
		.aria_label(label)
		.w(gpui::px(16.))
		.h(gpui::px(18.))
		.flex_none()
		.flex()
		.items_center()
		.justify_center()
		.child(if matches!(status, "waiting" | "recorded") {
			gpui::svg()
				.data(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="6"/><path d="M8 4.5V8l2.5 1.5"/></g></svg>"#)
				.size(gpui::px(14.))
				.text_color(crate::ui_theme::rgb(TEXT_MUTED))
				.into_any_element()
		} else if running {
			crate::ui_loading::loading("").into_any_element()
		} else {
			tone.icon().into_any_element()
		})
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Feedback {
	pub message: String,
	pub tone: Tone,
	busy: bool,
}

impl Feedback {
	pub(crate) fn neutral(message: impl Into<String>) -> Self {
		Self { message: message.into(), ..Self::default() }
	}

	pub(crate) fn loading(message: impl Into<String>) -> Self {
		Self { message: message.into(), busy: true, ..Self::default() }
	}

	pub(crate) fn success(message: impl Into<String>) -> Self {
		Self { message: message.into(), tone: Tone::Success, ..Self::default() }
	}

	pub(crate) fn warning(message: impl Into<String>) -> Self {
		Self { message: message.into(), tone: Tone::Warning, ..Self::default() }
	}

	pub(crate) fn error(message: impl Into<String>) -> Self {
		Self { message: message.into(), tone: Tone::Error, ..Self::default() }
	}

	pub(crate) fn clear(&mut self) {
		*self = Self::default();
	}
}

/// One mounted slot owns entry, replacement and exit; empty feedback still renders the slot.
#[derive(IntoElement)]
pub(crate) struct Notice {
	id: ElementId,
	scope: SharedString,
	feedback: Option<Feedback>,
	action: Option<AnyElement>,
}

pub(crate) fn notice(
	id: impl Into<ElementId>,
	scope: impl Into<SharedString>,
	feedback: Option<Feedback>,
) -> Notice {
	Notice { id: id.into(), scope: scope.into(), feedback, action: None }
}

impl Notice {
	pub(crate) fn action(mut self, action: Option<AnyElement>) -> Self {
		self.action = action;
		self
	}
}

struct NoticeState {
	presence: Presence<Feedback>,
	measured: f32,
	height: Tween,
}

impl RenderOnce for Notice {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id, cx, |_, _| NoticeState {
			presence: Presence::new(self.scope.clone()),
			measured: 0.,
			height: Tween::new(0.),
		});
		let now = Instant::now();
		let reduced = ui_motion::reduced();
		let target = self.feedback.filter(|feedback| !feedback.is_empty());
		let delay = if target.as_ref().is_some_and(|f| f.busy) {
			Duration::from_millis(250)
		} else {
			Duration::ZERO
		};
		let (feedback, opacity, height, elapsed, moving, current) = state.update(cx, |s, _| {
			if s.presence.scope != self.scope {
				s.height = Tween::new(0.);
				s.measured = 0.;
			}
			s.presence.update(self.scope, target.clone(), delay, now, reduced);
			let open = s.presence.target.is_some() && !s.presence.waiting(now, delay);
			s.height.target(if open { s.measured } else { 0. }, now);
			(
				s.presence.displayed.clone(),
				s.presence.fade.sample(now),
				s.height.sample(now),
				now.duration_since(s.presence.shown_at).as_secs_f32(),
				s.presence.moving(now, delay) || s.height.moving(now),
				s.presence.displayed == target,
			)
		});
		if moving || (feedback.as_ref().is_some_and(|f| f.busy) && !reduced) {
			ui_motion::request_frame(window, cx);
		}
		gpui::div().id("conversation-feedback-slot")
			.debug_selector(|| "conversation-feedback-slot".into())
			.w_full().flex_none().h(gpui::px(height)).overflow_hidden().when_some(
			feedback,
			|slot, feedback| {
				let tone = feedback.tone;
				let icon = if feedback.busy {
					gpui::svg()
						.data(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 2a6 6 0 1 1-6 6" fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round"/></svg>"#)
						.size(gpui::px(14.)).flex_none().text_color(crate::ui_theme::rgb(tone.color()))
						.with_transformation(gpui::Transformation::rotate(gpui::radians(
							if reduced { 0. } else { elapsed * std::f32::consts::TAU / 1.2 },
						)))
						.into_any_element()
				} else {
					tone.icon().into_any_element()
				};
				let row = tone.row_with_icon("conversation-feedback-status", &feedback.message, icon)
					.debug_selector(|| "conversation-activity-status".into())
					.justify_center()
					.children(self.action.filter(|_| current));
				slot.child(
					gpui::div().w_full().flex_none()
						.on_children_prepainted(move |bounds, _, cx| {
							if let Some(bounds) = bounds.first() {
								let height = f32::from(bounds.size.height);
								state.update(cx, |s, cx| {
									if (s.measured - height).abs() > 0.5 {
										s.measured = height;
										cx.notify();
									}
								});
							}
						})
						.child(tone.banner(row).opacity(opacity)),
				)
			},
		)
	}
}

/// Retain the old notice during its exit; changing route must never retain another chat's notice.
struct Presence<T> {
	scope: SharedString,
	displayed: Option<T>,
	target: Option<T>,
	changed: Instant,
	shown_at: Instant,
	fade: Tween,
}

impl<T: Clone + Eq> Presence<T> {
	fn new(scope: SharedString) -> Self {
		let now = Instant::now();
		Self {
			scope,
			displayed: None,
			target: None,
			changed: now,
			shown_at: now,
			fade: Tween::new(0.),
		}
	}

	fn waiting(&self, now: Instant, delay: Duration) -> bool {
		self.target.is_some() && now.duration_since(self.changed) < delay
	}

	fn moving(&self, now: Instant, delay: Duration) -> bool {
		self.fade.moving(now) || self.waiting(now, delay)
	}

	fn update(
		&mut self,
		scope: SharedString,
		target: Option<T>,
		delay: Duration,
		now: Instant,
		reduced: bool,
	) {
		if self.scope != scope {
			*self = Self::new(scope);
		}
		if self.target != target {
			self.target = target;
			self.changed = now;
		}
		let next = self.target.as_ref().filter(|_| !self.waiting(now, delay));
		if reduced {
			if self.displayed.as_ref() != next {
				self.shown_at = now;
			}
			self.displayed = next.cloned();
			self.fade = Tween::new(if next.is_some() { 1. } else { 0. });
			return;
		}
		if self.displayed.as_ref() != next {
			self.fade.target(0., now);
			if self.fade.sample(now) == 0. {
				self.displayed = next.cloned();
				self.shown_at = now;
			}
		}
		if self.displayed.as_ref() == next && next.is_some() {
			self.fade.target(1., now);
		}
	}
}

/// Lock immediately, retain its message through the exit, then restore the same draft editor.
pub(crate) fn composer_block(
	scope: impl Into<SharedString>,
	reason: Option<&'static str>,
	window: &mut Window,
	cx: &mut App,
) -> (Option<&'static str>, f32) {
	let scope = scope.into();
	let state =
		window.use_keyed_state("composer-block-presence", cx, |_, _| Presence::new(scope.clone()));
	let now = Instant::now();
	let (displayed, opacity, moving) = state.update(cx, |s, _| {
		s.update(scope, reason, Duration::ZERO, now, ui_motion::reduced());
		(s.displayed, s.fade.sample(now), s.moving(now, Duration::ZERO))
	});
	if moving {
		ui_motion::request_frame(window, cx);
	}
	(displayed, opacity)
}

impl Deref for Feedback {
	type Target = str;

	fn deref(&self) -> &str {
		&self.message
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::Render;
	const SETTLE: Duration = Duration::from_millis(220);

	#[test]
	fn brief_loading_never_enters_and_does_not_leak_into_the_next_operation() {
		let now = Instant::now();
		let delay = Duration::from_millis(250);
		let mut state = Presence::new("chat".into());
		state.update("chat".into(), Some("loading"), delay, now, false);
		assert!(state.displayed.is_none());
		state.update("chat".into(), None, delay, now + Duration::from_millis(180), false);
		assert!(state.displayed.is_none());
		assert!(!state.moving(now + SETTLE, delay));
		state.update("chat".into(), Some("loading"), delay, now + SETTLE, false);
		assert!(state.displayed.is_none());
		state.update("chat".into(), Some("loading"), delay, now + SETTLE + delay, false);
		assert_eq!(state.displayed, Some("loading"));
	}

	#[test]
	fn exit_keeps_the_message_and_can_reverse_without_an_opacity_jump() {
		let now = Instant::now();
		let mut state = Presence::new("chat".into());
		state.update("chat".into(), Some("warning"), Duration::ZERO, now, false);
		let settled = now + SETTLE;
		state.update("chat".into(), None, Duration::ZERO, settled, false);
		let interrupted = settled + Duration::from_millis(70);
		let fading = state.fade.sample(interrupted);
		assert_eq!(state.displayed, Some("warning"));
		assert!(fading > 0. && fading < 1.);
		state.update("chat".into(), Some("warning"), Duration::ZERO, interrupted, false);
		assert_eq!(state.fade.sample(interrupted), fading);
		assert_eq!(state.fade.sample(interrupted + SETTLE), 1.);
		state.update("chat".into(), None, Duration::ZERO, interrupted + SETTLE, false);
		state.update("chat".into(), None, Duration::ZERO, interrupted + SETTLE * 2, false);
		assert!(state.displayed.is_none());
	}

	#[test]
	fn replacement_uses_the_latest_result_and_drops_superseded_notices() {
		let now = Instant::now();
		let mut state = Presence::new("chat".into());
		state.update("chat".into(), Some("checking"), Duration::ZERO, now, false);
		state.update("chat".into(), Some("unavailable"), Duration::ZERO, now + SETTLE, false);
		assert_eq!(state.displayed, Some("checking"));
		assert_eq!(state.shown_at, now, "The outgoing indicator must keep its animation phase");
		state.update("chat".into(), Some("ready"), Duration::ZERO, now + SETTLE * 2, false);
		assert_eq!(state.displayed, Some("ready"));
	}

	#[test]
	fn changing_chat_cannot_fade_out_the_previous_chats_notice() {
		let now = Instant::now();
		let mut state = Presence::new("first".into());
		state.update("first".into(), Some("private warning"), Duration::ZERO, now, false);
		state.update("second".into(), None, Duration::ZERO, now + SETTLE, false);
		assert!(state.displayed.is_none());
		assert_eq!(state.fade.sample(now + SETTLE), 0.);
	}

	#[test]
	fn reduced_motion_settles_without_lingering_or_continuous_frames() {
		let now = Instant::now();
		let mut state = Presence::new("chat".into());
		state.update("chat".into(), Some("warning"), Duration::ZERO, now, false);
		state.update("chat".into(), None, Duration::ZERO, now + SETTLE, true);
		assert!(state.displayed.is_none());
		assert!(!state.moving(now + SETTLE, Duration::ZERO));
	}

	struct NoticeView(Option<Feedback>);
	impl Render for NoticeView {
		fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			gpui::div().w(gpui::px(500.)).flex().flex_col().child(notice(
				"notice",
				"chat",
				self.0.clone(),
			))
		}
	}

	struct DisclosureView(bool);
	impl Render for DisclosureView {
		fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			gpui::div().w(gpui::px(500.)).child(Tone::Warning.disclosure(
				"saved-input",
				self.0,
				gpui::div().flex_none().child(Tone::Warning.row("header", "Message not sent")),
				|_| gpui::div().child("A saved message with enough text to wrap across several lines at the final reading width. ".repeat(8)).into_any_element(),
			))
		}
	}

	#[gpui::test]
	fn disclosure_moves_as_one_surface_without_reflowing_its_detail(cx: &mut gpui::TestAppContext) {
		let (view, visual) = cx.add_window_view(|_, _| DisclosureView(false));
		for _ in 0..4 {
			visual.update(crate::ui_frame::draw_frame);
		}
		let compact = visual.debug_bounds("expandable-notice-card").unwrap();
		assert_eq!(compact.size.height, gpui::px(32.));
		let first_frame_limit = (gpui::px(468.) - compact.size.width) * 0.25;
		view.update(visual, |s, cx| {
			s.0 = true;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		let first_opening = visual.debug_bounds("expandable-notice-card").unwrap();
		assert!(
			(first_opening.size.width - compact.size.width).abs() < first_frame_limit,
			"compact={compact:?}, first_opening={first_opening:?}"
		);
		std::thread::sleep(Duration::from_millis(70));
		visual.update(crate::ui_frame::draw_frame);
		let opening = visual.debug_bounds("expandable-notice-card").unwrap();
		let detail = visual.debug_bounds("expandable-notice-detail").unwrap();
		assert!(opening.size.width > compact.size.width && opening.size.width < gpui::px(468.));
		std::thread::sleep(SETTLE);
		visual.update(crate::ui_frame::draw_frame);
		let open = visual.debug_bounds("expandable-notice-card").unwrap();
		assert_eq!(open.size.width, gpui::px(468.));
		assert_eq!(
			detail.size.width,
			visual.debug_bounds("expandable-notice-detail").unwrap().size.width
		);
		assert_eq!(compact.center().x, open.center().x);
		view.update(visual, |s, cx| {
			s.0 = false;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(
			(visual.debug_bounds("expandable-notice-card").unwrap().size.width - open.size.width)
				.abs()
				< first_frame_limit
		);
		std::thread::sleep(Duration::from_millis(70));
		visual.update(crate::ui_frame::draw_frame);
		let closing = visual.debug_bounds("expandable-notice-card").unwrap();
		assert!(closing.size.width > compact.size.width && closing.size.width < open.size.width);
		view.update(visual, |s, cx| {
			s.0 = true;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(
			(visual.debug_bounds("expandable-notice-card").unwrap().size.width
				- closing.size.width)
				.abs()
				< first_frame_limit
		);
		std::thread::sleep(SETTLE);
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(
			visual.debug_bounds("expandable-notice-card").unwrap().size.width,
			open.size.width
		);
	}

	#[gpui::test]
	fn the_slot_measures_content_and_animates_height_through_exit(cx: &mut gpui::TestAppContext) {
		let (view, visual) = cx.add_window_view(|_, _| NoticeView(None));
		visual.update(|window, cx| window.draw(cx).clear(cx));
		view.update(visual, |s, cx| {
			s.0 = Some(Feedback::warning("Draft kept"));
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(SETTLE);
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(SETTLE);
		visual.update(crate::ui_frame::draw_frame);
		let open = visual.debug_bounds("conversation-feedback-slot").unwrap().size.height;
		assert!(f32::from(open) > 24., "The notice must acquire its measured height");
		view.update(visual, |s, cx| {
			s.0 = None;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("conversation-activity-status").is_some());
		std::thread::sleep(Duration::from_millis(50));
		visual.update(crate::ui_frame::draw_frame);
		let closing = visual.debug_bounds("conversation-feedback-slot").unwrap().size.height;
		assert!(f32::from(closing) > 0. && closing < open);
		std::thread::sleep(SETTLE);
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(
			f32::from(visual.debug_bounds("conversation-feedback-slot").unwrap().size.height),
			0.
		);
		assert!(visual.debug_bounds("conversation-activity-status").is_none());
	}
}
