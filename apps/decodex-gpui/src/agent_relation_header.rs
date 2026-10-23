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
	pub task: String,
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
		let count = if show_activity { detail_rows(&h.task, &h.activity) } else { 0. };
		let portrait_width = layout.portrait_spacing(super::card::PORTRAIT_WIDTH);
		let selection = selection_mark(&h.key, h.selected, layout, window, cx);
		let avatar_selector = format!("relation-avatar-{}", h.key);
		let identity_selector = format!("relation-identity-{}", h.key);
		let status_selector = format!("relation-status-{}", h.key);
		let number_selector = format!("relation-number-{}", h.key);
		let figure = gpui::div()
			.debug_selector(move || avatar_selector.clone())
			.relative()
			.flex_none()
			.w(gpui::px(portrait_width))
			.h(gpui::px(layout.portrait_spacing(super::card::PORTRAIT_HEIGHT)))
			.child(selection)
			.child(h.avatar.element(layout));
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
					.flex()
					.items_center()
					.gap(gpui::px(4.))
					.child(
						gpui::div()
							.debug_selector(move || status_selector.clone())
							.size(gpui::px(4.))
							.flex_none()
							.rounded_full()
							.bg(crate::ui_theme::rgb(h.color)),
					)
					.child(identity)
					.when_some(
						h.team_members.filter(|_| !h.picked || layout.zoom > 0.5),
						|row, count| row.child(information_rows::team(&h.key, count, layout)),
					)
					.when(h.picked, |row| {
						row.when_some(h.parent.clone(), |row, parent| {
							row.child(information_rows::parent(&h.key, parent, layout))
						})
					}),
			);
		if show_activity {
			let mut content = gpui::div().flex().flex_col().w_full();
			if !h.task.is_empty() {
				content = content.child(information_rows::goal(&h.key, h.task.clone(), layout));
			}
			if h.activity.height > 0. {
				content = content.child(action_rows(&h.key, h.activity, layout, window, cx));
			}
			details = details.child(content);
		}
		body = body.child(details);
		body
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

fn action_rows(
	key: &str,
	frame: stream::Frame,
	layout: super::card::Layout,
	window: &mut Window,
	cx: &mut App,
) -> gpui::Div {
	information_rows::render(key, frame, layout, window, cx)
}

pub(super) fn detail_rows(task: &str, activity: &stream::Frame) -> f32 {
	activity.height / stream::LINE + if task.is_empty() { 0. } else { 1. }
}
