//! Source-backed collaboration across chats, with the existing conversation owners.
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
use decodex_protocol::{
	AgentExchangeDto, AgentExchangeQuestionDto, AgentExchangesResult, EntityId,
};
use gpui::{AnyElement, SharedString, prelude::*};
use std::{collections::BTreeMap, time::Instant};
use ui_theme::foreground;
#[path = "agent_collaboration_cases.rs"] mod cases;
use cases::exchanges;
#[cfg(test)]
#[path = "agent_collaboration_tests.rs"]
mod tests;

fn same_excerpt(a: &str, b: &str) -> bool {
	a.split_whitespace().eq(b.split_whitespace())
}

#[derive(Default)]
pub(in super::super) struct State {
	entries: BTreeMap<i64, AgentExchangeDto>,
	questions: Vec<AgentExchangeQuestionDto>,
	source: Option<EntityId>,
	next_before: Option<i64>,
	page: Option<i64>,
	questions_truncated: bool,
	task: Option<gpui::Task<()>>,
	read_at: Option<Instant>,
	earlier: bool,
	source_open: Option<String>,
	failed: bool,
}
struct Exchange {
	id: String,
	from: Row,
	to: Option<Row>,
	title: String,
	summary: String,
	state: &'static str,
	color: u32,
	request: Option<String>,
	steps: Vec<(&'static str, String)>,
	source: String,
	source_text: String,
	source_row: Row,
	source_event: Option<i64>,
	source_item: Option<String>,
	source_turn: Option<String>,
	source_thread: Option<String>,
	edge: Option<Edge>,
	earlier: bool,
	question: Option<AgentExchangeQuestionDto>,
	order: i64,
}

impl AgentSurface {
	pub(in super::super) fn refresh_collaboration(&mut self, cx: &mut Context<Self>) {
		self.read_collaboration(self.work_board.collaboration.page, false, cx);
	}

