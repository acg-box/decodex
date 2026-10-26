//! Feedback is placed by scope, not by severity.
//!
//! - A field or operation error stays beside the affected control.
//! - Conversation errors and recovery actions share one band above the composer. A send restriction
//!   disables the editor; neither errors nor quotes occupy its toolbar. Archive status and its
//!   restore action occupy the disabled composer without an additional notice above it.
//! - A message receipt stays with its message; it never becomes a global notification.
//! - App-wide connection, account and background notices belong to the notification center.
//! - Read-only notices pair a short state with one plain sentence about its effect or next action.
//!   Keep the explanation beside the state when it fits, and let it wrap before the trailing
//!   action.
//! - Centered empty states are reserved for a surface with no usable content. Modal alerts are
//!   reserved for decisions that must precede an explicit action.
//!
//! Text and actions are left aligned within the owning content column. Conversation feedback
//! uses the page surface without an enclosing card or shadow. Tone supplements an icon and
//! explicit wording; it does not determine placement, focus or automatic dismissal. A blocking
//! status remains until its cause is resolved, and never discards the user's saved draft.
//!
//! Use concise, complete wording that explains the state, effect and required action.
//! Explain unfamiliar controls and scope beside the control. Remove repeated labels and
//! boilerplate, while retaining consequences, costs, defaults and recovery steps.
use std::{
	ops::Deref,
	time::{Duration, Instant},
};

use gpui::{
	self, AnyElement, App, Div, ElementId, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, RenderOnce, Role, SharedString, Stateful, StatefulInteractiveElement as _,
	Styled as _, Window, prelude::FluentBuilder as _,
};

use crate::{
	ui_loading,
	ui_motion::{self, Tween},
	ui_theme::{
		self, CONVERSATION_INSET, CONVERSATION_WIDTH, ERROR, INFO, SUCCESS, TEXT, TEXT_MUTED,
		WARNING,
	},
};

const NOTICE_TEXT_SIZE: f32 = 12.;
const NOTICE_LINE_HEIGHT: f32 = 18.;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Tone {
	#[default]
	Neutral,
	Info,
	Success,
	Warning,
	Error,
}
impl Tone {
	pub(crate) fn color(self) -> u32 {
		match self {
			Self::Neutral => TEXT_MUTED,
			Self::Info => INFO,
			Self::Success => SUCCESS,
			Self::Warning => WARNING,
			Self::Error => ERROR,
		}
	}

	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Neutral => "Status",
			Self::Info => "Information",
			Self::Success => "Success",
			Self::Warning => "Attention",
			Self::Error => "Error",
		}
	}

	pub(crate) fn icon(self) -> impl IntoElement {
		let data: &'static [u8] = match self {
			Self::Neutral | Self::Info => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round"><circle cx="8" cy="8" r="6"/><path d="M8 7v4M8 4.7v.1"/></g></svg>"#,
			Self::Success => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="6"/><path d="m5 8 2 2 4-4"/></g></svg>"#,
			Self::Warning => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round"><circle cx="8" cy="8" r="6"/><path d="M8 4.7v4M8 11v.1"/></g></svg>"#,
			Self::Error => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="white" stroke-width="1.3" stroke-linecap="round"><circle cx="8" cy="8" r="6"/><path d="m6 6 4 4M10 6l-4 4"/></g></svg>"#,
		};

		gpui::svg()
			.data(data)
			.size(gpui::px(14.))
			.flex_none()
			.text_color(ui_theme::rgb(self.color()))
	}

	pub(crate) fn row(self, id: impl Into<ElementId>, message: &str) -> Stateful<Div> {
		self.row_with_icon(id, message, self.icon())
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
			.text_color(ui_theme::rgb(self.color()))
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
		Self { message: message.into(), tone: Tone::Info, busy: true }
	}

	pub(crate) fn info(message: impl Into<String>) -> Self {
		Self { message: message.into(), tone: Tone::Info, ..Self::default() }
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

	pub(crate) fn icon(&self) -> AnyElement {
		if self.busy {
			ui_loading::loading("").into_any_element()
		} else {
			self.tone.icon().into_any_element()
		}
	}

	pub(crate) fn clear(&mut self) {
		*self = Self::default();
	}
}

