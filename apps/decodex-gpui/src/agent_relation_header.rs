//! Compact canvas labels with an inline status light and a quiet agent number.
use super::stream;
use crate::{ui_motion, ui_theme::foreground};
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
		let overview_task_selector = task_selector.clone();
		let role_selector = format!("relation-role-{}", self.key);
		let separate_task = !self.task.is_empty() && self.task != self.identity;
		let task = if separate_task { self.task } else { self.identity.clone() };
		let details = separate_task && !layout.overview();
		let title = if details { self.identity.clone() } else { task.clone() };
		let line = if details { layout.task_line() } else { layout.title_line() };
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
					.when(!layout.labels(), |row| row.justify_center().items_center())
					.gap(gpui::px(layout.spacing(7.)))
					.child(
						gpui::div()
							.debug_selector(move || light_selector.clone())
							.flex_none()
							.mt(gpui::px((line - 4.) / 2.))
							.when(!layout.labels(), |light| light.mt_0())
							.size(gpui::px(4.))
							.rounded_full()
							.bg(crate::ui_theme::rgb(self.color)),
					)
					.when(layout.labels(), |row| {
						row.child(
							gpui::div()
								.flex_1()
								.min_w_0()
								.when(details, |slot| {
									slot.debug_selector(move || role_selector.clone())
								})
								.when(!details && separate_task, |slot| {
									slot.debug_selector(move || overview_task_selector.clone())
								})
								.child(
									gpui::div()
										.debug_selector(move || identity_selector.clone())
										.h(gpui::px(line))
										.text_size(gpui::px(if details {
											layout.task_size()
										} else {
											layout.title_size()
										}))
										.line_height(gpui::px(line))
										.text_color(crate::ui_theme::rgb(if details {
											foreground::BODY
										} else {
											foreground::PRIMARY
										}))
										.font_weight(gpui::FontWeight::NORMAL)
										.whitespace_nowrap()
										.text_ellipsis()
										.overflow_hidden()
										.child(title),
								),
						)
					})
					.child(
						gpui::div()
							.debug_selector(move || number_selector.clone())
							.flex_none()
							.h(gpui::px(line))
							.font_family("Menlo")
							.text_size(gpui::px(layout.detail_size()))
							.line_height(gpui::px(line))
							.text_color(crate::ui_theme::rgb(if self.selected {
								crate::ui_theme::BLUE
							} else {
								super::card::SECONDARY_TEXT
							}))
							.child(self.number),
					),
			)
			.when(details, |body| {
				body.child(
					gpui::div()
						.debug_selector(move || task_selector.clone())
						.h(gpui::px(layout.title_line()))
						.text_size(gpui::px(layout.title_size()))
						.line_height(gpui::px(layout.title_line()))
						.text_color(crate::ui_theme::rgb(foreground::PRIMARY))
						.whitespace_nowrap()
						.text_ellipsis()
						.overflow_hidden()
						.child(task),
				)
			})
			.when(!layout.overview() && has_actions, |body| body.child(actions))
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
				.text_size(gpui::px(layout.activity_size()))
				.line_height(gpui::px(layout.detail_line()))
				.text_color(crate::ui_theme::rgb(super::card::ACTIVITY_TEXT))
				.whitespace_nowrap()
				.text_ellipsis()
				.overflow_hidden()
				.opacity(line.opacity)
				.child(line.text),
		);
	}
	actions
}
