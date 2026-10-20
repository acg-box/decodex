//! The right workbench owns attention and intervention, using existing conversation state.
use super::{AgentSurface, Context, Row};
use crate::{
	shell::agent_surface::{ui_theme, workspace_size::Panel},
	ui_scroll::SmoothScrollArea,
};
use gpui::{
	AnyElement, IntoElement,
	prelude::{
		FluentBuilder, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled,
	},
};

struct Entry {
	row: Row,
	label: String,
	reason: String,
	result: bool,
	request: Option<i64>,
}

#[cfg(test)]
#[path = "agent_workbench_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "agent_workbench_wire_tests.rs"]
mod wire_tests;

fn section(title: impl Into<gpui::SharedString>) -> gpui::Div {
	gpui::div()
		.mt(gpui::px(22.))
		.mb(gpui::px(10.))
		.text_size(gpui::px(11.))
		.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
		.child(title.into())
}

fn note(text: impl Into<gpui::SharedString>) -> gpui::Div {
	gpui::div()
		.text_size(gpui::px(12.))
		.line_height(gpui::px(18.))
		.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
		.child(text.into())
}

impl AgentSurface {
	#[cfg(any(test, feature = "visual-capture"))]
	pub(in super::super) fn visual_workbench_page(&mut self, page: &str, cx: &mut Context<Self>) {
		if let Some(snapshot) = &mut self.snapshot {
			for work in snapshot
				.work_items
				.iter_mut()
				.filter(|work| ["agent", "release"].contains(&work.id.as_str()))
			{
				work.kind = super::super::AgentWorkKindDto::Manager;
			}
		}
		self.work_board.graph = false;
		self.workspace.graph_expanded = page != "workbench-split";
		self.workspace.agent_tree_visible = true;
		self.workspace.agent_panel_width = 320.;
		self.handoffs.fixture();
		self.visual_relation_evidence();
		self.visual_workbench_metrics();
		if matches!(page, "workbench-agent" | "workbench-split") {
			self.select_workbench_agent("release", cx);
		}
		if page == "workbench-history" {
			self.work_board.workbench_history_expanded = true;
		}
		cx.notify();
	}

	#[cfg(any(test, feature = "visual-capture"))]
	fn visual_workbench_metrics(&mut self) {
		use decodex_protocol::{AgentOutputRate, AgentThreadMetrics};
		let rows = self.factory_rows();
		let mut usage = std::collections::BTreeMap::new();
		for (index, row) in rows.into_iter().enumerate() {
			self.snapshot
				.as_mut()
				.expect("visual snapshot")
				.agent_slots
				.insert(row.key.clone(), index as u32);
			let generating = row.group == 1 && index % 2 == 0;
			let preview = Some(super::activity::Preview {
				turn: "fixture-operation".into(),
				task: row.title.clone(),
				text: match row.key.as_str() {
					"agent" => "Reviewing release blockers",
					"release" => "Waiting for your release decision",
					"flow" => "Regression checks complete",
					"verify" => "Running compatibility tests",
					"trace" => "Startup trace complete",
					"improve" => "Comparing launch profiles",
					"impact" => "Waiting for startup results",
					"ready" => "Waiting for verification",
					_ => "Checking cancellation behavior",
				}
				.into(),
				running: None,
			});
			let sample = crate::ui_work_metrics::Sample {
				tokens: Some(18_400 + index as u64 * 7_230),
				stats: Some(AgentThreadMetrics {
					completed_ms: Some(312_000 + index as u64 * 21_000),
					active_ms: (row.group == 1).then_some(8_000),
					generating,
					rate: Some(AgentOutputRate {
						estimated: true,
						active: generating,
						output_tokens: 220 + index as u64 * 4,
						duration_ms: 10_000,
						started_at_ms: 100_000,
						completed_at_ms: (!generating).then_some(110_000),
					}),
				}),
				read_at: None,
			};
			let output = (row.group == 2).then(|| super::activity::Output {
				truncated: false,
				turn: "fixture-operation".into(),
				text: match row.key.as_str() {
					"flow" =>
						"Regression checks passed. Launch state and saved drafts are preserved.",
					"trace" =>
						"Startup trace is ready. Native history recovery takes most of the time.",
					_ => "Verification is complete. The result is ready for review.",
				}
				.into(),
			});
			if let Some(brief) = self.work_board.briefs.get_mut(&row.key) {
				brief.metrics = sample;
				brief.preview = preview;
				brief.output = output;
			} else {
				self.work_board.briefs.insert(
					row.key.clone(),
					super::Brief {
						output,
						source_thread: row.thread.clone(),
						assignments: Default::default(),
						outcome: None,
						preview,
						question: None,
						resources: None,
						relations: vec![],
						metrics: sample,
						stamp: self.brief_stamp(&row),
						read_at: std::time::Instant::now(),
					},
				);
			}
			usage.insert(row.key, (row.thread, sample));
		}
		self.work_board.usage = Some(usage);
	}

