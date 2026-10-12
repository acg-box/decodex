//! Compact feedback for a first read. Refreshes with retained content stay quiet.
use std::{f32::consts::TAU, time::Instant};

use gpui::{
	self, AnyElement, App, IntoElement, RenderOnce, Role, SharedString, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{ui_motion, ui_theme::TEXT_MUTED};

#[derive(IntoElement)]
pub(crate) struct Loading {
	label: &'static str,
	icon_only: bool,
}
impl RenderOnce for Loading {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let clock = window.use_keyed_state(
			SharedString::from(format!("loading-{}", self.label)),
			cx,
			|_, _| Instant::now(),
		);
		let elapsed = clock.read(cx).elapsed().as_secs_f32();
		let scale = if self.icon_only { 1.5 } else { 1. };
		let reduced = ui_motion::reduced();
		let phase = if reduced { 0. } else { elapsed * TAU / 1.4 };

		if !reduced {
			ui_motion::request_frame(window, cx);
		}

		gpui::div()
			.id(SharedString::from(format!("loading-row-{}", self.label)))
			.role(Role::Status)
			.aria_label(if self.label.is_empty() { "Loading" } else { self.label })
			.debug_selector(move || format!("loading-feedback-{}", self.label))
			.h(gpui::px(24.))
			.flex_none()
			.flex()
			.items_center()
			.gap(gpui::px(8.))
			.text_size(gpui::px(11.))
			.text_color(gpui::rgb(TEXT_MUTED))
			.opacity(if reduced { 1. } else { ((elapsed - 0.12) / 0.12).clamp(0., 1.) })
			.child(gpui::div().flex().items_center().gap(gpui::px(4. * scale)).children(
				(0..3).map(|i| {
					gpui::div()
						.size(gpui::px(4. * scale))
						.rounded_full()
						.bg(gpui::rgb(TEXT_MUTED))
						.opacity(if reduced {
							0.6
						} else {
							0.3 + 0.45 * ((phase - i as f32 * 0.7).sin() + 1.) / 2.
						})
				}),
			))
			.when(!self.icon_only && !self.label.is_empty(), |row| row.child(self.label))
	}
}

/// First-load placeholder that reserves the reading surface, not a toolbar row.
#[derive(IntoElement)]
pub(crate) struct ConversationLoading {
	label: &'static str,
}
impl RenderOnce for ConversationLoading {
	fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
		// The history shape is unknown until read. Do not invent bubbles or text lengths.
		gpui::div()
			.id(SharedString::from(format!("conversation-loading-surface-{}", self.label)))
			.role(Role::Status)
			.aria_label(self.label)
			.debug_selector(move || format!("loading-feedback-{}", self.label))
			.w_full()
			.min_h(gpui::px(260.))
			.flex()
			.items_center()
			.justify_center()
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap(gpui::px(8.))
					.text_size(gpui::px(12.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(loading(""))
					.child(self.label),
			)
	}
}

pub(crate) fn loading(label: &'static str) -> Loading {
	Loading { label, icon_only: false }
}

pub(crate) fn conversation(label: &'static str) -> ConversationLoading {
	ConversationLoading { label }
}

/// Quiet full-workspace feedback; the status remains available to assistive technology.
pub(crate) fn workspace() -> Loading {
	Loading { label: "Opening workspace", icon_only: true }
}

/// Fade each data surface once. Retained data and background refreshes do not replay it.
#[derive(IntoElement)]
pub(crate) struct Content {
	key: SharedString,
	ready: bool,
	child: AnyElement,
}
#[derive(Default)]
struct Arrival {
	started: Option<Instant>,
}
impl Arrival {
	fn progress(&mut self, ready: bool, now: Instant, reduced: bool) -> f32 {
		if !ready {
			return 1.;
		}
		let started = *self.started.get_or_insert(now);
		if reduced {
			return 1.;
		}
		let t = (now.saturating_duration_since(started).as_secs_f32() / 0.18).clamp(0., 1.);
		1. - (1. - t).powi(3)
	}
}
impl RenderOnce for Content {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(
			SharedString::from(format!("load-arrival-{}", self.key)),
			cx,
			|_, _| Arrival::default(),
		);
		let opacity =
			state.update(cx, |s, _| s.progress(self.ready, Instant::now(), ui_motion::reduced()));
		if opacity < 1. {
			ui_motion::request_frame(window, cx);
		}
		gpui::div().w_full().min_w_0().flex_none().opacity(opacity).child(self.child)
	}
}
pub(crate) fn content(
	key: impl Into<SharedString>,
	ready: bool,
	child: impl IntoElement,
) -> Content {
	Content { key: key.into(), ready, child: child.into_any_element() }
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn data_arrives_once_without_replaying_refresh_or_delaying_reduced_motion() {
		let now = Instant::now();
		let mut state = Arrival::default();
		assert_eq!(state.progress(false, now, false), 1.);
		assert_eq!(state.progress(true, now, false), 0.);
		let halfway = now + std::time::Duration::from_millis(90);
		assert!((0.5..1.).contains(&state.progress(true, halfway, false)));
		let done = now + std::time::Duration::from_millis(180);
		assert_eq!(state.progress(true, done, false), 1.);
		assert_eq!(state.progress(false, done, false), 1.);
		assert_eq!(state.progress(true, done, false), 1.);
		assert_eq!(Arrival::default().progress(true, now, true), 1.);
	}
}
