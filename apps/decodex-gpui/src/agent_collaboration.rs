//! Source-backed exchanges remain visible when the selected chat changes.
use super::{
	super::{
		super::workspace::restore_shell_focus,
		relations::{Edge, Graph, Kind},
	},
	AgentSurface, Context, Row, ui_theme,
};
use crate::{
	shell::{agent_surface::workspace_size::Panel, workspace_symbols},
	ui_motion::{self, SmoothControl},
	ui_scroll::SmoothScrollArea,
};
use gpui::{AnyElement, SharedString, prelude::*};
use std::collections::BTreeMap;
#[cfg(test)]
#[path = "agent_collaboration_tests.rs"]
mod tests;

struct Message {
	from: String,
	text: String,
}

struct Exchange {
	id: String,
	from: Row,
	to: Row,
	edge: Edge,
	messages: Vec<Message>,
	order: usize,
}

impl Exchange {
	fn state(&self) -> (&'static str, u32) {
		if self.edge.kind == Kind::Dependency
			|| (self.edge.kind == Kind::Wait && self.edge.in_progress)
		{
			("Waiting", ui_theme::AMBER)
		} else if self.edge.in_progress {
			("Working", ui_theme::GREEN)
		} else if self.edge.kind == Kind::Returned {
			("Observed", ui_theme::GREEN)
		} else if self.edge.kind == Kind::Context {
			("Shared", ui_theme::TEXT_MUTED)
		} else if self.edge.kind == Kind::Spawned {
			("Assigned", ui_theme::TEXT_MUTED)
		} else {
			("Sent", ui_theme::TEXT_MUTED)
		}
	}
}

fn exchanges(graph: &Graph, rows: &[Row]) -> Vec<Exchange> {
	let mut cases: BTreeMap<String, Exchange> = BTreeMap::new();
	for (order, edge) in graph.edges.iter().enumerate() {
		if matches!(edge.kind, Kind::Assigned | Kind::Resource) {
			continue;
		}
		if edge.source.is_none() && edge.kind != Kind::Dependency {
			continue;
		}
		let Some(from) = rows.iter().find(|row| row.key == edge.from) else { continue };
		let Some(to) = rows.iter().find(|row| row.key == edge.to) else { continue };
		let id = if from.key < to.key {
			format!("{}|{}", from.key, to.key)
		} else {
			format!("{}|{}", to.key, from.key)
		};
		let case = cases.entry(id.clone()).or_insert_with(|| Exchange {
			id,
			from: from.clone(),
			to: to.clone(),
			edge: edge.clone(),
			messages: vec![],
			order,
		});
		case.order = order;
		case.edge = edge.clone();
		case.from.clone_from(from);
		case.to.clone_from(to);
		if let Some(text) = edge.excerpt.as_ref().filter(|text| !text.trim().is_empty())
			&& !case
				.messages
				.iter()
				.any(|message| message.from == edge.from && &message.text == text)
		{
			case.messages.push(Message { from: edge.from.clone(), text: text.clone() });
		}
	}
	let mut cases: Vec<_> = cases.into_values().collect();
	cases.sort_by_key(|case| {
		(case.state().0 != "Waiting", case.state().0 == "Observed", std::cmp::Reverse(case.order))
	});
	cases
}

impl AgentSurface {
	fn collaboration_participant(
		&self,
		row: &Row,
		case: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let number = self.relation_agent_number(row);
		let label = if row.native { format!("{number} {}", row.owner) } else { number.clone() };
		let tip = format!("{number} · {}", self.relation_agent_name(row));
		let key = row.key.clone();
		let keyboard_key = key.clone();
		let id = format!("collaboration-participant-{case}-{}", row.key);
		gpui::div()
			.id(SharedString::from(id.clone()))
			.debug_selector(move || id.clone())
			.role(gpui::Role::Button)
			.aria_label(label.clone())
			.tab_index(0)
			.min_w_0()
			.max_w(gpui::relative(0.7))
			.whitespace_nowrap()
			.text_ellipsis()
			.overflow_hidden()
			.cursor_pointer()
			.hover(|link| link.text_color(gpui::rgb(ui_theme::BLUE)))
			.tooltip(move |_, cx| {
				cx.new(|_| super::super::relations::RelationTip(tip.clone())).into()
			})
			.on_click(cx.listener(move |s, _, window, cx| {
				s.locate_relation_agent(&key, cx);
				restore_shell_focus(window, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, window, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.locate_relation_agent(&keyboard_key, cx);
					restore_shell_focus(window, cx);
					cx.stop_propagation();
				}
			}))
			.child(label)
			.smooth()
			.into_any_element()
	}