	fn workbench_row(&self) -> Option<Row> {
		let key = self.work_board.view.picked_agent.as_ref()?;
		self.board_rows().into_iter().find(|row| &row.key == key)
	}

	fn workbench_row_is_recipient(&self, row: &Row) -> bool {
		if let Some((owner, thread)) = &self.native_agents.selected {
			owner == &row.work && row.thread.as_ref() == Some(thread)
		} else {
			!row.native && self.selected.as_ref() == Some(&row.work) && self.selected_is_manager()
		}
	}

	#[cfg(test)]
	pub(in super::super) fn workbench_has_recipient(&self) -> bool {
		self.workbench_row().is_some_and(|row| self.workbench_row_is_recipient(&row))
	}

	pub(in super::super) fn graph_has_composer(&self) -> bool {
		(self.native_agents.selected.is_some() || self.selected_is_manager())
			&& !self.selected_is_archived()
			&& (self.conversation_work().is_some() || self.is_new_conversation())
	}

	pub(in super::super) fn select_workbench_agent(&mut self, key: &str, cx: &mut Context<Self>) {
		if !self.board_rows().iter().any(|row| row.key == key) {
			return;
		}
		self.work_board.view.keep_camera();
		// Inspection alone is not a read receipt. Open conversation owns that transition.
		self.handoffs.read_on_open = None;
		self.workspace.agent_tree_visible = true;
		self.work_board.view.picked_agent = Some(key.into());
		self.work_board.view.edge = None;
		self.work_board.focus = Some(key.into());
		self.handoffs.focus = None;
		cx.notify();
	}

	pub(in super::super) fn clear_workbench_agent(&mut self, cx: &mut Context<Self>) {
		self.work_board.view.picked_agent = None;
		self.work_board.focus = None;
		self.handoffs.focus = None;
		cx.notify();
	}

	fn workbench_requests(&self, rows: &[Row]) -> Vec<Entry> {
		self.snapshot
			.iter()
			.flat_map(|snapshot| &snapshot.pending_events)
			.filter(|event| {
				["permission_pending", "user_input_pending", "server_request_pending"]
					.contains(&event.event_kind.as_str())
			})
			.filter_map(|event| {
				let row = rows.iter().find(|row| !row.native && row.work == event.work_item_id)?;
				Some(Entry {
					row: row.clone(),
					label: if event.event_kind == "permission_pending" {
						"Permission needed"
					} else {
						"Answer needed"
					}
					.into(),
					reason: "This agent needs your response.".into(),
					result: false,
					request: Some(event.id),
				})
			})
			.collect()
	}

