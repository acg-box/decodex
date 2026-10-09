//! Native execution evidence inside a work graph node.
use super::{
	AgentSurface, AgentWorkItemDto, Context, InteractiveElement, ParentElement, Styled,
	ui_theme::{AMBER, GREEN, TEXT_MUTED},
};
use decodex_protocol::{AgentActivityDto, AgentTimelineContent, AgentTimelineResult};
use gpui::{AnyElement, Div};

#[derive(Default)]
pub(super) struct Execution {
	pub(super) runs: Vec<Run>,
}

#[derive(Default)]
pub(super) struct Run {
	id: String,
	status: String,
	steps: Vec<AgentActivityDto>,
	result: Option<String>,
}

impl Execution {
	pub(super) fn read(result: &AgentTimelineResult, work: &str, thread: &str) -> Option<Self> {
		let AgentTimelineResult::Available { work_id, page, .. } = result else {
			return None;
		};
		if work_id.as_str() != work || page.thread_id != thread {
			return None;
		}
		Some(Self::from_entries(&page.entries))
	}

	fn from_entries(entries: &[decodex_protocol::AgentTimelineEntry]) -> Self {
		let mut execution = Self::default();
		for entry in entries {
			let turn = match &entry.content {
				AgentTimelineContent::Item { turn_id, .. }
				| AgentTimelineContent::TurnBoundary { turn_id, .. } => turn_id,
				_ => continue,
			};
			let index =
				execution.runs.iter().position(|run| &run.id == turn).unwrap_or_else(|| {
					execution.runs.push(Run {
						id: turn.clone(),
						status: "Recorded".into(),
						..Run::default()
					});
					execution.runs.len() - 1
				});
			let run = &mut execution.runs[index];
			match &entry.content {
				AgentTimelineContent::Item { kind, text, phase, activity, .. } => {
					if kind == "agentMessage" && phase.as_deref() == Some("final_answer") {
						run.result = Some(preview(text, 240));
					}
					if let Some(activity) = activity
						.as_ref()
						.filter(|activity| kind != "agentInput" && &activity.turn_id == turn)
					{
						if let Some(existing) =
							run.steps.iter_mut().find(|step| step.item_id == activity.item_id)
						{
							*existing = activity.clone();
						} else {
							run.steps.push(activity.clone());
						}
					}
				},
				AgentTimelineContent::TurnBoundary { completed, status, error, .. } => {
					run.status = if *completed {
						status.clone().unwrap_or_else(|| "Ended".into())
					} else {
						"Running".into()
					};
					if let Some(error) = error {
						run.result = Some(error.message.clone());
					}
				},
				_ => {},
			}
		}
		execution
	}
}

impl AgentSurface {
	pub(super) fn execution_details(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Option<Div> {
		// The visible conversation already streams authoritative timeline updates.
		// Reuse those entries instead of freezing the node at its first inspection.
		let live = self.live_dock_execution(work);
		let execution = live.as_ref().or_else(|| {
			self.dock_evidence.execution.as_ref().filter(|_| {
				self.dock_evidence
					.key
					.as_ref()
					.is_none_or(|key| key == &self.dock_evidence_key(work))
			})
		})?;
		let active = matches!(
			work.dispatch_state,
			super::AgentDispatchStateDto::Running | super::AgentDispatchStateDto::Dispatching
		);
		let run = if active {
			execution.runs.iter().find(|run| Some(&run.id) == work.active_turn_id.as_ref())
		} else {
			execution.runs.last()
		}?;
		Some(self.execution_run(work, run, cx))
	}

	fn live_dock_execution(&self, work: &AgentWorkItemDto) -> Option<Execution> {
		let binding = self.timeline.native.binding.as_ref()?;
		(binding.work == work.id
			&& Some(&binding.thread) == work.codex_thread_id.as_ref()
			&& !self.timeline.native.entries.is_empty())
		.then(|| Execution::from_entries(&self.timeline.native.entries))
	}

	fn execution_run(&self, work: &AgentWorkItemDto, run: &Run, cx: &mut Context<Self>) -> Div {
		let mut body = gpui::div().w_full().flex().flex_col().gap_1();
		if let Some(result) = &run.result {
			body = body.child(gpui::div().py_1().child(result.clone()));
		} else if let Some(step) = run.steps.last() {
			body = body.child(if step.detail.is_empty() {
				step.label.clone()
			} else {
				format!("{} · {}", step.label, step.detail)
			});
		} else {
			body = body.child(status_label(&run.status));
		}
		let key = format!("dock-operations-{}-{}", work.id, run.id);
		let expanded = self.timeline.expanded_records.contains(&key);
		if !run.steps.is_empty() {
			body = body.child(self.workspace_action(
				"dock-evidence-toggle".into(),
				"Execution evidence".into(),
				move |s, cx| {
					if !s.timeline.expanded_records.remove(&key) {
						s.timeline.expanded_records.insert(key.clone());
					}
					cx.notify();
				},
				cx,
			));
		}
		if expanded {
			for (index, step) in run.steps.iter().enumerate() {
				body = body.child(self.execution_step(work, step, index, cx));
			}
		}
		body
	}

	fn execution_step(
		&self,
		work: &AgentWorkItemDto,
		step: &AgentActivityDto,
		index: usize,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let color = match step.status.as_str() {
			"failed" | "declined" => AMBER,
			"completed" => GREEN,
			_ => TEXT_MUTED,
		};
		let row = gpui::div()
			.debug_selector({
				let id = format!("dock-step-{}-{}", step.turn_id, step.item_id);
				move || id.clone()
			})
			.w_full()
			.min_w_0()
			.py_1()
			.px_1()
			.flex()
			.items_center()
			.gap_2()
			.border_b_1()
			.border_color(gpui::rgba(0xffffff0c))
			.child(
				gpui::div()
					.w(gpui::px(18.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(format!("{}", index + 1)),
			)
			.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.overflow_hidden()
					.whitespace_nowrap()
					.text_ellipsis()
					.child(if step.detail.is_empty() {
						step.label.clone()
					} else {
						format!("{} · {}", step.label, step.detail)
					}),
			)
			.child(
				gpui::div()
					.flex_none()
					.text_color(gpui::rgb(color))
					.child(status_label(&step.status)),
			)
			.child(gpui::div().w(gpui::px(44.)).text_color(gpui::rgb(TEXT_MUTED)).child(
				step.duration_ms.map(|ms| format!("{:.1}s", ms as f64 / 1000.)).unwrap_or_default(),
			));
		self.detail_row(work, step, row, cx)
	}
}

pub(super) fn preview(text: &str, limit: usize) -> String {
	let plain = super::markdown::plain_text(text);
	let mut chars = plain.chars();
	let result: String = chars.by_ref().take(limit).collect();
	if chars.next().is_some() { format!("{result}…") } else { result }
}

fn status_label(status: &str) -> String {
	match status {
		"completed" => "Completed",
		"failed" => "Failed",
		"interrupted" => "Stopped",
		"inProgress" | "running" => "Running",
		"declined" => "Declined",
		"exited" => "Exited",
		other => other,
	}
	.into()
}

#[cfg(any(test, feature = "visual-capture"))]
#[path = "agent_execution_dock_tests.rs"]
mod tests;
