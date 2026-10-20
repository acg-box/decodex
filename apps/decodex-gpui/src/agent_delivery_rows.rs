//! One compact work item, with source-backed changes revealed in place.
use super::{DATE_HEIGHT, ENTRANCE, Kind, OpenDelivery, Panel, clock, day_label};
use crate::{
	shell::workspace_symbols::{self, Symbol},
	ui_motion::{self, SmoothControl},
	ui_theme,
};
use decodex_protocol::{AgentDeliveryEntry, AgentDeliveryUpdate, AgentDeliveryUpdateKind};
use gpui::{Context, IntoElement, Window, prelude::*};

pub(super) const RAIL_WIDTH: f32 = 12.;
pub(super) const CONTENT_GAP: f32 = 8.;
const ROW_TOP: f32 = 8.;
const ROW_BOTTOM: f32 = 14.;
const META_HEIGHT: f32 = 24.;
pub(super) const MARKER_Y: f32 = ROW_TOP + META_HEIGHT / 2.;

pub(super) struct Placement {
	pub date: bool,
	pub height: f32,
}

pub(super) struct CachedLayout {
	entry: AgentDeliveryEntry,
	width: f32,
	expanded: bool,
	pub layout: std::rc::Rc<RowLayout>,
}
impl CachedLayout {
	pub fn get(
		cache: &mut std::collections::BTreeMap<i64, Self>,
		entry: &AgentDeliveryEntry,
		width: f32,
		expanded: bool,
		window: &Window,
	) -> std::rc::Rc<RowLayout> {
		let stale = cache.get(&entry.id).is_none_or(|old| {
			old.entry != *entry || old.width != width || old.expanded != expanded
		});
		if stale {
			cache.insert(
				entry.id,
				Self {
					entry: entry.clone(),
					width,
					expanded,
					layout: std::rc::Rc::new(RowLayout::new(entry, width, expanded, window)),
				},
			);
		}
		cache.get(&entry.id).expect("measured row").layout.clone()
	}
}

#[derive(Clone)]
pub(super) struct RowLayout {
	pub height: f32,
	title: String,
	title_height: f32,
	summary_height: f32,
	updates: Vec<(AgentDeliveryUpdate, f32)>,
	artifacts: usize,
	details_height: f32,
	expandable: bool,
}
impl RowLayout {
	fn new(entry: &AgentDeliveryEntry, width: f32, expanded: bool, window: &Window) -> Self {
		let title = entry.title.trim().to_owned();
		let full_title =
			super::text_height(&title, 12.5, 17., gpui::FontWeight::MEDIUM, width, None, window);
		let full_summary = if entry.summary.is_empty() {
			0.
		} else {
			super::text_height(
				&entry.summary,
				11.5,
				16.,
				gpui::FontWeight::NORMAL,
				width,
				None,
				window,
			)
		};
		let title_height = if expanded { full_title } else { full_title.min(34.) };
		let summary_height = if expanded { full_summary } else { full_summary.min(32.) };
		let mut updates: Vec<_> = entry
			.updates
			.iter()
			.rev()
			.filter(|update| {
				!(update.kind == AgentDeliveryUpdateKind::Result
					&& entry.summary.contains(&update.summary))
			})
			.take(2)
			.map(|update| {
				let h = super::text_height(
					&update.summary,
					11.5,
					16.,
					gpui::FontWeight::NORMAL,
					width,
					Some(2),
					window,
				);
				(update.clone(), h)
			})
			.collect();
		updates.reverse();
		let artifacts = entry.artifacts.len().min(3);
		let details_height = updates.iter().map(|(_, h)| 10. + 14. + 3. + h).sum::<f32>()
			+ artifacts.saturating_sub(1) as f32 * 25.;
		let expandable = full_title > 34. || full_summary > 32. || details_height > 0.;
		let height = ROW_TOP
			+ META_HEIGHT
			+ 5.
			+ title_height
			+ if summary_height > 0. { 4. + summary_height } else { 0. }
			+ artifacts.min(1) as f32 * 25.
			+ if expanded { details_height } else { 0. }
			+ ROW_BOTTOM;
		Self {
			height,
			title,
			title_height,
			summary_height,
			updates,
			artifacts,
			details_height,
			expandable,
		}
	}
}

