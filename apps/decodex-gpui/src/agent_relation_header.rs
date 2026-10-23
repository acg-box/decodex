//! A numbered Cloudlet with source-bound activity alongside it.
use super::{mascot, stream};
use crate::{ui_motion, ui_theme::foreground};
use gpui::{App, InteractiveElement as _, IntoElement, RenderOnce, Window, prelude::*};

#[derive(IntoElement)]
pub(super) struct Header {
	pub key: String,
	pub parent: Option<String>,
	pub number: String,
	pub color: u32,
	pub selected: bool,
	pub picked: bool,
	pub task: String,
	pub status: String,
	pub avatar: mascot::Avatar,
	pub metrics: Option<gpui::AnyElement>,
	pub activity: stream::Frame,
	pub layout: super::card::Layout,
}

impl RenderOnce for Header {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let h = self;
		let layout = h.layout.on_display(window.scale_factor());
		let show_activity = layout.activity_visible();
		let count = if show_activity { h.activity.height / stream::LINE } else { 0. };
		let portrait_width = layout.portrait_spacing(super::card::PORTRAIT_WIDTH);
		let selection = selection_mark(&h.key, h.selected, layout, window, cx);
		let avatar_selector = format!("relation-avatar-{}", h.key);
		let identity_selector = format!("relation-identity-{}", h.key);
		let task_selector = format!("relation-assignment-{}", h.key);
		let status_selector = format!("relation-status-{}", h.key);
		let number_selector = format!("relation-number-{}", h.key);
		let hint = h
			.picked
			.then(|| h.parent.as_ref().map(|parent| format!("Reports to {parent}")))
			.flatten();
		let mut figure = gpui::div()
			.debug_selector(move || avatar_selector.clone())
			.relative()
			.flex_none()
			.w(gpui::px(portrait_width))
			.h(gpui::px(layout.portrait_spacing(super::card::PORTRAIT_HEIGHT)))
			.child(selection)
			.child(h.avatar.element(layout))
			.child(
				gpui::div()
					.debug_selector(move || status_selector.clone())
					.absolute()
					.left(gpui::px(portrait_width + 4.))
					.top(gpui::px(layout.portrait_spacing(27.)))
					.size(gpui::px(layout.spacing(5.).max(4.) + 2.))
					.rounded_full()
					.border_1()
					.border_color(crate::ui_theme::rgba(0x00000070))
					.bg(crate::ui_theme::rgb(h.color)),
			);
		if let Some(text) = hint {
			let hint_selector = format!("relation-owner-hint-{}", h.key);
			let width = layout.pixel(measure_hint(&text, layout, window).ceil() + 10.);
			let height = layout.title_line() + 6.;
			figure = figure.child(
				crate::ui_motion::tooltip_surface(gpui::div())
					.debug_selector(move || hint_selector.clone())
					.absolute()
					.left(gpui::px(layout.pixel(portrait_width - width - 4.)))
					.top(gpui::px(-height - 2.))
					.w(gpui::px(width))
					.h(gpui::px(height))
					.font_family(crate::ui_theme::FONT_FAMILY)
					.font_weight(gpui::FontWeight::NORMAL)
					.text_size(gpui::px(layout.title_size()))
					.line_height(gpui::px(layout.title_line()))
					.text_color(crate::ui_theme::rgb(foreground::PRIMARY))
					.whitespace_nowrap()
					.flex()
					.items_center()
					.justify_center()
					.child(text),
			);
		}
		let portrait = gpui::div()
			.flex_none()
			.w(gpui::px(portrait_width))
			.flex()
			.flex_col()
			.items_center()
			.gap(gpui::px(layout.gap()))
			.child(figure)
			.when_some(h.metrics, |portrait, metrics| portrait.child(metrics));
		let mut body = gpui::div()
			.w_full()
			.h(gpui::px(layout.header_height(count)))
			.font_family(crate::ui_theme::FONT_FAMILY)
			.font_weight(gpui::FontWeight::NORMAL)
			.flex_none()
			.flex()
			.items_start()
			.gap(gpui::px(layout.details_gap()))
			.child(portrait);
		if !layout.labels() {
			return body.justify_center();
		}
		let identity = gpui::div()
			.debug_selector(move || number_selector.clone())
			.h(gpui::px(layout.title_line()))
			.flex_none()
			.font_family(crate::ui_theme::FONT_FAMILY)
			.font_features(super::card::number_features())
			.text_size(gpui::px(layout.title_size()))
			.line_height(gpui::px(layout.title_line()))
			.font_weight(gpui::FontWeight::NORMAL)
			.text_color(crate::ui_theme::rgb(if h.selected {
				crate::ui_theme::BLUE
			} else {
				foreground::PRIMARY
			}))
			.child(h.number);
		let mut details = gpui::div()
			.flex_1()
			.min_w_0()
			.pt(gpui::px(layout.spacing(super::card::DETAILS_TOP)))
			.flex()
			.flex_col()
			.gap(gpui::px(layout.spacing(super::card::IDENTITY_GAP)))
			.child(
				gpui::div()
					.debug_selector(move || identity_selector.clone())
					.h(gpui::px(layout.title_line()))
					.child(identity),
			);
		if show_activity {
			let activity = if h.activity.height > 0. {
				action_rows(&h.key, h.activity, layout, window, cx)
			} else {
				gpui::div()
					.debug_selector(move || task_selector.clone())
					.w_full()
					.h(gpui::px(layout.activity_line()))
					.text_size(gpui::px(layout.activity_size()))
					.line_height(gpui::px(layout.activity_line()))
					.text_color(crate::ui_theme::rgb(foreground::PRIMARY))
					.whitespace_nowrap()
					.text_ellipsis()
					.overflow_hidden()
					.child(if h.task.is_empty() { h.status } else { h.task })
			};
			details = details.child(activity);
		}
		body = body.child(details);
		body
	}
}

fn measure_hint(text: &str, layout: super::card::Layout, window: &mut Window) -> f32 {
	let mut font = window.text_style().font();
	font.family = crate::ui_theme::FONT_FAMILY.into();
	font.weight = gpui::FontWeight::NORMAL;
	font.features = gpui::FontFeatures::default();
	f32::from(
		window
			.text_system()
			.shape_line(
				text.to_owned().into(),
				gpui::px(layout.title_size()),
				&[gpui::TextRun { len: text.len(), font, ..Default::default() }],
				None,
			)
			.width,
	)
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
	if frame.moving && layout.activity_visible() {
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
