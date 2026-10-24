//! A numbered Cloudlet with source-bound activity alongside it.
use super::{mascot, stream};
use crate::{ui_motion, ui_theme::foreground};
use gpui::{App, InteractiveElement as _, IntoElement, RenderOnce, Window, prelude::*};

#[derive(IntoElement)]
pub(super) struct Header {
	pub key: String,
	pub parent: Option<String>,
	pub team_members: Option<usize>,
	pub number: String,
	pub color: u32,
	pub selected: bool,
	pub picked: bool,
	pub live: bool,
	pub task: String,
	pub avatar: mascot::Avatar,
	pub metrics: Option<gpui::AnyElement>,
	pub activity: stream::Frame,
	pub layout: super::card::Layout,
	pub interaction: super::card::Interaction,
}

impl Header {
	fn metadata(&self, layout: super::card::Layout) -> gpui::Div {
		let portrait_width = layout.portrait_spacing(super::card::PORTRAIT_WIDTH);
		let identity_selector = format!("relation-identity-{}", self.key);
		let status_selector = format!("relation-status-{}", self.key);
		let number_selector = format!("relation-number-{}", self.key);
		let identity = gpui::div()
			.id(gpui::SharedString::from(format!("relation-identity-target-{}", self.key)))
			.debug_selector(move || identity_selector.clone())
			.w(gpui::px(portrait_width))
			.h(gpui::px(layout.title_line()))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.gap(gpui::px(4.))
			.child(
				gpui::div()
					.debug_selector(move || status_selector.clone())
					.size(gpui::px(4.))
					.flex_none()
					.rounded_full()
					.bg(crate::ui_theme::rgb(self.color)),
			)
			.child(
				gpui::div()
					.debug_selector(move || number_selector.clone())
					.h(gpui::px(layout.title_line()))
					.flex_none()
					.font_features(super::card::number_features())
					.text_size(gpui::px(layout.title_size()))
					.line_height(gpui::px(layout.title_line()))
					.text_color(crate::ui_theme::rgb(if self.selected {
						crate::ui_theme::BLUE
					} else {
						foreground::BODY
					}))
					.child(self.number.clone()),
			);
		gpui::div()
			.w_full()
			.h(gpui::px(layout.title_line()))
			.flex_none()
			.flex()
			.items_center()
			.gap(gpui::px(layout.details_gap()))
			.child(self.interaction.apply(identity, false))
			.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.flex()
					.items_center()
					.when_some(self.team_members, |row, count| {
						row.child(information_rows::team(&self.key, count, layout))
					})
					.when(self.picked, |row| {
						row.when_some(self.parent.clone(), |row, parent| {
							row.child(information_rows::parent(&self.key, parent, layout))
						})
					}),
			)
	}
}

impl RenderOnce for Header {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let layout = self.layout.on_display(window.scale_factor());
		let metadata = layout.labels().then(|| self.metadata(layout));
		let h = self;
		let (task_lines, count) = content_rows(&h.task, &h.activity, layout, cx.text_system());
		let gap = content_gap(task_lines, &h.activity, layout);
		let portrait_width = layout.portrait_spacing(super::card::PORTRAIT_WIDTH);
		let selection = selection_mark(&h.key, h.selected, layout, window, cx);
		let avatar_selector = format!("relation-avatar-{}", h.key);
		let figure = gpui::div()
			.debug_selector(move || avatar_selector.clone())
			.relative()
			.flex_none()
			.w(gpui::px(portrait_width))
			.h(gpui::px(layout.portrait_spacing(super::card::PORTRAIT_HEIGHT)))
			.child(selection)
			.child(h.avatar.element(layout));
		let portrait = gpui::div()
			.id(gpui::SharedString::from(format!("relation-portrait-target-{}", h.key)))
			.flex_none()
			.w(gpui::px(portrait_width))
			.flex()
			.flex_col()
			.items_center()
			.gap(gpui::px(layout.gap()))
			.child(figure)
			.when_some(h.metrics, |portrait, metrics| portrait.child(metrics));
		let mut content = gpui::div()
			.w_full()
			.flex()
			.items_start()
			.gap(gpui::px(layout.details_gap()))
			.child(h.interaction.apply(portrait, true));
		let mut body = gpui::div()
			.w_full()
			.h(gpui::px(layout.header_height(count)))
			.font_family(crate::ui_theme::FONT_FAMILY)
			.font_weight(gpui::FontWeight::NORMAL)
			.flex_none()
			.flex()
			.flex_col()
			.gap(gpui::px(layout.metadata_gap()));
		let Some(metadata) = metadata else { return body.child(content.justify_center()) };
		body = body.child(metadata);
		if layout.activity_visible() {
			let mut details = gpui::div()
				.id(gpui::SharedString::from(format!("relation-details-target-{}", h.key)))
				.flex_1()
				.min_w_0()
				.pt(gpui::px(layout.spacing(super::card::DETAILS_TOP)))
				.flex()
				.flex_col()
				.gap(gpui::px(gap * layout.activity_line()));
			if !h.task.is_empty() {
				details = details.child(information_rows::goal(
					&h.key,
					&h.task,
					layout,
					cx.text_system(),
				));
			}
			if h.activity.height > 0. {
				details = details.child(information_rows::render(
					&h.key, h.activity, layout, h.live, window, cx,
				));
			}
			content = content.child(h.interaction.apply(details, false));
		}
		body.child(content)
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

#[path = "agent_relation_rows.rs"] mod information_rows;

fn content_gap(task_lines: usize, frame: &stream::Frame, layout: super::card::Layout) -> f32 {
	if task_lines > 0 && frame.height > 0. { 4. / layout.activity_line() } else { 0. }
}

/// The painted header and camera share the exact current text measurement.
pub(super) fn content_rows(
	task: &str,
	activity: &stream::Frame,
	layout: super::card::Layout,
	text_system: &std::sync::Arc<gpui::TextSystem>,
) -> (usize, f32) {
	if !layout.activity_visible() {
		return (0, 0.);
	}
	let lines = information_rows::task_excerpt(task, layout, text_system).lines;
	(lines, activity.height / stream::LINE + lines as f32 + content_gap(lines, activity, layout))
}

pub(super) fn detail_rows(
	task: &str,
	activity: &stream::Frame,
	layout: super::card::Layout,
	text_system: &std::sync::Arc<gpui::TextSystem>,
) -> f32 {
	let Some((min, max)) = layout.activity_range() else { return 0. };
	let task_lines = [min, max]
		.into_iter()
		.map(|zoom| information_rows::task_excerpt(task, layout.at_zoom(zoom), text_system).lines)
		.max()
		.unwrap_or(0);
	activity.layout_height / stream::LINE
		+ task_lines as f32
		+ content_gap(task_lines, activity, layout)
}

pub(super) fn operation_height(
	text: &str,
	layout: super::card::Layout,
	text_system: &std::sync::Arc<gpui::TextSystem>,
) -> (f32, f32) {
	let height = |layout| {
		information_rows::message_height(
			information_rows::message_excerpt(text, layout, text_system).lines,
			layout,
		)
	};
	let Some((min, max)) = layout.activity_range() else { return (0., 0.) };
	let reserved =
		[min, max].into_iter().map(|zoom| height(layout.at_zoom(zoom))).fold(0., f32::max);
	(if layout.activity_visible() { height(layout) } else { 0. }, reserved)
}