	fn collaboration_exchange_header(&self, case: &Exchange, cx: &mut Context<Self>) -> AnyElement {
		let expanded = self.work_board.collaboration_expanded.as_ref() == Some(&case.id);
		let key = case.id.clone();
		let edge = case.edge.clone();
		let toggle = std::rc::Rc::new(move |surface: &mut Self, cx: &mut Context<Self>| {
			surface.toggle_collaboration_exchange(&key, &edge, cx);
		});
		let keyboard_toggle = toggle.clone();
		let detail = match &case.edge.link {
			Some(link) => format!("{}\n\n{link}", case.edge.detail),
			None => case.edge.detail.clone(),
		};
		let id = SharedString::from(format!("collaboration-{}", case.id));
		let status = if case.messages.is_empty() && case.edge.kind == Kind::Returned {
			case.edge.label.as_str()
		} else {
			case.state().0
		};
		let label = format!(
			"{} {} to {} exchange",
			if expanded { "Collapse" } else { "Expand" },
			self.relation_agent_number(&case.from),
			self.relation_agent_number(&case.to)
		);
		let header = gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(6.))
			.text_size(gpui::px(ui_theme::CAPTION_SIZE))
			.line_height(gpui::px(20.))
			.child(self.collaboration_participant(&case.from, &case.id, cx))
			.child(gpui::div().flex_none().text_color(gpui::rgb(ui_theme::TEXT_MUTED)).child("→"))
			.child(self.collaboration_participant(&case.to, &case.id, cx))
			.child(
				gpui::div()
					.id(id.clone())
					.debug_selector({
						let id = id.to_string();
						move || id.clone()
					})
					.role(gpui::Role::Button)
					.aria_expanded(expanded)
					.aria_label(label)
					.tab_index(0)
					.min_w(gpui::px(24.))
					.h(gpui::px(24.))
					.flex_1()
					.flex()
					.items_center()
					.justify_end()
					.gap(gpui::px(5.))
					.cursor_pointer()
					.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
					.hover(|control| control.text_color(gpui::rgb(ui_theme::TEXT)))
					.tooltip(move |_, cx| {
						cx.new(|_| super::super::relations::RelationTip(detail.clone())).into()
					})
					.child(
						gpui::div()
							.size(gpui::px(4.))
							.rounded_full()
							.flex_none()
							.bg(gpui::rgb(case.state().1)),
					)
					.child(
						gpui::div()
							.min_w_0()
							.overflow_hidden()
							.whitespace_nowrap()
							.text_ellipsis()
							.child(status.to_owned()),
					)
					.child(workspace_symbols::disclosure_chevron(
						SharedString::from(format!("collaboration-chevron-{}", case.id)),
						expanded,
					))
					.on_click(cx.listener(move |s, _, window, cx| {
						toggle(s, cx);
						restore_shell_focus(window, cx);
					}))
					.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, window, cx| {
						if !event.is_held
							&& ["enter", "space"].contains(&event.keystroke.key.as_str())
						{
							keyboard_toggle(s, cx);
							restore_shell_focus(window, cx);
							cx.stop_propagation();
						}
					}))
					.smooth(),
			);
		header.into_any_element()
	}

	fn toggle_collaboration_exchange(&mut self, key: &str, edge: &Edge, cx: &mut Context<Self>) {
		let close = self.work_board.collaboration_expanded.as_deref() == Some(key);
		self.work_board.collaboration_expanded = (!close).then(|| key.to_owned());
		self.work_board.view.edge = (!close).then(|| edge.clone());
		self.work_board.view.keep_camera();
		cx.notify();
	}

	fn collaboration_exchange(&self, case: &Exchange, cx: &mut Context<Self>) -> AnyElement {
		let expanded = self.work_board.collaboration_expanded.as_ref() == Some(&case.id);
		let key = case.id.clone();
		let edge = case.edge.clone();
		let preview_toggle = std::rc::Rc::new(move |surface: &mut Self, cx: &mut Context<Self>| {
			surface.toggle_collaboration_exchange(&key, &edge, cx);
		});
		let preview_keyboard = preview_toggle.clone();
		// Show the actual exchange excerpt. A native assignment is the fallback
		// when the provider recorded an event without a message body.
		let preview = case.messages.last().map_or_else(
			|| {
				let agent = [&case.from, &case.to].into_iter().find(|row| row.native);
				agent.map(|row| self.graph_preview(row).0).unwrap_or_default()
			},
			|message| super::super::activity::text(&message.text),
		);
		let preview_selector = format!("collaboration-preview-{}", case.id);
		let preview = gpui::div()
			.id(SharedString::from(preview_selector.clone()))
			.debug_selector(move || preview_selector.clone())
			.role(gpui::Role::Button)
			.aria_label("Expand exchange contents")
			.tab_index(0)
			.w_full()
			.min_w_0()
			.py(gpui::px(2.))
			.text_size(gpui::px(11.))
			.line_height(gpui::px(16.))
			.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
			.whitespace_normal()
			.line_clamp(2)
			.text_ellipsis()
			.cursor_pointer()
			.hover(|control| control.text_color(gpui::rgb(ui_theme::TEXT)))
			.on_click(cx.listener(move |s, _, window, cx| {
				preview_toggle(s, cx);
				restore_shell_focus(window, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, window, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					preview_keyboard(s, cx);
					restore_shell_focus(window, cx);
					cx.stop_propagation();
				}
			}))
			.child(preview.clone())
			.smooth();
		let mut context = gpui::div()
			.w_full()
			.min_w_0()
			.mt(gpui::px(4.))
			.border_l_1()
			.border_color(gpui::rgba(0xaaa0c440))
			.pl(gpui::px(8.))
			.flex()
			.flex_col()
			.gap(gpui::px(8.));
		for message in &case.messages {
			let author = if message.from == case.from.key { &case.from } else { &case.to };
			context = context.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.flex()
					.flex_col()
					.gap(gpui::px(2.))
					.child(
						gpui::div()
							.text_size(gpui::px(ui_theme::CAPTION_SIZE))
							.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
							.child(self.relation_agent_number(author)),
					)
					.child(
						gpui::div()
							.w_full()
							.min_w_0()
							.text_size(gpui::px(11.))
							.line_height(gpui::px(17.))
							.text_color(gpui::rgb(ui_theme::TEXT))
							.whitespace_normal()
							.child(message.text.clone()),
					),
			);
		}
		if case.messages.is_empty() {
			context =
				context.child(super::note("No reply body recorded.").text_size(gpui::px(11.)));
		}
		let selector = format!("collaboration-row-{}", case.id);
		gpui::div()
			.debug_selector(move || selector.clone())
			.w_full()
			.min_w_0()
			.py(gpui::px(8.))
			.border_b_1()
			.border_color(gpui::rgba(0xffffff12))
			.flex()
			.flex_col()
			.child(self.collaboration_exchange_header(case, cx))
			.child(ui_motion::disclosure(
				SharedString::from(format!("collaboration-preview-reveal-{}", case.id)),
				!expanded,
				preview,
			))
			.child(ui_motion::disclosure(
				SharedString::from(format!("collaboration-context-reveal-{}", case.id)),
				expanded,
				context,
			))
			.into_any_element()
	}

	pub(in super::super::super) fn render_agent_workbench(
		&self,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let graph = self.relation_graph();
		let cases = exchanges(&graph, &self.factory_rows());
		let mut body = gpui::div().w_full().min_w_0().flex().flex_col();
		let entries = self.workbench_entries();
		for entry in entries.iter().filter(|entry| !entry.result) {
			body = body.child(self.workbench_entry(entry, cx));
		}
		for case in &cases {
			body = body.child(self.collaboration_exchange(case, cx));
		}
		if cases.is_empty() && entries.iter().all(|entry| entry.result) {
			body = body.child(super::note("No collaboration recorded.").mt_2());
		}
		gpui::div()
			.id("workbench-body")
			.debug_selector(|| "workbench-body".into())
			.role(gpui::Role::Group)
			.aria_label("Collaboration across chats")
			.size_full()
			.font_weight(gpui::FontWeight::NORMAL)
			.min_h_0()
			.min_w_0()
			.overflow_y_scroll()
			.px(gpui::px(10.))
			.capture_any_mouse_down(
				cx.listener(|s, _, _, _| s.workspace.focused_panel = Some(Panel::Right)),
			)
			.child(body)
			.smooth_scroll("workbench-scroll")
			.into_any_element()
	}
}