	fn read_collaboration(&mut self, before: Option<i64>, explicit: bool, cx: &mut Context<Self>) {
		if !self.command_connection_ready() {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Some(source) =
			self.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.clone())
		else {
			return;
		};
		if self.work_board.collaboration.source.as_ref() != Some(&source) {
			self.work_board.collaboration =
				State { source: Some(source.clone()), ..Default::default() };
		}
		let state = &mut self.work_board.collaboration;
		if state.task.is_some()
			|| (!explicit && state.read_at.is_some_and(|at| at.elapsed().as_secs() < 2))
		{
			return;
		}
		state.read_at = Some(Instant::now());
		if explicit {
			state.page = before;
		}
		let epoch = self.command_epoch;
		let read = cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| ())?;
			runtime
				.block_on(super::super::super::AgentClient::new(profile).exchanges(before))
				.map_err(|_| ())
		});
		self.work_board.collaboration.task = Some(cx.spawn(async move |surface, cx| {
			let result = read.await;
			let _ = surface.update(cx, |s, cx| {
				if s.command_epoch != epoch
					|| s.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.as_ref())
						!= Some(&source)
				{
					return;
				}
				let state = &mut s.work_board.collaboration;
				state.task = None;
				let changed = match result {
					Ok(AgentExchangesResult::Available {
						source: observed,
						entries,
						questions,
						questions_truncated,
						next_before,
					}) if observed == source => {
						let entries =
							entries.into_iter().map(|entry| (entry.event_id, entry)).collect();
						let changed = state.failed
							|| state.entries != entries
							|| state.questions != questions
							|| state.questions_truncated != questions_truncated
							|| state.next_before != next_before
							|| explicit;
						state.failed = false;
						// Refresh the selected page; an old assessment must not stay cached as
						// current.
						state.entries = entries;
						state.questions = questions;
						state.questions_truncated = questions_truncated;
						state.next_before = next_before;
						changed
					},
					_ => {
						let changed = !state.failed;
						state.failed = true;
						changed
					},
				};
				if changed {
					cx.notify();
				}
			});
		}));
	}

	fn collaboration_cases(&self, graph: &Graph, rows: &[Row]) -> Vec<Exchange> {
		let state = &self.work_board.collaboration;
		let rebound = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| snapshot.runtime_source.as_ref())
			.is_some_and(|source| state.source.as_ref() != Some(source));
		let records: Vec<_> =
			if rebound { vec![] } else { state.entries.values().cloned().collect() };
		let questions = if rebound { &[][..] } else { state.questions.as_slice() };
		let mut cases = exchanges(graph, rows, &records, questions);
		for row in rows.iter().filter(|row| row.native && row.group != 1) {
			let Some(brief) = self.work_board.briefs.get(&row.key).filter(|brief| {
				brief.source_thread == row.thread && brief.stamp == self.brief_stamp(row)
			}) else {
				continue;
			};
			let (Some(output), Some((turn, status))) = (&brief.output, &brief.outcome) else {
				continue;
			};
			if output.turn != *turn
				|| status != "completed"
				|| brief.metrics.stats.is_some_and(|stats| stats.active_ms.is_some())
			{
				continue;
			}
			let Some(thread) = row.thread.as_ref() else {
				continue;
			};
			let Some(parent) = self
				.native_agents
				.lists
				.get(&row.work)
				.and_then(|agents| agents.iter().find(|agent| &agent.thread_id == thread))
				.and_then(|agent| {
					rows.iter()
						.find(|parent| parent.thread.as_ref() == Some(&agent.parent_thread_id))
				})
			else {
				continue;
			};
			// This is the child's reported output, not a reconstructed reply to a parent call.
			// Compact parent notifications remain separate source records in Earlier.
			for case in &mut cases {
				if case
					.edge
					.as_ref()
					.is_some_and(|edge| edge.kind == Kind::Returned && edge.from == row.key)
				{
					case.earlier = true;
				}
			}
			let mut source_text: String = output.text.chars().take(640).collect();
			if output.truncated || output.text.chars().count() > 640 {
				source_text.push('…');
			}
			cases.push(Exchange {
				id: format!("native-report:{thread}:{turn}"),
				from: row.clone(),
				to: Some(parent.clone()),
				title: brief
					.preview
					.as_ref()
					.filter(|preview| preview.turn == *turn && !preview.task.is_empty())
					.map(|preview| preview.task.clone())
					.or_else(|| self.native_assignment(row))
					.unwrap_or_else(|| "Reported output".into()),
				summary: super::super::activity::text(&output.text).chars().take(180).collect(),
				state: "Reported",
				color: ui_theme::BLUE,
				request: None,
				steps: vec![("Review", "Acceptance and reply linkage are unknown.".into())],
				source: "Native report".into(),
				source_text,
				source_row: row.clone(),
				source_event: None,
				source_item: None,
				source_turn: Some(turn.clone()),
				source_thread: Some(thread.clone()),
				edge: None,
				earlier: false,
				question: None,
				order: 0,
			});
		}
		cases
	}

	fn collaboration_action(
		&self,
		id: String,
		label: String,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		gpui::div()
			.id(SharedString::from(id.clone()))
			.debug_selector(move || id.clone())
			.role(gpui::Role::Button)
			.aria_label(label.clone())
			.tab_index(0)
			.min_w_0()
			.min_h(gpui::px(22.))
			.flex()
			.items_center()
			.text_size(gpui::px(foreground::METRIC_SIZE))
			.line_height(gpui::px(18.))
			.font_weight(gpui::FontWeight::MEDIUM)
			.text_color(crate::ui_theme::rgb(foreground::SECONDARY))
			.cursor_pointer()
			.hover(|style| style.text_color(crate::ui_theme::rgb(ui_theme::BLUE)))
			.on_click(cx.listener(move |s, _, window, cx| {
				action(s, cx);
				restore_shell_focus(window, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, window, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					keyboard(s, cx);
					restore_shell_focus(window, cx);
					cx.stop_propagation();
				}
			}))
			.child(label)
			.smooth()
			.into_any_element()
	}

	fn collaboration_participant(
		&self,
		row: &Row,
		case: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let number = self.relation_agent_number(row);
		let label = format!("{number} {}", self.relation_agent_name(row));
		let key = row.key.clone();
		let keyboard = key.clone();
		let id = format!("collaboration-participant-{case}-{}", row.key);
		gpui::div()
			.id(SharedString::from(id.clone()))
			.debug_selector(move || id.clone())
			.role(gpui::Role::Button)
			.aria_label(format!("Locate {label} in Graph"))
			.tab_index(0)
			.min_w_0()
			.max_w(gpui::relative(0.6))
			.whitespace_nowrap()
			.text_ellipsis()
			.overflow_hidden()
			.cursor_pointer()
			.hover(|link| link.text_color(crate::ui_theme::rgb(ui_theme::BLUE)))
			.on_click(cx.listener(move |s, _, window, cx| {
				s.locate_relation_agent(&key, cx);
				restore_shell_focus(window, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, window, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.locate_relation_agent(&keyboard, cx);
					restore_shell_focus(window, cx);
					cx.stop_propagation();
				}
			}))
			.child(number)
			.smooth()
			.into_any_element()
	}

	fn collaboration_source_field(
		&self,
		case: &Exchange,
		label: &'static str,
		value: &str,
	) -> AnyElement {
		let key = format!("collaboration-reference-{}-{label}", case.id);
		gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(8.))
			.child(gpui::div().w(gpui::px(40.)).flex_none().child(label))
			.child(
				gpui::div()
					.debug_selector({
						let key = key.clone();
						move || key.clone()
					})
					.flex_1()
					.min_w_0()
					.whitespace_nowrap()
					.text_ellipsis()
					.overflow_hidden()
					.child(value.to_owned()),
			)
			.child(crate::shell::agent_surface::markdown::copy_button(
				&format!("copy-{key}"),
				match label {
					"Thread" => "Copy thread ID",
					"Turn" => "Copy turn ID",
					"Item" => "Copy item ID",
					"Event" => "Copy event ID",
					_ => "Copy source ID",
				},
				value.to_owned(),
			))
			.into_any_element()
	}

	fn toggle_collaboration_exchange(
		&mut self,
		key: &str,
		edge: Option<&Edge>,
		cx: &mut Context<Self>,
	) {
		let close = self.work_board.collaboration_expanded.as_deref() == Some(key);
		self.work_board.collaboration_expanded = (!close).then(|| key.to_owned());
		self.work_board.view.edge = if close { None } else { edge.cloned() };
		self.work_board.view.keep_camera();
		cx.notify();
	}

	fn collaboration_exchange_header(&self, case: &Exchange, cx: &mut Context<Self>) -> AnyElement {
		let expanded = self.work_board.collaboration_expanded.as_ref() == Some(&case.id);
		let source_open =
			expanded && self.work_board.collaboration.source_open.as_ref() == Some(&case.id);
		let key = case.id.clone();
		let edge = case.edge.clone();
		let keyboard_key = key.clone();
		let keyboard_edge = edge.clone();
		let selector = format!("collaboration-{}", case.id);
		let mut route = gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(5.))
			.text_size(gpui::px(foreground::CAPTION_SIZE))
			.line_height(gpui::px(19.))
			.font_weight(gpui::FontWeight::MEDIUM)
			.text_color(crate::ui_theme::rgb(foreground::SECONDARY))
			.child(
				gpui::div()
					.flex_none()
					.size(gpui::px(4.))
					.rounded_full()
					.bg(crate::ui_theme::rgb(case.color)),
			)
			.child(self.collaboration_participant(&case.from, &case.id, cx))
			.child(
				gpui::div()
					.flex_none()
					.text_color(crate::ui_theme::rgb(foreground::SECONDARY))
					.child("→"),
			);
		route = if let Some(to) = &case.to {
			route.child(self.collaboration_participant(to, &case.id, cx))
		} else {
			route.child(if case.question.is_some() { "You" } else { "Unknown" })
		};
		route = route.child(gpui::div().flex_1()).child(
			gpui::div()
				.flex_none()
				.text_color(crate::ui_theme::rgb(
					if matches!(case.state, "Reported" | "Recorded") {
						foreground::SECONDARY
					} else {
						case.color
					},
				))
				.child(case.state),
		);
		let title = gpui::div()
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector.clone())
			.role(gpui::Role::Button)
			.aria_expanded(expanded)
			.aria_label(format!("{} {}", if expanded { "Collapse" } else { "Expand" }, case.title))
			.tab_index(0)
			.w_full()
			.min_w_0()
			.cursor_pointer()
			.py(gpui::px(3.))
			.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.flex()
					.items_start()
					.gap(gpui::px(5.))
					.child(
						gpui::div()
							.flex_1()
							.min_w_0()
							.text_size(gpui::px(12.))
							.line_height(gpui::px(19.))
							.font_weight(gpui::FontWeight::NORMAL)
							.text_color(crate::ui_theme::rgb(foreground::PRIMARY))
							.line_clamp(if expanded { 2 } else { 1 })
							.text_ellipsis()
							.child(case.title.clone()),
					)
					.child(workspace_symbols::disclosure_chevron(
						SharedString::from(format!("collaboration-chevron-{}", case.id)),
						expanded,
					)),
			)
			.when(!source_open || !same_excerpt(&case.summary, &case.source_text), |header| {
				header.child(
					gpui::div()
						.text_size(gpui::px(11.5))
						.line_height(gpui::px(ui_theme::BODY_LINE_HEIGHT))
						.text_color(crate::ui_theme::rgb(foreground::BODY))
						.line_clamp(
							if expanded
								|| !matches!(
									case.state,
									"Reported" | "Recorded" | "Shared" | "Accepted"
								) {
								2
							} else {
								1
							},
						)
						.text_ellipsis()
						.child(case.summary.clone()),
				)
			})
			.on_click(cx.listener(move |s, _, window, cx| {
				s.toggle_collaboration_exchange(&key, edge.as_ref(), cx);
				restore_shell_focus(window, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, window, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.toggle_collaboration_exchange(&keyboard_key, keyboard_edge.as_ref(), cx);
					restore_shell_focus(window, cx);
					cx.stop_propagation();
				}
			}))
			.smooth();
		gpui::div().w_full().min_w_0().child(route).child(title).into_any_element()
	}

	fn collaboration_exchange(&self, case: &Exchange, cx: &mut Context<Self>) -> AnyElement {
		let expanded = self.work_board.collaboration_expanded.as_ref() == Some(&case.id);
		let source_open = self.work_board.collaboration.source_open.as_ref() == Some(&case.id);
		let related = self.work_board.view.picked_agent.as_ref().is_some_and(|key| {
			case.from.key == *key || case.to.as_ref().is_some_and(|row| row.key == *key)
		});
		let mut context =
			gpui::div().w_full().min_w_0().flex().flex_col().gap(gpui::px(7.)).pt(gpui::px(4.));
		if let Some(request) = case.request.as_ref().filter(|request| {
			!same_excerpt(request, &case.summary)
				&& !(source_open && same_excerpt(request, &case.source_text))
		}) {
			context = context.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.border_l_1()
					.border_color(crate::ui_theme::rgba(0xaaa0c450))
					.pl(gpui::px(8.))
					.child(
						gpui::div()
							.text_size(gpui::px(foreground::CAPTION_SIZE))
							.line_height(gpui::px(15.))
							.font_weight(gpui::FontWeight::MEDIUM)
							.text_color(crate::ui_theme::rgb(foreground::SECONDARY))
							.child("Request"),
					)
					.child(
						gpui::div()
							.text_size(gpui::px(11.5))
							.line_height(gpui::px(17.))
							.text_color(crate::ui_theme::rgb(foreground::BODY))
							.line_clamp(3)
							.text_ellipsis()
							.child(request.clone()),
					),
			);
		}
		if let Some(question) = &case.question {
			context = context.child(ui_motion::disclosure(
				SharedString::from(format!("collaboration-question-{}", case.id)),
				question.pending,
				gpui::div()
					.text_size(gpui::px(11.5))
					.line_height(gpui::px(17.))
					.border_l_1()
					.border_color(crate::ui_theme::rgba(0xd4b27c60))
					.pl(gpui::px(8.))
					.child(question.question.title.clone()),
			));
		}
		let mut source_notes = Vec::new();
		for (label, fact) in &case.steps {
			let note = gpui::div()
				.w_full()
				.min_w_0()
				.flex()
				.items_start()
				.gap(gpui::px(8.))
				.child(
					gpui::div()
						.w(gpui::px(46.))
						.flex_none()
						.text_size(gpui::px(foreground::CAPTION_SIZE))
						.line_height(gpui::px(18.))
						.font_weight(gpui::FontWeight::MEDIUM)
						.text_color(crate::ui_theme::rgb(foreground::SECONDARY))
						.child(*label),
				)
				.child(
					gpui::div()
						.flex_1()
						.min_w_0()
						.text_size(gpui::px(11.5))
						.line_height(gpui::px(18.))
						.text_color(crate::ui_theme::rgb(foreground::BODY))
						.line_clamp(3)
						.text_ellipsis()
						.child(fact.clone()),
				);
			if matches!(case.state, "Reported" | "Recorded" | "Shared") {
				source_notes.push(note.into_any_element());
			} else {
				context = context.child(note);
			}
		}
		let key = case.id.clone();
		let source_key = key.clone();
		let mut actions = gpui::div()
			.w_full()
			.flex()
			.flex_wrap()
			.items_center()
			.justify_between()
			.gap(gpui::px(8.))
			.child(self.collaboration_action(
				format!("collaboration-source-{key}"),
				"Source".into(),
				move |s, cx| {
					let state = &mut s.work_board.collaboration;
					state.source_open = if state.source_open.as_ref() == Some(&source_key) {
						None
					} else {
						Some(source_key.clone())
					};
					cx.notify();
				},
				cx,
			));
		let recipient = if case.state == "Not sent" {
			Some(&case.from)
		} else if case.state == "To review"
			|| case.state == "Check result"
			|| case.state == "Decision"
			|| case.state == "Changes asked"
		{
			case.to.as_ref()
		} else {
			Some(&case.source_row)
		};
		if let Some(row) = recipient {
			let row = row.clone();
			let label = if case.question.as_ref().is_some_and(|question| question.pending) {
				"Open question".into()
			} else {
				format!("Open {}", self.relation_agent_number(&row))
			};
			actions = actions.child(self.collaboration_action(
				format!("collaboration-open-{key}"),
				label,
				move |s, cx| {
					s.open_graph_agent(&row, cx);
				},
				cx,
			));
		}
		context = context.child(actions);
		let mut source = gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.flex_col()
			.gap(gpui::px(4.))
			.text_size(gpui::px(foreground::CAPTION_SIZE))
			.line_height(gpui::px(17.))
			.text_color(crate::ui_theme::rgb(foreground::SECONDARY))
			.children(source_notes);
		source = if matches!(case.source.as_str(), "Native report" | "Native question") {
			source.child(gpui::div().child(case.source.clone()))
		} else {
			source.child(self.collaboration_source_field(case, "Source", &case.source))
		};
		if let Some(event) = case.source_event {
			source =
				source.child(self.collaboration_source_field(case, "Event", &event.to_string()));
		}
		if let Some(thread) = &case.source_thread {
			source = source.child(self.collaboration_source_field(case, "Thread", thread));
		}
		if let Some(turn) = &case.source_turn {
			source = source.child(self.collaboration_source_field(case, "Turn", turn));
		}
		if let Some(item) = &case.source_item {
			source = source.child(self.collaboration_source_field(case, "Item", item));
		}
		source = source.child(
			gpui::div()
				.text_color(crate::ui_theme::rgb(foreground::BODY))
				.child(case.source_text.clone()),
		);
		if case.source_thread.is_some() && case.source_thread == case.source_row.thread {
			let row = case.source_row.clone();
			source = source.child(self.collaboration_action(
				format!("collaboration-original-{}", case.id),
				"Open original conversation".into(),
				move |s, cx| s.open_graph_agent(&row, cx),
				cx,
			));
		}
		context = context.child(ui_motion::disclosure(
			SharedString::from(format!("collaboration-source-reveal-{}", case.id)),
			source_open,
			source,
		));
		let selector = format!("collaboration-row-{}", case.id);
		gpui::div()
			.id(SharedString::from(selector.clone()))
			.debug_selector(move || selector.clone())
			.w_full()
			.min_w_0()
			.flex_none()
			.relative()
			.py(gpui::px(12.))
			.border_b_1()
			.border_color(crate::ui_theme::rgba(0xffffff12))
			.when(related, |row| {
				row.child(
					gpui::div()
						.debug_selector(|| "collaboration-selection-mark".into())
						.absolute()
						.left(gpui::px(-7.))
						.top(gpui::px(12.))
						.bottom(gpui::px(12.))
						.border_l_1()
						.border_color(crate::ui_theme::rgb(ui_theme::BLUE)),
				)
			})
			.child(self.collaboration_exchange_header(case, cx))
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
		let rows = self.factory_rows();
		let cases = self.collaboration_cases(&graph, &rows);
		let mut body = gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.flex_col()
			.text_size(gpui::px(11.))
			.line_height(gpui::px(17.))
			.text_color(crate::ui_theme::rgb(foreground::SECONDARY));
		let entries = self.workbench_entries();
		for entry in entries.iter().filter(|entry| {
			!entry.result
				&& (entry.request.is_some()
					|| !cases.iter().any(|case| {
						case.question.as_ref().is_some_and(|question| {
							question.pending && question.work_id == entry.row.work
						})
					}))
		}) {
			body = body.child(self.workbench_entry(entry, cx));
		}
		for case in cases.iter().filter(|case| {
			!case.earlier || self.work_board.collaboration_expanded.as_ref() == Some(&case.id)
		}) {
			body = body.child(self.collaboration_exchange(case, cx));
		}
		let earlier: Vec<_> = cases
			.iter()
			.filter(|case| {
				case.earlier && self.work_board.collaboration_expanded.as_ref() != Some(&case.id)
			})
			.collect();
		if !earlier.is_empty() {
			let open = self.work_board.collaboration.earlier;
			body = body.child(self.collaboration_action(
				"collaboration-earlier".into(),
				format!("Earlier · {}", earlier.len()),
				|s, cx| {
					s.work_board.collaboration.earlier = !s.work_board.collaboration.earlier;
					cx.notify();
				},
				cx,
			));
			let mut history = gpui::div().w_full().min_w_0().flex().flex_col();
			for case in earlier {
				history = history.child(self.collaboration_exchange(case, cx));
			}
			body = body.child(ui_motion::disclosure("collaboration-earlier-reveal", open, history));
		}
		if self.work_board.collaboration.page.is_some() {
			body = body.child(self.collaboration_action(
				"collaboration-latest".into(),
				"Back to latest".into(),
				|s, cx| s.read_collaboration(None, true, cx),
				cx,
			));
		}
		if let Some(before) = self.work_board.collaboration.next_before {
			body = body.child(self.collaboration_action(
				"collaboration-older".into(),
				"Earlier sources".into(),
				move |s, cx| s.read_collaboration(Some(before), true, cx),
				cx,
			));
		}
		if self.work_board.collaboration.questions_truncated {
			body = body
				.child(super::note("Older questions remain in their source conversations.").mt_2());
		}
		if self.work_board.collaboration.failed {
			body = body.child(
				super::note("Could not refresh collaboration. Last observations retained.").mt_2(),
			);
		} else if cases.is_empty() && entries.iter().all(|entry| entry.result) {
			body = body.child(super::note("No exchanges recorded.").mt_2());
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
			.px(gpui::px(14.))
			.capture_any_mouse_down(
				cx.listener(|s, _, _, _| s.workspace.focused_panel = Some(Panel::Right)),
			)
			.child(body)
			.smooth_scroll("workbench-scroll")
			.into_any_element()
	}
}
