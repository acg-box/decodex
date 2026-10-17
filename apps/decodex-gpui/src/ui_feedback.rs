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
	ui_theme::{AMBER, CONVERSATION_INSET, GREEN, TEXT_MUTED},
};

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
			Self::Error => 0xf28b82,
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
		gpui::svg().data(data).size(gpui::px(14.)).flex_none().text_color(gpui::rgb(self.color()))
	}

	pub(crate) fn row(self, id: impl Into<ElementId>, message: &str) -> Stateful<Div> {
		self.row_with_icon(id, message, self.icon())
	}

	pub(crate) fn surface(self) -> Div {
		gpui::div()
			.rounded(gpui::px(12.))
			.border_1()
			.border_color(gpui::rgba((self.color() << 8) | 0x26))
			.bg(gpui::rgba((self.color() << 8) | 0x07))
	}

	pub(crate) fn banner(self, content: impl IntoElement) -> Div {
		gpui::div()
			.w_full()
			.flex()
			.justify_center()
			.px(gpui::px(CONVERSATION_INSET))
			.py(gpui::px(4.))
			.child(self.surface().max_w_full().px_3().py(gpui::px(6.)).child(content))
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
			.text_size(gpui::px(12.))
			.line_height(gpui::px(18.))
			.text_color(gpui::rgb(self.color()))
			.child(gpui::div().mt(gpui::px(2.)).flex_none().child(icon))
			.child(gpui::div().min_w_0().child(message.to_owned()))
	}
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
						.size(gpui::px(14.)).flex_none().text_color(gpui::rgb(tone.color()))
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
