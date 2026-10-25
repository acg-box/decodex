//! A character speaks above its head; its stable identity stays at its feet.
use super::{mascot, stream};
use crate::{
	ui_motion::{self, SmoothControl as _},
	ui_theme::foreground,
};
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
	fn metadata(
		&self,
		layout: super::card::Layout,
		window: &mut Window,
		cx: &mut App,
	) -> gpui::Stateful<gpui::Div> {
		let identity_selector = format!("relation-identity-{}", self.key);
		let status_selector = format!("relation-status-{}", self.key);
		let number_selector = format!("relation-number-{}", self.key);
		let identity = gpui::div()
			.id(gpui::SharedString::from(format!("relation-identity-target-{}", self.key)))
			.debug_selector(move || identity_selector.clone())
			.h(gpui::px(layout.title_line()))
			.flex_none()
			.flex()
			.items_center()
			.gap(gpui::px(3.))
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
			)
			.when(self.picked, |row| {
				row.when_some(self.parent.clone(), |row, parent| {
					row.child(information_rows::parent(&self.key, parent, layout))
				})
			})
			.when(!(self.picked && self.parent.is_some()), |row| {
				row.when_some(self.team_members, |row, count| {
					row.child(self.team_button(count, layout, window, cx))
				})
			});
		let lines = if layout.activity_visible() {
			information_rows::task_excerpt(&self.task, layout, cx.text_system()).lines.max(1)
		} else {
			1
		};
		let caption = gpui::div()
			.id(gpui::SharedString::from(format!("relation-caption-target-{}", self.key)))
			.w_full()
			.h(gpui::px(lines as f32 * layout.title_line()))
			.flex_none()
			.flex()
			.items_start()
			.justify_center()
			.gap(gpui::px(6.))
			.child(identity)
			.when(layout.activity_visible() && !self.task.is_empty(), |row| {
				row.child(
					gpui::div().max_w(gpui::px(layout.caption_width())).flex_none().child(
						information_rows::goal(&self.key, &self.task, layout, cx.text_system()),
					),
				)
			});
		self.interaction.apply(caption, false)
	}

	fn team_button(
		&self,
		count: usize,
		layout: super::card::Layout,
		window: &mut Window,
		cx: &mut App,
	) -> gpui::AnyElement {
		let focus = window
			.use_keyed_state(
				gpui::SharedString::from(format!("relation-team-focus-{}", self.key)),
				cx,
				|_, cx| cx.focus_handle().tab_stop(true),
			)
			.read(cx)
			.clone();
		let team = information_rows::team(&self.key, count, layout)
			.id(gpui::SharedString::from(format!("relation-team-target-{}", self.key)))
			.role(gpui::Role::Button)
			.tab_index(0)
			.track_focus(&focus)
			.aria_label(format!("Frame {} team, {count} agents", self.number))
			.px(gpui::px(1.))
			.rounded(gpui::px(3.))
			.hover(|style| style.bg(crate::ui_theme::rgba(crate::ui_theme::HOVER_FILL)))
			.focus_visible(|style| style.text_color(crate::ui_theme::rgb(crate::ui_theme::BLUE)))
			.on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
				window.focus(&focus, cx);
				cx.stop_propagation();
			});
		self.interaction.frame_team(team).smooth().into_any_element()
	}
}

impl RenderOnce for Header {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let layout = self.layout.on_display(window.scale_factor());
		if self.activity.moving && layout.activity_visible() {
			ui_motion::request_frame(window, cx);
		}
		let metadata = layout.labels().then(|| self.metadata(layout, window, cx));
		let h = self;
		let (_, count) = content_rows(&h.task, &h.activity, layout, cx.text_system());
		let speech_height = h.activity.height * layout.activity_line() / stream::LINE;
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
		let content = gpui::div()
			.w_full()
			.mt(gpui::px(layout.speech_offset(h.activity.height / stream::LINE)))
			.flex()
			.items_center()
			.justify_center()
			.child(h.interaction.apply(portrait, true));
		let mut body = gpui::div()
			.w_full()
			.h(gpui::px(layout.header_height(count)))
			.relative()
			.font_family(crate::ui_theme::FONT_FAMILY)
			.font_weight(gpui::FontWeight::NORMAL)
			.flex_none()
			.flex()
			.flex_col()
			.gap(gpui::px(layout.metadata_gap()));
		let Some(metadata) = metadata else { return body.child(content.justify_center()) };
		if layout.activity_visible() && speech_height > 0. {
			let details = gpui::div()
				.id(gpui::SharedString::from(format!("relation-details-target-{}", h.key)))
				.absolute()
				.top_0()
				.left(gpui::px(layout.speech_left()))
				.w(gpui::px(layout.details_width()))
				.h(gpui::px(speech_height))
				.child(information_rows::render(&h.key, h.activity, layout, h.live, window, cx));
			body = body.child(h.interaction.apply(details, false));
		}
		body.child(content).child(metadata)
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

pub(super) fn speech_width(
	activity: &stream::Frame,
	layout: super::card::Layout,
	system: &std::sync::Arc<gpui::TextSystem>,
	shaper: &gpui::WindowTextSystem,
) -> Option<f32> {
	activity
		.lines
		.iter()
		.map(|line| {
			information_rows::bubble_width(
				&information_rows::message_excerpt(&line.text, layout, system),
				layout,
				shaper,
			)
		})
		.reduce(f32::max)
}

fn content_gap(height: f32, layout: super::card::Layout) -> f32 {
	// The sprite has transparent headroom. Let the short tail enter that space.
	if height > 0. { -layout.spacing(2.) / layout.activity_line() } else { 0. }
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
	(
		lines,
		activity.height / stream::LINE
			+ lines.saturating_sub(1) as f32
			+ content_gap(activity.height, layout),
	)
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
		+ task_lines.saturating_sub(1) as f32
		+ [min, max]
			.into_iter()
			.map(|zoom| content_gap(activity.layout_height, layout.at_zoom(zoom)))
			.fold(f32::NEG_INFINITY, f32::max)
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
