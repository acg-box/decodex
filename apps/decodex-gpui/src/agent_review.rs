//! Review presentation from native instruction reads, children and question state.
use decodex_protocol::{AgentReviewActivity, AgentTimelineContent, AgentTimelineEntry};

use super::{AgentSurface, Row};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Evidence {
	pub step: AgentReviewActivity,
	pub turn: String,
	pub item: String,
}

pub(super) fn latest(entries: &[AgentTimelineEntry]) -> Option<Evidence> {
	entries.iter().rev().find_map(|entry| {
		let AgentTimelineContent::Item { activity: Some(activity), .. } = &entry.content else {
			return None;
		};
		Some(Evidence {
			step: activity.review.clone()?,
			turn: activity.turn_id.clone(),
			item: activity.item_id.clone(),
		})
	})
}

impl AgentSurface {
	pub(super) fn review_evidence(&self, row: &Row) -> Option<Evidence> {
		let current = self
			.timeline
			.native
			.binding
			.as_ref()
			.filter(|binding| {
				binding.work == row.work && row.thread.as_ref() == Some(&binding.thread)
			})
			.and_then(|_| latest(&self.timeline.native.entries));
		current.or_else(|| {
			self.work_board
				.briefs
				.get(&row.key)
				.filter(|brief| brief.source_thread == row.thread)
				.and_then(|brief| brief.review.clone())
		})
	}

	pub(super) fn reviewer_role(&self, row: &Row) -> Option<String> {
		if !row.native {
			return None;
		}
		match self.current_review(row)?.step {
			AgentReviewActivity::Reviewer { role } => Some(role.replace('_', " ")),
			_ => None,
		}
	}

	pub(super) fn reviewer_title(&self, row: &Row) -> Option<String> {
		let role = self.reviewer_role(row)?;
		let mut chars = role.chars();
		let title: String = chars.next()?.to_uppercase().chain(chars).collect();
		Some(format!("{title} review"))
	}

	/// Only the current Lead turn owns a preparation/assessment phase. A later
	/// ordinary question must not inherit an earlier review workflow.
	pub(super) fn lead_review(&self, row: &Row) -> Option<Evidence> {
		if row.native {
			return None;
		}
		self.current_review(row)
	}

	fn current_review(&self, row: &Row) -> Option<Evidence> {
		let evidence = self.review_evidence(row)?;
		let brief = self.work_board.briefs.get(&row.key)?;
		let active = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| {
				snapshot
					.work_items
					.iter()
					.find(|work| work.id == row.work && work.codex_thread_id == row.thread)
			})
			.and_then(|work| work.active_turn_id.as_deref());
		let visible = self
			.timeline
			.native
			.binding
			.as_ref()
			.filter(|binding| {
				binding.work == row.work && row.thread.as_ref() == Some(&binding.thread)
			})
			.and_then(|_| super::latest_outcome(&self.timeline.native.entries));
		let turn = active
			.or_else(|| visible.as_ref().map(|(turn, _)| turn.as_str()))
			.or_else(|| brief.preview.as_ref().map(|preview| preview.turn.as_str()))
			.or_else(|| brief.outcome.as_ref().map(|(turn, _)| turn.as_str()))?;
		(turn == evidence.turn).then_some(evidence)
	}

	pub(super) fn review_phase(&self, row: &Row) -> Option<&'static str> {
		if self.reviewer_role(row).is_some() {
			if row.status == "Approval" {
				return Some("Needs approval");
			}
			if row.status == "Input needed" {
				return Some("Needs your answer");
			}
			return Some(if row.group == 1 {
				"Reviewing"
			} else if self
				.work_output(row)
				.is_some_and(|output| self.work_output_complete(row, output.turn))
			{
				"Report ready"
			} else {
				"Review paused"
			});
		}
		let evidence = self.lead_review(row)?;
		if self.graph_question(row).is_some() {
			return Some("Needs your decision");
		}
		if row.group != 1 {
			return Some("Awaiting next step");
		}
		let reviewers: Vec<_> = self
			.native_agents
			.lists
			.get(&row.work)
			.into_iter()
			.flatten()
			.filter(|agent| {
				Some(&agent.parent_thread_id) == row.thread.as_ref()
					&& self
						.work_board
						.briefs
						.get(&format!("native:{}:{}", row.work, agent.thread_id))
						.and_then(|brief| brief.review.as_ref())
						.is_some_and(|evidence| {
							matches!(evidence.step, AgentReviewActivity::Reviewer { .. })
						})
			})
			.collect();
		if reviewers.iter().any(|agent| agent.status == "active") {
			return Some("Waiting for review");
		}
		Some(if !reviewers.is_empty() && matches!(evidence.step, AgentReviewActivity::Assess) {
			"Assessing findings"
		} else {
			"Preparing review"
		})
	}
}