impl Panel {
	fn row_body(
		&self,
		entry: &AgentDeliveryEntry,
		layout: &RowLayout,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let expanded = self.expanded == Some(entry.id);
		let title_height = ui_motion::value(
			gpui::SharedString::from(format!("delivery-title-height-{}", entry.id)),
			layout.title_height,
			window,
			cx,
		);
		let summary_height = ui_motion::value(
			gpui::SharedString::from(format!("delivery-summary-height-{}", entry.id)),
			layout.summary_height,
			window,
			cx,
		);
		let progress = ui_motion::value(
			gpui::SharedString::from(format!("delivery-details-{}", entry.id)),
			if expanded { 1. } else { 0. },
			window,
			cx,
		);
		let identity_title = self
			.filter
			.agents
			.iter()
			.find(|agent| agent.id == entry.work_id)
			.map_or(layout.title.as_str(), |agent| agent.label.as_str());
		let initial = identity_title
			.chars()
			.find(|c| c.is_alphanumeric())
			.unwrap_or('A')
			.to_uppercase()
			.to_string();
		let identity = entry.slot.map_or(initial.clone(), |slot| format!("{initial} A{slot}"));
		let header = gpui::div()
			.debug_selector({
				let id = entry.id;
				move || format!("delivery-meta-{id}")
			})
			.mb(gpui::px(5.))
			.h(gpui::px(META_HEIGHT))
			.flex_none()
			.flex()
			.items_center()
			.gap(gpui::px(6.))
			.whitespace_nowrap()
			.text_size(gpui::px(ui_theme::CAPTION_SIZE))
			.line_height(gpui::px(META_HEIGHT))
			.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
			.child(gpui::div().flex_none().font_weight(gpui::FontWeight::MEDIUM).child(identity))
			.child(gpui::div().flex_1())
			.when(self.width >= 220., |row| {
				row.child(gpui::div().flex_none().child(clock(entry.updated_at_micros)))
			})
			.when(layout.expandable, |row| row.child(self.expand_control(entry, cx)))
			.when(self.active_work.contains(&entry.work_id), |row| {
				row.child(Self::source_arrow(entry, cx))
			});
		let mut body = gpui::div()
			.min_w_0()
			.flex_1()
			.flex()
			.flex_col()
			.child(header)
			.child(
				gpui::div()
					.id(gpui::SharedString::from(format!("delivery-request-{}", entry.id)))
					.debug_selector({
						let id = entry.id;
						move || format!("delivery-request-{id}")
					})
					.h(gpui::px(title_height))
					.flex_none()
					.overflow_hidden()
					.whitespace_normal()
					.text_size(gpui::px(12.5))
					.line_height(gpui::px(17.))
					.font_weight(gpui::FontWeight::MEDIUM)
					.child(layout.title.clone()),
			)
			.when(summary_height > 0., |body| {
				body.child(
					gpui::div()
						.id(gpui::SharedString::from(format!("delivery-response-{}", entry.id)))
						.debug_selector({
							let id = entry.id;
							move || format!("delivery-response-{id}")
						})
						.mt(gpui::px(4.))
						.h(gpui::px(summary_height))
						.flex_none()
						.overflow_hidden()
						.whitespace_normal()
						.text_size(gpui::px(11.5))
						.line_height(gpui::px(16.))
						.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
						.child(entry.summary.clone()),
				)
			});
		if let Some(artifact) = entry.artifacts.first() {
			body = body.child(Self::artifact(entry.id, 0, artifact, cx));
		}
		if progress <= 0.001 {
			return body;
		}
		let details = self.row_details(entry, layout, cx);

		body.child(
			gpui::div()
				.h(gpui::px(layout.details_height * progress))
				.flex_none()
				.overflow_hidden()
				.opacity(progress)
				.child(details),
		)
	}

