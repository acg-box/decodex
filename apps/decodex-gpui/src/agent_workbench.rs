//! Persistent collaboration across chats, using native requests and evidence.
#[path = "agent_collaboration.rs"] mod collaboration;
#[cfg(any(test, feature = "visual-capture"))]
#[path = "agent_collaboration_fixture.rs"]
mod reference_fixture;

pub(super) use collaboration::State as Collaboration;

use gpui::{
	AnyElement, Div, IntoElement, SharedString,
	prelude::{ParentElement, Styled},
};

use crate::shell::agent_surface::{
	ui_theme::{self, FIELD_MATERIAL, TEXT_MUTED, WARNING},
	work_board::{AgentSurface, Context, Row},
};

struct Entry {
	row: Row,
	label: String,
	reason: String,
	result: bool,
	request: Option<i64>,
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

		if page == "workbench" {
			self.visual_collaboration_fixture(cx);
		}
		if matches!(page, "workbench-agent" | "workbench-split") {
			self.select_workbench_agent("release", cx);
		}
		if page == "workbench-history" {
			self.work_board.collaboration_expanded = None;
		}

		cx.notify();
	}

	#[cfg(any(test, feature = "visual-capture"))]
	fn visual_workbench_metrics(&mut self) {
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
				recent: vec![],
			});
			let sample = crate::ui_work_metrics::Sample {
				tokens: Some(18_400 + index as u64 * 7_230),
				stats: Some(decodex_protocol::AgentThreadMetrics {
					completed_ms: Some(312_000 + index as u64 * 21_000),
					active_ms: (row.group == 1).then_some(8_000),
					generating,
					rate: Some(decodex_protocol::AgentOutputRate {
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

	#[cfg(test)]
	fn workbench_row(&self) -> Option<Row> {
		let key = self.work_board.view.picked_agent.as_ref()?;

		self.board_rows().into_iter().find(|row| &row.key == key)
	}

	#[cfg(test)]
	fn workbench_row_is_recipient(&self, row: &Row) -> bool {
		if let Some((owner, thread)) = &self.native_agents.selected {
			owner == &row.work && row.thread.as_ref() == Some(thread)
		} else {
			!row.native && self.selected.as_ref() == Some(&row.work) && self.selected_is_agent()
		}
	}

	#[cfg(test)]
	pub(in super::super) fn workbench_has_recipient(&self) -> bool {
		self.workbench_row().is_some_and(|row| self.workbench_row_is_recipient(&row))
	}

	pub(in super::super) fn graph_has_composer(&self) -> bool {
		(self.native_agents.selected.is_some()
			|| self.selected_is_agent()
			|| self.selected_is_archived())
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

	#[cfg(test)]
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
					reason: "Respond in the conversation.".into(),
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
		let number = self.relation_agent_number(&entry.row);
		let identity = self.relation_agent_name(&entry.row);

		gpui::div()
			.flex_none()
			.w_full()
			.min_w_0()
			.p(gpui::px(10.))
			.rounded(gpui::px(12.))
			.bg(ui_theme::rgba(FIELD_MATERIAL))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap(gpui::px(6.))
					.text_size(gpui::px(11.))
					.line_height(gpui::px(18.))
					.child(
						gpui::div()
							.flex_none()
							.size(gpui::px(4.))
							.rounded_full()
							.bg(ui_theme::rgb(WARNING)),
					)
					.child(format!("{number} · {identity}")),
			)
			.child(note(entry.label.clone()).mt_1())
			.child(
				note(entry.reason.clone())
					.text_size(gpui::px(11.))
					.line_height(gpui::px(16.))
					.mt_1()
					.mb_1(),
			)
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
}

fn note(text: impl Into<SharedString>) -> Div {
	gpui::div()
		.text_size(gpui::px(12.))
		.line_height(gpui::px(18.))
		.text_color(ui_theme::rgb(TEXT_MUTED))
		.child(text.into())
}

#[cfg(test)]
#[path = "agent_workbench_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "agent_workbench_wire_tests.rs"]
mod wire_tests;
