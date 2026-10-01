//! Compact feedback for a first read. Refreshes with retained content stay quiet.
use std::{f32::consts::TAU, time::Instant};

use gpui::{
	self, App, IntoElement, PathBuilder, RenderOnce, Role, SharedString, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{ui_motion, ui_theme::TEXT_MUTED};

#[derive(IntoElement)]
pub(crate) struct Loading {
	label: &'static str,
}
impl RenderOnce for Loading {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let clock = window.use_keyed_state(
			SharedString::from(format!("loading-{}", self.label)),
			cx,
			|_, _| Instant::now(),
		);
		let elapsed = clock.read(cx).elapsed().as_secs_f32();
		let reduced = ui_motion::reduced();
		let phase = if reduced { 0. } else { (elapsed - 0.15).max(0.) * TAU };

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
			.child(
				gpui::canvas(
					|_, _, _| (),
					move |bounds, _, window, _| {
						let mut path = PathBuilder::stroke(gpui::px(1.25));

						for i in 0..=24 {
							let angle = phase + i as f32 / 24. * TAU * 0.72;
							let p = bounds.center()
								+ gpui::point(
									gpui::px(angle.cos() * 4.5),
									gpui::px(angle.sin() * 4.5),
								);

							if i == 0 {
								path.move_to(p);
							} else {
								path.line_to(p);
							}
						}

						if let Ok(path) = path.build() {
							window.paint_path(path, gpui::rgb(TEXT_MUTED));
						}
					},
				)
				.size(gpui::px(12.))
				.flex_none(),
			)
			.when(!self.label.is_empty(), |row| row.child(self.label))
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
	Loading { label }
}

pub(crate) fn conversation(label: &'static str) -> ConversationLoading {
	ConversationLoading { label }
}