impl Deref for Feedback {
	type Target = str;

	fn deref(&self) -> &str {
		&self.message
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
impl Notice {
	pub(crate) fn action(mut self, action: Option<AnyElement>) -> Self {
		self.action = action;
		self
	}
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
		let (feedback, opacity, height, moving, current) = state.update(cx, |s, _| {
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
				s.presence.moving(now, delay) || s.height.moving(now),
				s.presence.displayed == target,
			)
		});

		if moving || (feedback.as_ref().is_some_and(|f| f.busy) && !reduced) {
			ui_motion::request_frame(window, cx);
		}

		gpui::div()
			.id("conversation-feedback-slot")
			.debug_selector(|| "conversation-feedback-slot".into())
			.w_full()
			.flex_none()
			.h(gpui::px(height))
			.overflow_hidden()
			.when_some(feedback, |slot, feedback| {
				let row = status_row(
					"conversation-feedback-status",
					feedback.tone,
					"",
					&feedback.message,
					feedback.icon(),
				)
				.debug_selector(|| "conversation-activity-status".into())
				.children(self.action.filter(|_| current));

				slot.child(
					gpui::div()
						.w_full()
						.flex_none()
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
						.child(notice_rail(row).opacity(opacity)),
				)
			})
	}
}

struct NoticeState {
	presence: Presence<Feedback>,
	measured: f32,
	height: Tween,
}

/// Retain the old notice during its exit; changing route must never retain another chat's notice.
struct Presence<T> {
	scope: SharedString,
	displayed: Option<T>,
	target: Option<T>,
	changed: Instant,
	fade: Tween,
}
impl<T> Presence<T>
where
	T: Clone + Eq,
{
	fn new(scope: SharedString) -> Self {
		let now = Instant::now();

		Self { scope, displayed: None, target: None, changed: now, fade: Tween::new(0.) }
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
			self.displayed = next.cloned();
			self.fade = Tween::new(if next.is_some() { 1. } else { 0. });

			return;
		}
		if self.displayed.as_ref() != next {
			self.fade.target(0., now);

			if self.fade.sample(now) == 0. {
				self.displayed = next.cloned();
			}
		}
		if self.displayed.as_ref() == next && next.is_some() {
			self.fade.target(1., now);
		}
	}
}

/// A persistent state uses the same icon, title, explanation and trailing-action layout.
/// The owner supplies the surface so a composer never acquires a second enclosing border.
pub(crate) fn status_row(
	id: impl Into<ElementId>,
	tone: Tone,
	title: &str,
	detail: &str,
	icon: impl IntoElement,
) -> Stateful<Div> {
	state_row(id, tone, title, detail, icon, false)
}

/// A known read-only restriction keeps its explanation beside the title when space allows.
pub(crate) fn read_only_row(
	id: impl Into<ElementId>,
	title: &str,
	detail: &str,
	icon: impl IntoElement,
) -> Stateful<Div> {
	state_row(id, Tone::Neutral, title, detail, icon, true)
}

/// Local feedback stays in its owning form or setting, without a second card or shadow.
pub(crate) fn inline_feedback(
	id: impl Into<SharedString>,
	title: &str,
	feedback: &Feedback,
) -> Option<AnyElement> {
	if feedback.is_empty() {
		return None;
	}

	let id = id.into();
	let selector = id.to_string();

	Some(
		status_row(id, feedback.tone, title, &feedback.message, feedback.tone.icon())
			.debug_selector(move || selector.clone())
			.min_h(gpui::px(0.))
			.items_start()
			.py(gpui::px(6.))
			.into_any_element(),
	)
}

/// A compact execution state, shared by live history, saved history and Graph.
/// Keep the state name available to assistive technology without repeating it visually.
pub(crate) fn execution_icon(id: impl Into<SharedString>, status: &str) -> Stateful<Div> {
	let (tone, label, running) = match status {
		"running" | "inProgress" | "Running" => (Tone::Info, "Running", true),
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
				.text_color(ui_theme::rgb(TEXT_MUTED))
				.into_any_element()
		} else if running {
			ui_loading::loading("").into_any_element()
		} else {
			tone.icon().into_any_element()
		})
}

