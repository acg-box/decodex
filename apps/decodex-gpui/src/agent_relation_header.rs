//! Compact canvas labels with an inline status light and a quiet agent number.
use super::stream;
use crate::{ui_motion, ui_theme::TEXT};
use gpui::{App, IntoElement, RenderOnce, Window, prelude::*};

#[derive(IntoElement)]
pub(super) struct Header {
	pub key: String,
	pub identity: String,
	pub number: String,
	pub color: u32,
	pub selected: bool,
	pub task: String,
	pub activity: stream::Frame,
	pub layout: super::card::Layout,
}

impl RenderOnce for Header {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let layout = self.layout.on_display(window.scale_factor());
		let has_actions = self.activity.height > 0.;
		let actions = action_rows(&self.key, self.activity, layout, window, cx);
		let identity_selector = format!("relation-identity-{}", self.key);
		let number_selector = format!("relation-number-{}", self.key);
		let light_selector = format!("relation-status-{}", self.key);
		let task_selector = format!("relation-assignment-{}", self.key);
		let wrap_title = layout.full_detail()
			&& self.identity == self.task
			&& self.identity.chars().count() > 24;
		gpui::div()
			.w_full()
			.flex_none()
			.min_w_0()
			.flex()
			.flex_col()
			.gap(gpui::px(layout.spacing(2.)))
			.child(
				gpui::div()
					.w_full()
					.flex()
					.items_start()
					.when(layout.overview(), |row| row.justify_center().items_center())
					.gap(gpui::px(layout.spacing(5.)))
					.child(
						gpui::div()
							.debug_selector(move || light_selector.clone())
							.flex_none()
							.mt(gpui::px((layout.title_line() - 4.) / 2.))
							.when(layout.overview(), |light| light.mt_0())
							.size(gpui::px(4.))
							.rounded_full()
							.bg(gpui::rgb(self.color)),
					)
					.when(!layout.overview(), |row| {
						row.child(
							gpui::div()
								.debug_selector(move || identity_selector.clone())
								.flex_1()
								.min_w_0()
								.h(gpui::px(layout.title_line() * if wrap_title { 2. } else { 1. }))
								.text_size(gpui::px(layout.title_size()))
								.line_height(gpui::px(layout.title_line()))
								.text_color(gpui::rgb(TEXT))
								.font_weight(gpui::FontWeight::SEMIBOLD)
								.when(wrap_title, |title| title.whitespace_normal().line_clamp(2))
								.when(!wrap_title, |title| title.whitespace_nowrap())
								.text_ellipsis()
								.overflow_hidden()
								.child(self.identity.clone()),
						)
					})
					.child(
						gpui::div()
							.debug_selector(move || number_selector.clone())
							.flex_none()
							.h(gpui::px(layout.title_line()))
							.font_family("Menlo")
							.text_size(gpui::px(layout.detail_size()))
							.line_height(gpui::px(layout.title_line()))
							.text_color(gpui::rgb(if self.selected {
								crate::ui_theme::BLUE
							} else {
								super::card::SECONDARY_TEXT
							}))
							.child(self.number),
					),
			)
			.when(
				!layout.overview() && !self.task.is_empty() && self.task != self.identity,
				|body| {
					body.child(
						gpui::div()
							.debug_selector(move || task_selector.clone())
							.h(gpui::px(layout.task_line()))
							.text_size(gpui::px(layout.task_size()))
							.line_height(gpui::px(layout.task_line()))
							.text_color(gpui::rgb(super::card::TASK_TEXT))
							.font_weight(gpui::FontWeight::NORMAL)
							.whitespace_nowrap()
							.text_ellipsis()
							.overflow_hidden()
							.child(self.task),
					)
				},
			)
			.when(layout.full_detail() && has_actions, |body| body.child(actions))
	}
}

fn action_rows(
	key: &str,
	frame: stream::Frame,
	layout: super::card::Layout,
	window: &mut Window,
	cx: &mut App,
) -> gpui::Div {
	let activity_scale = layout.detail_line() / stream::LINE;
	if frame.moving {
		ui_motion::request_frame(window, cx);
	}
	let mut actions = gpui::div()
		.debug_selector({
			let key = key.to_owned();
			move || format!("relation-preview-{key}")
		})
		.relative()
		.w_full()
		.mt(gpui::px(layout.spacing(4.)))
		.h(gpui::px(frame.height * activity_scale))
		.flex_none()
		.overflow_hidden();
	for line in frame.lines {
		let selector = format!("relation-operation-{key}-{}", line.id);
		actions = actions.child(
			gpui::div()
				.debug_selector(move || selector.clone())
				.absolute()
				.left_0()
				.right_0()
				.top(gpui::px(line.y * activity_scale))
				.h(gpui::px(layout.detail_line()))
				.text_size(gpui::px(layout.detail_size()))
				.line_height(gpui::px(layout.detail_line()))
				.text_color(gpui::rgb(super::card::ACTIVITY_TEXT))
				.whitespace_nowrap()
				.text_ellipsis()
				.overflow_hidden()
				.opacity(line.opacity)
				.child(line.text),
		);
	}
	actions
}