#[cfg(any(test, feature = "visual-capture"))]
#[derive(serde::Deserialize)]
pub(super) struct Capture {
	pub thread: String,
	pub expand: Option<String>,
	pub agents: Vec<decodex_protocol::NativeAgentDto>,
	pub pages: Vec<decodex_protocol::AgentTimelinePage>,
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	/// Read-only rendering of protocol pages captured from a disposable native session.
	#[allow(dead_code, reason = "used by the separate visual capture binary")]
	pub(crate) fn visual_review_capture(
		&mut self,
		path: &std::path::Path,
		cx: &mut gpui::Context<Self>,
	) {
		let data: Capture = serde_json::from_slice(&std::fs::read(path).expect("review capture"))
			.expect("native review pages");
		self.apply_review_capture(data, cx);
	}

	pub(super) fn apply_review_capture(&mut self, data: Capture, cx: &mut gpui::Context<Self>) {
		self.visual_workspace_fixture(cx);
		let snapshot = self.snapshot.as_mut().expect("fixture snapshot");
		snapshot.work_items.retain(|work| work.id == "agent");
		snapshot.dependencies.clear();
		snapshot.context_references.clear();
		let work = &mut snapshot.work_items[0];
		work.codex_thread_id = Some(data.thread.clone());
		work.title = "Review handoff design".into();
		work.kind = decodex_protocol::AgentWorkKindDto::Manager;
		if let Some(page) = data.pages.iter().find(|page| page.thread_id == data.thread)
			&& let Some((turn, outcome)) = super::latest_outcome(&page.entries)
			&& outcome == "inProgress"
		{
			work.active_turn_id = Some(turn);
			work.dispatch_state = decodex_protocol::AgentDispatchStateDto::Running;
		}
		self.native_agents.lists.clear();
		self.native_agents.lists.insert("agent".into(), data.agents);
		self.work_board.briefs.clear();
		self.work_board.collaboration = Default::default();
		let slots = self
			.factory_rows()
			.into_iter()
			.enumerate()
			.map(|(index, row)| (row.key, index as u32))
			.collect();
		self.snapshot.as_mut().expect("capture snapshot").agent_slots = slots;
		for row in self.factory_rows() {
			let Some(page) =
				data.pages.iter().find(|page| Some(&page.thread_id) == row.thread.as_ref())
			else {
				continue;
			};
			self.work_board.briefs.insert(
				row.key.clone(),
				super::Brief {
					review: latest(&page.entries),
					source_thread: row.thread.clone(),
					output: super::activity::output(&page.entries),
					assignments: super::activity::assignments(&page.entries, &page.thread_id),
					outcome: super::latest_outcome(&page.entries),
					preview: super::activity::latest(&page.entries, None),
					question: None,
					resources: None,
					relations: page
						.entries
						.iter()
						.filter_map(|entry| match &entry.content {
							AgentTimelineContent::Item {
								item_id,
								collaboration: Some(call),
								..
							} => Some((item_id.clone(), call.clone())),
							_ => None,
						})
						.collect(),
					metrics: crate::ui_work_metrics::Sample::from_page(page),
					stamp: self.brief_stamp(&row),
					read_at: std::time::Instant::now(),
				},
			);
		}
		self.work_board.graph = true;
		if let Some(thread) = data.expand
			&& let Some(evidence) = self
				.work_board
				.briefs
				.values()
				.find(|brief| brief.source_thread.as_ref() == Some(&thread))
				.and_then(|brief| brief.review.as_ref())
		{
			self.work_board.collaboration_expanded =
				Some(format!("review:{thread}:{}", evidence.turn));
		}
		self.workspace.graph_expanded = false;
		self.workspace.agent_tree_visible = true;
		self.workspace.agent_panel_width = 320.;
		cx.notify();
	}
}