pub(crate) fn notice(
	id: impl Into<ElementId>,
	scope: impl Into<SharedString>,
	feedback: Option<Feedback>,
) -> Notice {
	Notice { id: id.into(), scope: scope.into(), feedback, action: None }
}

/// Lock immediately, retain the read-only draft through exit, then restore its editor.
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

fn state_row(
	id: impl Into<ElementId>,
	tone: Tone,
	title: &str,
	detail: &str,
	icon: impl IntoElement,
	inline: bool,
) -> Stateful<Div> {
	gpui::div()
		.id(id)
		.role(Role::Status)
		.aria_label(match (title.is_empty(), detail.is_empty()) {
			(true, _) => detail.to_owned(),
			(_, true) => title.to_owned(),
			_ => format!("{title}. {detail}"),
		})
		.w_full()
		.min_w_0()
		.min_h(gpui::px(if inline { 32. } else { 44. }))
		.flex()
		.items_center()
		.gap(gpui::px(10.))
		.text_size(gpui::px(NOTICE_TEXT_SIZE))
		.line_height(gpui::px(NOTICE_LINE_HEIGHT))
		.child(
			gpui::div()
				.h(gpui::px(NOTICE_LINE_HEIGHT))
				.flex_none()
				.flex()
				.items_center()
				.text_color(ui_theme::rgb(tone.color()))
				.child(icon),
		)
		.child(
			gpui::div()
				.flex_1()
				.min_w_0()
				.flex()
				.when(inline, |text| {
					text.flex_wrap().items_center().gap_x(gpui::px(8.)).gap_y(gpui::px(2.))
				})
				.when(!inline, |text| text.flex_col().gap(gpui::px(2.)))
				.when(!title.is_empty(), |text| {
					text.child(
						gpui::div()
							.font_weight(FontWeight::MEDIUM)
							.text_color(ui_theme::rgb(TEXT))
							.child(title.to_owned()),
					)
				})
				.when(!detail.is_empty(), |d| {
					d.child(
						gpui::div().text_color(ui_theme::rgb(TEXT_MUTED)).child(detail.to_owned()),
					)
				}),
		)
}

pub(crate) fn notice_rail(content: impl IntoElement) -> Div {
	gpui::div()
		.w_full()
		.flex()
		.justify_center()
		.px(gpui::px(CONVERSATION_INSET))
		.child(gpui::div().w_full().min_w_0().max_w(gpui::px(CONVERSATION_WIDTH)).child(content))
}

#[cfg(test)]
mod tests {
	use std::thread;

	use gpui::{ParentElement as _, Render, Styled as _};

	use crate::ui_feedback::{
		self, Duration, Feedback, Instant, IntoElement, Presence, Window, gpui,
	};

	const SETTLE: Duration = Duration::from_millis(220);

	struct NoticeView(Option<Feedback>);
	impl Render for NoticeView {
		fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			gpui::div().w(gpui::px(500.)).flex().flex_col().child(ui_feedback::notice(
				"notice",
				"chat",
				self.0.clone(),
			))
		}
	}

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
	#[gpui::test]
	fn the_slot_measures_content_and_animates_height_through_exit(cx: &mut gpui::TestAppContext) {
		let (view, visual) = cx.add_window_view(|_, _| NoticeView(None));

		visual.update(crate::ui_frame::draw_current_frame);

		view.update(visual, |s, cx| {
			s.0 = Some(Feedback::warning("Draft kept"));

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(SETTLE);

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(SETTLE);

		visual.update(crate::ui_frame::draw_frame);

		let open = visual.debug_bounds("conversation-feedback-slot").unwrap().size.height;

		assert!(f32::from(open) > 24., "The notice must acquire its measured height");

		view.update(visual, |s, cx| {
			s.0 = None;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("conversation-activity-status").is_some());

		thread::sleep(Duration::from_millis(50));

		visual.update(crate::ui_frame::draw_frame);

		let closing = visual.debug_bounds("conversation-feedback-slot").unwrap().size.height;

		assert!(f32::from(closing) > 0. && closing < open);

		thread::sleep(SETTLE);

		visual.update(crate::ui_frame::draw_frame);

		assert_eq!(
			f32::from(visual.debug_bounds("conversation-feedback-slot").unwrap().size.height),
			0.
		);
		assert!(visual.debug_bounds("conversation-activity-status").is_none());
	}
}