	fn row_details(
		&self,
		entry: &AgentDeliveryEntry,
		layout: &RowLayout,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let mut details = gpui::div().w_full().flex_none().flex().flex_col();
		for (update, height) in &layout.updates {
			let (label, color) = match update.kind {
				AgentDeliveryUpdateKind::Progress => ("Progress", ui_theme::TEXT_MUTED),
				AgentDeliveryUpdateKind::Decision => ("Your choice", ui_theme::BLUE),
				AgentDeliveryUpdateKind::Result => ("Earlier result", ui_theme::TEXT_MUTED),
			};
			details = details.child(
				gpui::div()
					.mt(gpui::px(10.))
					.flex_none()
					.flex()
					.flex_col()
					.child(
						gpui::div()
							.h(gpui::px(14.))
							.flex()
							.items_center()
							.gap(gpui::px(6.))
							.text_size(gpui::px(ui_theme::CAPTION_SIZE))
							.line_height(gpui::px(14.))
							.text_color(gpui::rgb(color))
							.child(label)
							.child(gpui::div().flex_1())
							.child(clock(update.created_at_micros)),
					)
					.child(
						gpui::div()
							.mt(gpui::px(3.))
							.h(gpui::px(*height))
							.overflow_hidden()
							.whitespace_normal()
							.text_size(gpui::px(11.5))
							.line_height(gpui::px(16.))
							.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
							.child(update.summary.clone()),
					),
			);
		}
		for (index, artifact) in entry.artifacts.iter().take(layout.artifacts).enumerate().skip(1) {
			details = details.child(Self::artifact(entry.id, index, artifact, cx));
		}

		details
	}