	fn workbench_entries(&self) -> Vec<Entry> {
		let rows = self.board_rows();
		let mut entries = self.workbench_requests(&rows);
		for item in self.handoff_items() {
			if entries.iter().any(|entry| entry.row.work == item.work && entry.request.is_some()) {
				continue;
			}
			if let Some(row) = rows.iter().find(|row| !row.native && row.work == item.work) {
				entries.push(Entry {
					row: row.clone(),
					label: item.label.into(),
					reason: item.reason.into(),
					result: item.result,
					request: None,
				});
			}
		}
		for row in &rows {
			if entries.iter().any(|entry| entry.row.key == row.key && entry.request.is_some()) {
				continue;
			}
			if let Some(question) = self.graph_question(row) {
				if let Some(entry) = entries.iter_mut().find(|entry| entry.row.key == row.key) {
					entry.label = "Answer needed".into();
					entry.reason = question.into();
					entry.result = false;
					continue;
				}
				entries.push(Entry {
					row: row.clone(),
					label: "Answer needed".into(),
					reason: question.into(),
					result: false,
					request: None,
				});
			}
		}
		entries
	}

	fn workbench_entry(&self, entry: &Entry, cx: &mut Context<Self>) -> AnyElement {
		let key = entry.row.key.clone();
		let request = entry.request;
		let action_key = request.map_or_else(
			|| format!("workbench-item-{key}"),
			|event| format!("workbench-item-request-{event}"),
		);
		let (task, _) = self.graph_preview(&entry.row);
		gpui::div()
			.flex_none()
			.w_full()
			.min_w_0()
			.pb_3()
			.mb_3()
			.border_b_1()
			.border_color(gpui::rgba(ui_theme::HOVER_FILL))
			.child(note(entry.label.clone()))
			.child(gpui::div().mt_1().child(if task.is_empty() {
				entry.row.title.clone()
			} else {
				task
			}))
			.child(note(entry.reason.clone()).mt_1().mb_2())
			.child(
				self.workspace_action(
					action_key,
					if entry.result {
						"Review result"
					} else if entry.row.native {
						"Open conversation"
					} else {
						"Review request"
					}
					.into(),
					move |s, cx| {
						let Some(row) = s.board_rows().into_iter().find(|row| row.key == key)
						else {
							return;
						};
						if let Some(event) = request {
							if !s.snapshot.as_ref().is_some_and(|snapshot| {
								snapshot.pending_events.iter().any(|pending| {
									pending.id == event && pending.work_item_id == row.work
								})
							}) {
								return;
							}
							s.workspace.graph_expanded = false;
							s.open_page(&row.work, cx);
							s.request = None;
							s.request_task = None;
							s.load_request(event, cx);
						} else {
							s.open_graph_agent(&row, cx);
						}
					},
					cx,
				),
			)
			.into_any_element()
	}

