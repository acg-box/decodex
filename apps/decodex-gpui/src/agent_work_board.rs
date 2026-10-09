//! Source-backed agent relationships and execution previews.
#[path = "agent_relation_canvas.rs"] mod relations;
use super::{
	AgentSurface, Context, InteractiveElement, IntoElement, ParentElement, Styled,
	ui_theme::{AMBER, GREEN, TEXT_MUTED},
};
use gpui::{AnyElement, prelude::FluentBuilder};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Board {
	pub(super) graph: bool,
	focus: Option<String>,
	view: relations::View,
	pub(super) briefs: BTreeMap<String, Brief>,
	pub(super) brief_task: Option<gpui::Task<()>>,
}
impl Board {
	pub(super) fn focus_work(&mut self, id: &str) {
		self.focus = Some(id.into());
	}

	pub(super) fn clear_focus(&mut self) {
		self.focus = None;
	}

	pub(super) fn new(_: &mut Context<AgentSurface>) -> Self {
		Self::default()
	}

	pub(super) fn clear(&mut self, _: &mut Context<AgentSurface>) {
		*self = Self::default();
	}
}
#[derive(Clone)]
pub(super) struct Row {
	key: String,
	work: String,
	thread: Option<String>,
	native: bool,
	title: String,
	owner: String,
	workspace: Option<String>,
	status: String,
	group: u8,
	color: u32,
	review: bool,
}

impl AgentSurface {
	fn board_rows(&self) -> Vec<Row> {
		let Some(snapshot) = &self.snapshot else { return Vec::new() };
		let workspace_for = |id: &str| {
			snapshot
				.workspaces
				.iter()
				.find(|w| w.work_ids.iter().any(|work| work == id))
				.map(|w| w.id.clone())
		};
		let mut rows = Vec::new();
		let mut threads = BTreeSet::new();
		let handoffs = self.handoff_items();
		for work in &snapshot.work_items {
			if let Some(thread) = &work.codex_thread_id {
				threads.insert(thread.clone());
			}
			let state = super::dock::progress_state(snapshot, work);

			let owner = work
				.parent_goal_id
				.as_ref()
				.and_then(|id| snapshot.work_items.iter().find(|w| &w.id == id));
			let review = handoffs.iter().any(|h| h.work == work.id && h.result && h.attention);
			rows.push(Row {
				key: work.id.clone(),
				work: work.id.clone(),
				thread: work.codex_thread_id.clone(),
				native: false,
				title: self.work_label(work),
				owner: owner.map(|w| self.work_label(w)).unwrap_or_default(),

				workspace: workspace_for(&work.id),
				status: state.label.into(),
				review,
				group: state.group,
				color: state.color,
			});
		}
		for (owner, agents) in &self.native_agents.lists {
			if !snapshot.work_items.iter().any(|w| &w.id == owner) {
				continue;
			}
			for agent in agents {
				if !threads.insert(agent.thread_id.clone()) {
					continue;
				}
				let (status, group, color) = native_state(&agent.status);
				let task = agent.task.split_whitespace().collect::<Vec<_>>().join(" ");
				rows.push(Row {
					key: format!("native:{owner}:{}", agent.thread_id),
					work: owner.clone(),
					thread: Some(agent.thread_id.clone()),
					native: true,
					title: if task.is_empty() { agent.title.clone() } else { task },
					owner: agent.title.clone(),
					workspace: workspace_for(owner),
					status: status.into(),
					review: false,
					group,
					color,
				});
			}
		}
		for row in &mut rows {
			if let Some(brief) = self.work_board.briefs.get(&row.key)
				&& brief.stamp == self.brief_stamp(row)
				&& let Some((turn, outcome)) = &brief.outcome
				&& (row.native
					|| snapshot.work_items.iter().any(|w| {
						w.id == row.work
							&& w.active_turn_id.as_ref().is_none_or(|active| active == turn)
					})) {
				apply_outcome(row, outcome);
			}
		}
		rows.sort_by(|a, b| a.key.cmp(&b.key));
		rows
	}
}
fn latest_outcome(entries: &[decodex_protocol::AgentTimelineEntry]) -> Option<(String, String)> {
	entries.iter().rev().find_map(|e| match &e.content {
		decodex_protocol::AgentTimelineContent::TurnBoundary {
			turn_id, completed, status, ..
		} => Some((
			turn_id.clone(),
			if *completed { status.clone().unwrap_or_default() } else { "inProgress".into() },
		)),
		_ => None,
	})
}

fn apply_outcome(row: &mut Row, outcome: &str) {
	// Current execution, requests, uncertainty and dependencies outrank a previous result.
	if ![
		"Not running",
		"Idle",
		"Marked complete",
		"Follow-up pending",
		"Update pending",
		"Scheduled check",
	]
	.contains(&row.status.as_str())
	{
		return;
	}
	let (label, color) = match outcome {
		"failed" => ("Failed", crate::ui_theme::ERROR),
		"interrupted" => ("Interrupted", TEXT_MUTED),
		_ => return,
	};
	row.status = label.into();
	row.color = color;
	row.group = 2;
	row.review = false;
}

