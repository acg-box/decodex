//! A numbered Cloudlet with source-bound activity alongside it.
use super::{mascot, stream};
use crate::{ui_motion, ui_theme::foreground};
use gpui::{App, IntoElement, RenderOnce, Window, prelude::*};

#[derive(IntoElement)]
pub(super) struct Header {
	pub key: String,
	pub number: String,
	pub color: u32,
	pub selected: bool,
	pub task: String,
	pub status: String,
	pub avatar: mascot::Avatar,
	pub activity: stream::Frame,
	pub layout: super::card::Layout,
}

impl RenderOnce for Header {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let layout = self.layout.on_display(window.scale_factor());
		let frame = if layout.overview() { self.activity.latest() } else { self.activity };
		let has_actions = frame.height > 0.;
		let actions_height = frame.height / stream::LINE;
		let identity_selector = format!("relation-identity-{}", self.key);
		let number_selector = format!("relation-number-{}", self.key);
		let light_selector = format!("relation-status-{}", self.key);
		let task_selector = format!("relation-assignment-{}", self.key);
		let avatar_selector = format!("relation-avatar-{}", self.key);
		let actions = action_rows(&self.key, frame, layout, window, cx);
		let task = if self.task.is_empty() { self.status } else { self.task };
		gpui::div()
			.w_full()
			.h(gpui::px(layout.header_height(actions_height)))
			.flex_none()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(layout.spacing(12.)))
			.when(!layout.labels(), |body| body.justify_center())
			.child(
				gpui::div()
					.debug_selector(move || avatar_selector.clone())
					.flex_none()
					.w(gpui::px(layout.spacing(super::card::PORTRAIT_WIDTH)))
					.flex()
					.flex_col()
					.items_center()
					.gap(gpui::px(layout.spacing(2.)))
					.child(self.avatar.element(layout))
					.when(layout.labels(), |portrait| {
						portrait.child(
							gpui::div()
								.debug_selector(move || identity_selector.clone())
								.h(gpui::px(layout.title_line()))
								.flex()
								.items_center()
								.gap(gpui::px(layout.spacing(5.)))
								.child(
									gpui::div()
										.debug_selector(move || light_selector.clone())
										.size(gpui::px(4.))
										.rounded_full()
										.bg(crate::ui_theme::rgb(self.color)),
								)
								.child(
									gpui::div()
										.debug_selector(move || number_selector.clone())
										.font_family("Menlo")
										.text_size(gpui::px(layout.title_size()))
										.line_height(gpui::px(layout.title_line()))
										.font_weight(gpui::FontWeight::MEDIUM)
										.text_color(crate::ui_theme::rgb(if self.selected {
											crate::ui_theme::BLUE
										} else {
											foreground::PRIMARY
										}))
										.child(self.number),
								),
						)
					}),
			)
			.when(layout.labels(), |body| {
				body.child(
					gpui::div()
						.flex_1()
						.min_w_0()
						.flex()
						.flex_col()
						.justify_center()
						.when(!layout.overview() || !has_actions, |details| {
							details.child(
								gpui::div()
									.debug_selector(move || task_selector.clone())
									.h(gpui::px(layout.title_line()))
									.text_size(gpui::px(layout.title_size()))
									.line_height(gpui::px(layout.title_line()))
									.font_weight(gpui::FontWeight::NORMAL)
									.text_color(crate::ui_theme::rgb(foreground::PRIMARY))
									.whitespace_nowrap()
									.text_ellipsis()
									.overflow_hidden()
									.child(task),
							)
						})
						.when(has_actions, |details| details.child(actions)),
				)
			})
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
	if frame.moving && layout.labels() {
		ui_motion::request_frame(window, cx);
	}
	let mut actions = gpui::div()
		.debug_selector({
			let key = key.to_owned();
			move || format!("relation-preview-{key}")
		})
		.relative()
		.w_full()
		.when(!layout.overview(), |actions| actions.mt(gpui::px(layout.spacing(4.))))
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