	fn workbench_agent_row(&self, row: &Row, cx: &mut Context<Self>) -> AnyElement {
		let key = row.key.clone();
		let keyboard = key.clone();
		let (task, _) = self.graph_preview(row);
		let (activity_label, activity) = self.work_activity_summary(row);
		let name = if row.native { &row.owner } else { &row.title };
		gpui::div()
			.id(gpui::SharedString::from(format!("workbench-agent-{key}")))
			.role(gpui::Role::Button)
			.aria_label(format!("{name}, {}, {activity_label}: {activity}", row.status))
			.tab_index(0)
			.w_full()
			.min_w_0()
			.py_3()
			.border_b_1()
			.border_color(gpui::rgba(ui_theme::HOVER_FILL))
			.cursor_pointer()
			.hover(|row| row.bg(gpui::rgba(ui_theme::HOVER_FILL)))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap_2()
					.child(
						gpui::div()
							.size(gpui::px(6.))
							.flex_none()
							.rounded_full()
							.bg(gpui::rgb(row.color)),
					)
					.child(
						gpui::div()
							.flex_1()
							.min_w_0()
							.whitespace_normal()
							.line_clamp(2)
							.text_ellipsis()
							.text_size(gpui::px(11.))
							.line_height(gpui::px(15.))
							.child(name.clone()),
					),
			)
			.when(row.native && task != *name && !task.is_empty(), |view| {
				view.child(
					note(task)
						.text_size(gpui::px(10.))
						.ml(gpui::px(14.))
						.mt_1()
						.line_clamp(2)
						.text_ellipsis(),
				)
			})
			.child(
				gpui::div()
					.ml(gpui::px(14.))
					.mt_2()
					.child(note(activity_label).text_size(gpui::px(9.)).line_height(gpui::px(13.)))
					.when(!activity.is_empty(), |view| {
						view.child(
							gpui::div()
								.debug_selector(|| "workbench-operation".into())
								.mt_1()
								.text_size(gpui::px(11.))
								.line_height(gpui::px(15.))
								.whitespace_normal()
								.line_clamp(3)
								.text_ellipsis()
								.child(activity),
						)
					}),
			)
			.when(row.group == 0, |view| {
				view.child(
					note("Open to respond →").text_size(gpui::px(10.)).ml(gpui::px(14.)).mt_2(),
				)
			})
			.on_click(cx.listener(move |s, _, _, cx| s.select_workbench_agent(&key, cx)))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.select_workbench_agent(&keyboard, cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn workbench_overview(&self, cx: &mut Context<Self>) -> AnyElement {
		let entries = self.workbench_entries();
		let mut body = gpui::div().w_full().min_w_0().flex().flex_col();
		if !entries.is_empty() {
			body = body.child(section("Needs attention"));
			for entry in &entries {
				body = body.child(self.workbench_entry(entry, cx));
			}
		}
		let mut rows = self.factory_rows();
		rows.sort_by_key(|row| {
			(
				row.group,
				std::cmp::Reverse(
					self.snapshot
						.as_ref()
						.and_then(|snapshot| {
							snapshot.work_items.iter().find(|work| work.id == row.work)
						})
						.map(|work| work.updated_at_micros),
				),
				row.key.clone(),
			)
		});
		for row in &rows {
			body = body.child(self.workbench_agent_row(row, cx));
		}
		if rows.is_empty() {
			body = body.child(note("No work is active."));
		}
		body.into_any_element()
	}

	fn workbench_materials(&self, row: &Row, cx: &mut Context<Self>) -> AnyElement {
		let mut body =
			gpui::div().flex().flex_col().min_w_0().child(section("Materials & evidence"));
		let resources = self
			.work_board
			.briefs
			.get(&row.key)
			.filter(|brief| {
				brief.source_thread == row.thread && brief.stamp == self.brief_stamp(row)
			})
			.and_then(|brief| brief.resources.as_ref());
		let has_context = self.snapshot.as_ref().is_some_and(|snapshot| {
			snapshot
				.context_references
				.iter()
				.any(|reference| !row.native && reference.recipient_work_id == row.work)
		});
		if resources.is_none_or(Vec::is_empty) && !has_context {
			return gpui::div().into_any_element();
		}

		if let Some(resources) = resources.filter(|resources| !resources.is_empty()) {
			for resource in resources {
				body = body.child(
					note(format!("{} · {}", resource.attachment_type, resource.identity_key))
						.mb_2(),
				);
			}
			if !row.native {
				let key = row.key.clone();
				body = body.child(self.workspace_action(
					"workbench-open-materials".into(),
					"Manage resources".into(),
					move |s, cx| {
						if let Some(row) = s.board_rows().into_iter().find(|row| row.key == key) {
							s.open_graph_agent(&row, cx);
							s.toggle_resources(&row.work, cx);
							s.clear_workbench_agent(cx);
						}
					},
					cx,
				));
			}
		}
		if let Some(snapshot) = &self.snapshot {
			for reference in snapshot
				.context_references
				.iter()
				.filter(|reference| !row.native && reference.recipient_work_id == row.work)
			{
				let work = reference.source_work_id.clone();
				let thread = reference.source_thread_id.clone();
				let key = format!("workbench-context-{}", reference.event_id);
				let label = snapshot
					.work_items
					.iter()
					.find(|item| item.id == work)
					.map(|work| self.work_label(work))
					.unwrap_or_else(|| work.clone());
				body = body
					.child(note(format!(
						"Context from {label} · {}",
						if reference.delivery_turn_id.is_some() { "Delivered" } else { "Queued" }
					)))
					.child(self.workspace_action(
						key,
						"Open source conversation".into(),
						move |s, cx| {
							if s.snapshot.as_ref().is_some_and(|snapshot| {
								snapshot.work_items.iter().any(|item| {
									item.id == work
										&& item.codex_thread_id.as_ref() == Some(&thread)
								})
							}) {
								s.clear_workbench_agent(cx);
								s.workspace.graph_expanded = false;
								s.open_page(&work, cx);
							} else {
								cx.open_url(&format!("codex://threads/{thread}"));
							}
						},
						cx,
					));
			}
		}
		body.into_any_element()
	}

	fn workbench_selected(&self, row: &Row, cx: &mut Context<Self>) -> AnyElement {
		let (task, _) = self.graph_preview(row);
		let (activity_label, action) = self.work_activity_summary(row);
		let mut body = gpui::div().flex().flex_col().min_w_0();
		let identity = if row.native { &row.owner } else { &row.title };
		if !task.is_empty() && task != *identity {
			body = body.child(section("Assignment")).child(note(task));
		}
		if let Some(output) = self.work_output(row) {
			let key = format!("workbench-answer-{}-{}", row.key, output.turn);
			body = body
				.child(section(if row.group == 1 { "Latest update" } else { "Latest result" }))
				.child(
					gpui::div()
						.debug_selector(|| "workbench-answer".into())
						.w_full()
						.min_w_0()
						.child(super::super::markdown::render_process(output.text, &key)),
				)
				.when(output.truncated, |body| body.child(note("This history entry is truncated.")))
				.child(gpui::div().mt_2().child(super::super::markdown::copy_button(
					&format!("workbench-copy-{}", row.key),
					"Copy result",
					output.text.to_owned(),
				)));
		}
		let entries = self.workbench_entries();
		let has_request = entries.iter().any(|entry| entry.row.key == row.key && !entry.result);
		if !action.is_empty() && !activity_label.starts_with("Latest ") && !has_request {
			body = body.child(section(activity_label)).child(note(action));
		}
		for entry in entries.iter().filter(|entry| entry.row.key == row.key && !entry.result) {
			body = body.child(self.workbench_entry(entry, cx));
		}
		if !has_request && (!self.workbench_row_is_recipient(row) || self.workspace.graph_expanded)
		{
			let key = row.key.clone();
			body = body.child(gpui::div().mt_3().flex().child(self.workspace_action(
				"workbench-open-conversation".into(),
				"Continue in conversation".into(),
				move |s, cx| {
					if let Some(row) = s.board_rows().into_iter().find(|row| row.key == key) {
						s.open_graph_agent(&row, cx);
					}
				},
				cx,
			)));
		}
		if let Some(item) = self
			.handoff_items()
			.into_iter()
			.find(|item| item.result && item.work == row.work && !row.native)
		{
			body = body.child(self.workspace_action(
				"workbench-mark-viewed".into(),
				"Mark viewed".into(),
				move |s, cx| {
					if let Some(snapshot) = &s.snapshot {
						s.handoffs.acknowledge(&item.work, &item.key, snapshot);
					}
					cx.notify();
				},
				cx,
			));
		}
		body = body.child(self.workbench_materials(row, cx));
		let others: Vec<_> = entries.iter().filter(|entry| entry.row.key != row.key).collect();
		if !others.is_empty() {
			body = body.child(section("Also needs your attention"));
			for entry in others {
				body = body.child(self.workbench_entry(entry, cx));
			}
		}
		let graph = self.relation_graph();
		let hierarchy = |edge: &&super::relations::Edge| {
			matches!(edge.kind, super::relations::Kind::Assigned | super::relations::Kind::Spawned)
		};
		let owner = if graph.edges.iter().filter(hierarchy).any(|edge| edge.from == row.key) {
			row.key.as_str()
		} else {
			graph
				.edges
				.iter()
				.filter(hierarchy)
				.find(|edge| edge.to == row.key)
				.map(|edge| edge.from.as_str())
				.unwrap_or(&row.key)
		};
		let mut related = std::collections::BTreeSet::from([owner]);
		for edge in graph.edges.iter().filter(hierarchy).filter(|edge| edge.from == owner) {
			related.insert(edge.to.as_str());
		}
		let team: Vec<_> = self
			.factory_rows()
			.into_iter()
			.filter(|other| other.key != row.key && related.contains(other.key.as_str()))
			.collect();
		if !team.is_empty() {
			body = body.child(section("Working with"));
			for other in team {
				body = body.child(self.workbench_agent_row(&other, cx));
			}
		}
		body.into_any_element()
	}

	fn workbench_header(&self, selected: Option<&Row>, _cx: &mut Context<Self>) -> AnyElement {
		let rows = self.factory_rows();
		let running = rows.iter().filter(|row| row.group == 1).count();
		let waiting = rows.iter().filter(|row| row.group == 0).count();
		gpui::div()
			.flex_none()
			.min_w_0()
			.px_4()
			.pt_4()
			.pb_2()
			.flex()
			.flex_col()
			.gap_2()
			.child(
				gpui::div()
					.text_size(gpui::px(10.))
					.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
					.child(if running == 0 && waiting == 0 {
						"Latest work".into()
					} else {
						format!("{running} running · {waiting} need attention")
					}),
			)
			.when_some(selected, |header, row| {
				header.child(
					gpui::div()
						.flex()
						.items_start()
						.gap_2()
						.min_w_0()
						.child(
							gpui::div()
								.size(gpui::px(6.))
								.mt(gpui::px(5.))
								.flex_none()
								.rounded_full()
								.bg(gpui::rgb(row.color)),
						)
						.child(
							gpui::div()
								.flex_1()
								.min_w_0()
								.whitespace_normal()
								.text_size(gpui::px(12.))
								.line_height(gpui::px(17.))
								.font_weight(gpui::FontWeight::MEDIUM)
								.child(if row.native {
									row.owner.clone()
								} else {
									row.title.clone()
								}),
						),
				)
			})
			.into_any_element()
	}

	pub(in super::super) fn render_agent_workbench(&self, cx: &mut Context<Self>) -> AnyElement {
		let selected = self.workbench_row().or_else(|| {
			self.factory_rows().into_iter().find(|row| self.workbench_row_is_recipient(row))
		});
		let connection = self.work_board.view.edge.as_ref().and_then(|picked| {
			self.relation_graph().edges.into_iter().find(|edge| edge.id == picked.id)
		});
		gpui::div()
			.size_full()
			.flex()
			.flex_col()
			.min_h_0()
			.min_w_0()
			.child(
				self.workbench_header(
					if connection.is_some() { None } else { selected.as_ref() },
					cx,
				),
			)
			.child(
				gpui::div()
					.id("workbench-body")
					.debug_selector(|| "workbench-body".into())
					.role(gpui::Role::Group)
					.aria_label("Agent details")
					.flex_1()
					.min_h_0()
					.min_w_0()
					.overflow_y_scroll()
					.px_4()
					.pb_4()
					.capture_any_mouse_down(
						cx.listener(|s, _, _, _| s.workspace.focused_panel = Some(Panel::Right)),
					)
					.child(if let Some(edge) = &connection {
						self.relation_details(edge, cx)
					} else if let Some(row) = &selected {
						self.workbench_selected(row, cx)
					} else {
						self.workbench_overview(cx)
					})
					.smooth_scroll("workbench-scroll"),
			)
			.child(
				gpui::div()
					.flex_none()
					.border_t_1()
					.border_color(gpui::rgba(ui_theme::HOVER_FILL))
					.px_3()
					.py_2()
					.child(
						self.workspace_action(
							"workbench-history".into(),
							if self.work_board.workbench_history_expanded {
								"History ▴"
							} else {
								"History ▾"
							}
							.into(),
							|s, cx| {
								s.work_board.workbench_history_expanded =
									!s.work_board.workbench_history_expanded;
								cx.notify();
							},
							cx,
						),
					),
			)
			.when(self.work_board.workbench_history_expanded, |panel| {
				panel.child(
					gpui::div()
						.debug_selector(|| "workbench-history-section".into())
						.h(gpui::px(240.))
						.flex_none()
						.child(self.deliveries.clone()),
				)
			})
			.into_any_element()
	}
}