fn native_state(status: &str) -> (&'static str, u8, u32) {
	match status {
		"active" | "running" => ("Running", 1, crate::ui_theme::BLUE),
		"waitingOnApproval" => ("Approval", 0, AMBER),
		"waitingOnUserInput" => ("Input needed", 0, AMBER),
		"systemError" => ("Error", 0, crate::ui_theme::ERROR),
		"idle" => ("Idle", 2, TEXT_MUTED),
		"notLoaded" => ("Not running", 2, TEXT_MUTED),
		_ => ("Unknown", 0, TEXT_MUTED),
	}
}

impl AgentSurface {
	pub(super) fn render_work_board(&self, cx: &mut Context<Self>) -> AnyElement {
		let graph = self.relation_graph();
		let mut content =
			gpui::div().flex_1().h_0().min_h_0().flex().child(self.relation_canvas(&graph, cx));
		if let Some(edge) = self
			.work_board
			.view
			.edge
			.as_ref()
			.and_then(|picked| graph.edges.iter().find(|e| e.id == picked.id))
			.cloned()
		{
			content = content.child(self.relation_details(&edge, cx));
		}

		gpui::div()
			.size_full()
			.flex()
			.flex_col()
			.child(content)
			.child(self.relation_metrics(&graph, cx))
			.into_any_element()
	}
}

pub(super) struct Brief {
	outcome: Option<(String, String)>,
	resources: Option<Vec<decodex_protocol::AgentResourceDto>>,
	relations: Vec<(String, decodex_protocol::AgentCollaborationDto)>,
	metrics: relations::metrics::Metrics,
	stamp: String,
	read_at: std::time::Instant,
}

impl AgentSurface {
	fn factory_rows(&self) -> Vec<Row> {
		self.board_rows()
	}

	fn brief_stamp(&self, row: &Row) -> String {
		let work =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == row.work));
		serde_json::json!([
			work.map(|w| self.dock_evidence_key(w)),
			row.thread,
			self.native_agents
				.lists
				.get(&row.work)
				.and_then(|agents| agents
					.iter()
					.find(|a| Some(&a.thread_id) == row.thread.as_ref()))
				.map(|a| &a.status)
		])
		.to_string()
	}

	pub(super) fn refresh_factory_briefs(&mut self, cx: &mut Context<Self>) {
		if self.work_board.graph
			|| !self.workspace.graph_visible
			|| self.work_board.brief_task.is_some()
			|| !self.command_connection_ready()
		{
			return;
		}
		let Some(profile) = self.profile.clone() else { return };
		let mut rows = self.factory_rows();
		rows.retain(|r| r.thread.is_some());
		rows.sort_by_key(|r| self.work_board.briefs.get(&r.key).map(|b| b.read_at));
		let requests: Vec<_> = rows
			.into_iter()
			.filter(|r| {
				self.work_board.briefs.get(&r.key).is_none_or(|b| {
					b.stamp != self.brief_stamp(r)
						|| (r.group == 1 && b.read_at.elapsed().as_secs() >= 5)
						|| b.read_at.elapsed().as_secs() >= 30
				})
			})
			.take(4)
			.map(|row| {
				let stamp = self.brief_stamp(&row);
				(row, stamp)
			})
			.collect();
		if requests.is_empty() {
			return;
		}
		let read = cx.background_executor().spawn(async move {
			let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build()
			else {
				return Vec::new();
			};
			runtime.block_on(async move {
				let client = super::AgentClient::new(profile);
				let mut results = Vec::new();
				for (row, stamp) in requests {
					let (Ok(owner), Some(thread)) = (
						super::EntityId::new(&row.work),
						row.thread.as_deref().and_then(|t| super::EntityId::new(t).ok()),
					) else {
						continue;
					};
					let mut relations = Vec::new();
					let mut outcome = None;
					let mut metrics = relations::metrics::Metrics::default();
					if let Ok(result) = client.timeline(owner.clone(), thread.clone(), None).await
						&& let decodex_protocol::AgentTimelineResult::Available {
							work_id,
							page,
							..
						} = &result
						&& work_id.as_str() == owner.as_str()
						&& page.thread_id == thread.as_str()
					{
						metrics = relations::metrics::Metrics::from_entries(&page.entries);
						outcome = latest_outcome(&page.entries);
						relations = page
							.entries
							.iter()
							.filter_map(|entry| match &entry.content {
								decodex_protocol::AgentTimelineContent::Item {
									item_id,
									collaboration: Some(call),
									..
								} if call.sender_thread_id == page.thread_id => Some((item_id.clone(), call.clone())),
								_ => None,
							})
							.collect();
					}

					let resources = if row.native {
						None
					} else {
						match client.resources(owner).await {
							Ok(decodex_protocol::AgentResourcesResult::Available { resources }) =>
								Some(resources),
							_ => None,
						}
					};
					results.push((
						row.key,
						Brief {
							outcome,
							stamp,
							relations,
							resources,
							metrics,
							read_at: std::time::Instant::now(),
						},
					));
				}
				results
			})
		});
		self.work_board.brief_task = Some(cx.spawn(async move |surface, cx| {
			let results = read.await;
			let _ = surface.update(cx, |s, cx| {
				let rows = s.board_rows();
				s.work_board.briefs.retain(|key, _| rows.iter().any(|r| &r.key == key));
				for (key, brief) in results {
					if rows.iter().any(|r| r.key == key && s.brief_stamp(r) == brief.stamp) {
						s.work_board.briefs.insert(key, brief);
					}
				}
				s.work_board.brief_task = None;
				cx.notify();
			});
		}));
	}
}

#[cfg(test)]
#[path = "agent_work_board_tests.rs"]
mod tests;
