//! Agent identities and current work, with stable text while native output grows.
use crate::{
	ui_motion,
	ui_theme::{TEXT, TEXT_MUTED},
};
use gpui::{App, IntoElement, RenderOnce, Window, prelude::*};
use std::time::Instant;

#[derive(IntoElement)]
pub(super) struct Header {
	pub key: String,
	pub identity: String,
	pub show_assignment: bool,
	pub task: String,
	pub preview: String,
	pub layout: super::card::Layout,
	pub color: u32,
}

struct PreviewSwap {
	task_source: String,
	task: String,
	source: String,
	current: String,
	previous: Option<String>,
	started: Instant,
}

impl Header {
	fn preview(&self, window: &mut Window, cx: &mut App) -> (String, String, Option<String>, f32) {
		let now = Instant::now();
		let state = window.use_keyed_state(
			gpui::SharedString::from(format!("relation-preview-{}", self.key)),
			cx,
			|_, _| PreviewSwap {
				task_source: self.task.clone(),
				task: super::super::activity::text(&self.task),
				source: self.preview.clone(),
				current: super::super::activity::text(&self.preview),
				previous: None,
				started: now,
			},
		);
		let (task, current, previous, progress) = state.update(cx, |state, _| {
			if state.task_source != self.task {
				state.task_source = self.task.clone();
				state.task = super::super::activity::text(&self.task);
			}
			if state.source != self.preview {
				let next = super::super::activity::text(&self.preview);
				let growing = self.preview.starts_with(&state.source);
				state.previous = (!ui_motion::reduced() && !growing && next != state.current)
					.then(|| state.current.clone());
				state.current = next;
				state.source = self.preview.clone();
				state.started = now;
			}
			let progress = if ui_motion::reduced() {
				1.
			} else {
				(state.started.elapsed().as_secs_f32() / 0.22).min(1.)
			};
			if progress >= 1. {
				state.previous = None;
			}
			let opacity = if state.previous.is_some() { progress } else { 1. };
			(state.task.clone(), state.current.clone(), state.previous.clone(), opacity)
		});
		if previous.is_some() {
			ui_motion::request_frame(window, cx);
		}
		let task = if task.is_empty() { "Assignment unavailable".into() } else { task };
		(task, current, previous, progress)
	}

	fn status_light(&self) -> gpui::Div {
		let selector = format!("relation-status-{}", self.key);
		gpui::div()
			.debug_selector(move || selector.clone())
			.size(gpui::px(5. * self.layout.chrome_scale))
			.flex_none()
			.rounded_full()
			.bg(gpui::rgb(self.color))
	}

	fn identity_row(&self) -> gpui::Div {
		let identity_selector = format!("relation-identity-{}", self.key);
		let task_selector = format!("relation-task-{}", self.key);
		gpui::div()
			.debug_selector(move || task_selector.clone())
			.flex()
			.items_start()
			.gap(gpui::px(7. * self.layout.text_scale))
			.h(gpui::px(if self.show_assignment { 13. } else { 26. } * self.layout.text_scale))
			.child(self.status_light().mt(gpui::px(4. * self.layout.text_scale)))
			.child(
				gpui::div()
					.debug_selector(move || identity_selector.clone())
					.flex_1()
					.min_w_0()
					.whitespace_normal()
					.line_clamp(if self.show_assignment { 1 } else { 2 })
					.text_ellipsis()
					.text_size(gpui::px(11. * self.layout.text_scale))
					.line_height(gpui::px(13. * self.layout.text_scale))
					.font_weight(gpui::FontWeight::MEDIUM)
					.text_color(gpui::rgb(TEXT))
					.child(self.identity.clone()),
			)
	}
}

impl RenderOnce for Header {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let selector = format!("relation-activity-{}", self.key);
		let (task, current, previous, progress) = self.preview(window, cx);
		let task_selector = format!("relation-assignment-{}", self.key);
		let preview_selector = format!("relation-preview-{}", self.key);
		let opacity = progress * progress * (3. - 2. * progress);
		let preview_line = |text: String| {
			gpui::div()
				.absolute()
				.inset_0()
				.whitespace_normal()
				.line_clamp(self.layout.preview_lines)
				.text_ellipsis()
				.text_size(gpui::px(10. * self.layout.text_scale))
				.line_height(gpui::px(11. * self.layout.text_scale))
				.text_color(gpui::rgb(TEXT_MUTED))
				.child(text)
		};
		gpui::div()
			.debug_selector(move || selector.clone())
			.w_full()
			.flex_none()
			.min_w_0()
			.flex()
			.flex_col()
			.gap(gpui::px(4. * self.layout.text_scale))
			.child(self.identity_row())
			.when(self.show_assignment && task != self.identity && !task.is_empty(), |view| {
				view.child(
					gpui::div()
						.debug_selector(move || task_selector.clone())
						.w_full()
						.h(gpui::px(12. * self.layout.text_scale * self.layout.task_lines as f32))
						.flex_none()
						.text_size(gpui::px(10.5 * self.layout.text_scale))
						.line_height(gpui::px(12. * self.layout.text_scale))
						.font_weight(gpui::FontWeight::NORMAL)
						.text_color(gpui::rgb(TEXT))
						.whitespace_normal()
						.line_clamp(self.layout.task_lines)
						.text_ellipsis()
						.overflow_hidden()
						.child(task),
				)
			})
			.when(!current.is_empty(), |view| {
				view.child(
					gpui::div()
						.debug_selector(move || preview_selector.clone())
						.relative()
						.w_full()
						.h(gpui::px(
							11. * self.layout.text_scale * self.layout.preview_lines as f32,
						))
						.flex_none()
						.overflow_hidden()
						.children(previous.map(|text| preview_line(text).opacity(1. - opacity)))
						.child(preview_line(current).opacity(opacity)),
				)
			})
	}
}