	fn expand_control(
		&self,
		entry: &AgentDeliveryEntry,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let id = entry.id;
		let expanded = self.expanded == Some(id);
		let action = if expanded { "Collapse" } else { "Expand" };
		let aria = entry.slot.map_or_else(
			|| format!("{action} conversation content"),
			|slot| format!("{action} A{slot} content"),
		);
		gpui::div()
			.id(("delivery-expand", id as u64))
			.debug_selector(move || format!("delivery-expand-{id}"))
			.role(gpui::Role::Button)
			.tab_index(0)
			.aria_expanded(expanded)
			.aria_label(aria)
			.size(gpui::px(META_HEIGHT))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.cursor_pointer()
			.child(workspace_symbols::process_chevron(
				gpui::SharedString::from(format!("delivery-chevron-{id}")),
				expanded,
			))
			.on_click(cx.listener(move |s, _, _, cx| s.toggle_entry(id, cx)))
			.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					s.toggle_entry(id, cx);
					cx.stop_propagation();
				}
			}))
			.smooth()
			.into_any_element()
	}

	fn source_arrow(entry: &AgentDeliveryEntry, cx: &mut Context<Self>) -> gpui::AnyElement {
		let id = entry.id;
		let work = entry.work_id.clone();
		let keyboard_work = work.clone();
		let aria = entry.slot.map_or_else(
			|| "Open conversation".to_owned(),
			|slot| format!("Open A{slot} conversation"),
		);
		gpui::div()
			.id(("delivery-open", id as u64))
			.debug_selector(move || format!("delivery-open-{id}"))
			.role(gpui::Role::Button)
			.tab_index(0)
			.aria_label(aria)
			.size(gpui::px(META_HEIGHT))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.cursor_pointer()
			.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
			.child(workspace_symbols::icon_sized(Symbol::Forward, 13.))
			.on_click(cx.listener(move |_, _, _, cx| {
				cx.stop_propagation();
				cx.emit(OpenDelivery(work.clone()));
			}))
			.on_key_down(cx.listener(move |_, e: &gpui::KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					cx.stop_propagation();
					cx.emit(OpenDelivery(keyboard_work.clone()));
				}
			}))
			.smooth()
			.into_any_element()
	}

	fn artifact(
		id: i64,
		index: usize,
		artifact: &decodex_protocol::AgentDeliveryArtifact,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let target = artifact.target.clone();
		let keyboard_target = target.clone();
		gpui::div()
			.id(gpui::SharedString::from(format!("delivery-artifact-{id}-{index}")))
			.role(gpui::Role::Button)
			.tab_index(0)
			.cursor_pointer()
			.aria_label(format!("Open {}", artifact.title))
			.mt(gpui::px(5.))
			.h(gpui::px(20.))
			.flex_none()
			.flex()
			.items_center()
			.gap(gpui::px(5.))
			.text_size(gpui::px(11.5))
			.line_height(gpui::px(16.))
			.text_color(gpui::rgb(ui_theme::BLUE))
			.child(workspace_symbols::icon_sized(
				if target.starts_with("file:") { Symbol::Document } else { Symbol::Link },
				12.,
			))
			.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.text_ellipsis()
					.whitespace_nowrap()
					.overflow_hidden()
					.child(artifact.title.clone()),
			)
			.on_click(cx.listener(move |_, _, _, cx| {
				cx.stop_propagation();
				cx.open_url(&target);
			}))
			.on_key_down(cx.listener(move |_, e: &gpui::KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					cx.stop_propagation();
					cx.open_url(&keyboard_target);
				}
			}))
			.smooth()
			.into_any_element()
	}

	pub(super) fn row(
		&self,
		entry: &AgentDeliveryEntry,
		layout: &RowLayout,
		placement: Placement,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let Placement { date, height } = placement;
		let (label, color) = match entry.kind {
			Kind::Requested => ("Planned", ui_theme::TEXT_MUTED),
			Kind::Working => ("In progress", ui_theme::BLUE),
			Kind::Waiting => ("Waiting", ui_theme::TEXT_MUTED),
			Kind::Unconfirmed => ("Check outcome", ui_theme::AMBER),
			Kind::Result => ("Ready", ui_theme::GREEN),
			Kind::Accepted => ("Done", ui_theme::GREEN),
			Kind::NeedsInput => ("Needs you", ui_theme::AMBER),
			Kind::Failed => ("Failed", ui_theme::ERROR),
			Kind::Stopped => ("Stopped", ui_theme::TEXT_MUTED),
		};
		let progress = self
			.feed
			.arrivals
			.get(&entry.id)
			.filter(|_| ui_motion::animations_active())
			.map_or(1., |at| (at.elapsed().as_secs_f32() / ENTRANCE.as_secs_f32()).min(1.));
		let progress = 1. - (1. - progress).powi(3);
		if progress < 1. {
			ui_motion::request_frame(window, cx);
		}
		let id = entry.id;
		let row = gpui::div()
			.id(("delivery", id as u64))
			.debug_selector(move || format!("delivery-{id}"))
			.role(gpui::Role::Group)
			.aria_label(format!("{}: {}", entry.title, label))
			.h(gpui::px(height))
			.w_full()
			.flex()
			.gap(gpui::px(CONTENT_GAP))
			.pt(gpui::px(ROW_TOP))
			.pb(gpui::px(ROW_BOTTOM))
			.child(
				gpui::div().relative().w(gpui::px(RAIL_WIDTH)).h_full().flex_none().child(
					gpui::div()
						.id(gpui::SharedString::from(format!("delivery-marker-{id}")))
						.debug_selector(move || format!("delivery-marker-{id}"))
						.absolute()
						.top(gpui::px(MARKER_Y - ROW_TOP - 3.5))
						.left(gpui::px((RAIL_WIDTH - 7.) / 2.))
						.size(gpui::px(7.))
						.rounded_full()
						.bg(gpui::rgb(color)),
				),
			)
			.child(self.row_body(entry, layout, window, cx));
		let content = gpui::div()
			.w_full()
			.flex()
			.flex_col()
			.when(date, |d| {
				d.child(
					gpui::div()
						.h(gpui::px(DATE_HEIGHT))
						.pl(gpui::px(RAIL_WIDTH + CONTENT_GAP))
						.flex()
						.items_center()
						.text_size(gpui::px(11.))
						.font_weight(gpui::FontWeight::MEDIUM)
						.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
						.child(day_label(entry.updated_at_micros)),
				)
			})
			.child(row);
		gpui::div()
			.h(gpui::px((height + if date { DATE_HEIGHT } else { 0. }) * progress))
			.flex_none()
			.overflow_hidden()
			.opacity(progress)
			.child(content)
			.into_any_element()
	}
}
