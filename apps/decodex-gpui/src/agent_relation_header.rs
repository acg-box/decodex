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
	pub metrics: Option<gpui::AnyElement>,
	pub activity: stream::Frame,
	pub layout: super::card::Layout,
}

impl RenderOnce for Header {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let layout = self.layout.on_display(window.scale_factor());
		let frame = self.activity;
		let has_actions = frame.height > 0.;
		let actions_height = frame.height / stream::LINE;
		let identity_selector = format!("relation-identity-{}", self.key);
		let number_selector = format!("relation-number-{}", self.key);
		let light_selector = format!("relation-status-{}", self.key);
		let task_selector = format!("relation-assignment-{}", self.key);
		let avatar_selector = format!("relation-avatar-{}", self.key);
		let actions = action_rows(&self.key, frame, layout, window, cx);
		let selection = selection_mark(&self.key, self.selected, layout, window, cx);
		let task = if self.task.is_empty() { self.status } else { self.task };
		let identity = gpui::div()
			.debug_selector(move || identity_selector.clone())
			.h(gpui::px(layout.title_line()))
			.flex_none()
			.flex()
			.items_center()
			.gap(gpui::px(layout.spacing(4.)))
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
			)
			.child(
				gpui::div()
					.debug_selector(move || light_selector.clone())
					.size(gpui::px(4.))
					.rounded_full()
					.bg(crate::ui_theme::rgb(self.color)),
			);
		gpui::div()
			.w_full()
			.h(gpui::px(layout.header_height(actions_height)))
			.flex_none()
			.min_w_0()
			.flex()
			.items_start()
			.gap(gpui::px(layout.spacing(super::card::COLUMN_GAP)))
			.when(!layout.labels(), |body| body.justify_center())
			.child(
				gpui::div()
					.flex_none()
					.w(gpui::px(layout.portrait_spacing(super::card::PORTRAIT_WIDTH)))
					.flex()
					.flex_col()
					.items_center()
					.gap(gpui::px(layout.gap()))
					.child(
						gpui::div()
							.debug_selector(move || avatar_selector.clone())
							.relative()
							.flex_none()
							.w(gpui::px(layout.portrait_spacing(super::card::PORTRAIT_WIDTH)))
							.h(gpui::px(layout.portrait_spacing(super::card::PORTRAIT_HEIGHT)))
							.child(selection)
							.child(self.avatar.element(layout)),
					)
					.when_some(self.metrics, |portrait, metrics| portrait.child(metrics)),
			)
			.when(layout.labels(), |body| {
				body.child(
					gpui::div()
						.flex_1()
						.min_w_0()
						.pt(gpui::px(layout.spacing(super::card::DETAILS_TOP)))
						.flex()
						.flex_col()
						.gap(gpui::px(layout.spacing(super::card::IDENTITY_GAP)))
						.child(identity)
						.when(!has_actions, |details| {
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

/// Selection belongs to the character. Fade its small ground ring without
/// changing the hit bounds, the sprite's motion, or the node's placement.
fn selection_mark(
	key: &str,
	selected: bool,
	layout: super::card::Layout,
	window: &mut Window,
	cx: &mut App,
) -> gpui::Div {
	let selector = format!("relation-selection-{key}");
	let opacity = ui_motion::value(
		gpui::SharedString::from(format!("graph-selection-{key}")),
		if selected { 1. } else { 0. },
		window,
		cx,
	);
	gpui::div()
		.debug_selector(move || selector.clone())
		.absolute()
		.inset_0()
		.opacity(opacity)
		.child(
			gpui::canvas(
				|_, _, _| (),
				move |bounds, _, window, _| {
					let center = gpui::point(
						bounds.center().x,
						bounds.top() + gpui::px(layout.portrait_spacing(42.)),
					);
					let (rx, ry) =
						(layout.portrait_spacing(22.), layout.portrait_spacing(2.5).max(1.));
					let p = |x, y| center + gpui::point(gpui::px(x), gpui::px(y));
					let mut ring = gpui::PathBuilder::stroke(gpui::px(1.));
					let k = 0.552_284_8;
					ring.move_to(p(rx, 0.));
					ring.cubic_bezier_to(p(0., ry), p(rx, k * ry), p(k * rx, ry));
					ring.cubic_bezier_to(p(-rx, 0.), p(-k * rx, ry), p(-rx, k * ry));
					ring.cubic_bezier_to(p(0., -ry), p(-rx, -k * ry), p(-k * rx, -ry));
					ring.cubic_bezier_to(p(rx, 0.), p(k * rx, -ry), p(rx, -k * ry));
					if let Ok(path) = ring.build() {
						window.paint_path(
							path,
							crate::ui_theme::rgb(crate::ui_theme::BLUE).opacity(0.65),
						);
					}
				},
			)
			.size_full(),
		)
}

fn action_rows(
	key: &str,
	frame: stream::Frame,
	layout: super::card::Layout,
	window: &mut Window,
	cx: &mut App,
) -> gpui::Div {
	let activity_scale = layout.activity_line() / stream::LINE;
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
				.h(gpui::px(layout.activity_line()))
				.text_size(gpui::px(layout.activity_size()))
				.line_height(gpui::px(layout.activity_line()))
				.text_color(crate::ui_theme::rgb(if line.current {
					foreground::PRIMARY
				} else {
					super::card::ACTIVITY_TEXT
				}))
				.whitespace_nowrap()
				.text_ellipsis()
				.overflow_hidden()
				.opacity(line.opacity)
				.child(line.text),
		);
	}
	actions
}
