//! Source records for global handoff previews.
use gpui::Div;

use crate::shell::agent_surface::{
	AgentHistoryResult, AgentSnapshotDto, AgentSurface, AgentWorkItemDto, Context,
	InteractiveElement, ParentElement, Styled, graph, ui_theme::TEXT_MUTED,
};

#[derive(Default)]
pub(super) struct Evidence {
	pub(super) key: Option<String>,
	pub(super) execution: Option<super::execution_dock::Execution>,
	history: Option<AgentHistoryResult>,
	final_response: Option<String>,
	request: Option<super::AgentRequestResult>,
	pub(super) task: Option<gpui::Task<()>>,
	read_at: Option<std::time::Instant>,
}

impl AgentSurface {
	#[cfg(any(test, feature = "visual-capture"))]
	pub(super) fn visual_dock_page(&mut self, page: &str, cx: &mut Context<Self>) {
		self.selected = Some("release".into());
		self.handoffs.fixture();
		if let Some(history) = self.timeline.cache.get("agent").cloned() {
			self.timeline.cache.insert("release".into(), history);
		}
		self.history = self.timeline.cache.get("agent").cloned().map(|h| ("release".into(), h));
		let snapshot = self.snapshot.as_mut().expect("fixture snapshot");
		let mut ready =
			snapshot.work_items.iter().find(|w| w.id == "flow").expect("fixture work").clone();
		ready.id = "website".into();
		ready.title = "Documentation update".into();
		ready.parent_goal_id = Some("agent".into());
		snapshot.work_items.push(ready);
		let mut parallel = snapshot
			.work_items
			.iter()
			.find(|w| w.id == "improve")
			.expect("running fixture")
			.clone();
		parallel.id = "search".into();
		parallel.title = "Search indexing".into();
		parallel.parent_goal_id = Some("agent".into());
		snapshot.work_items.push(parallel);
		if let Some(mut history) = self.timeline.cache.get("verify").cloned() {
			if let AgentHistoryResult::Available { entries, .. } = &mut history {
				entries[0].text =
					"The setup guide now includes the new sign-in flow and recovery steps.".into();
			}
			self.timeline.cache.insert("website".into(), history);
		}
		self.workspace.dock_record = match page {
			"dock-result" => Some("website".into()),
			"dock-running" | "dock-dependencies" | "dock-blocked" => Some("release".into()),
			_ => None,
		};
		self.handoffs.focus = self.workspace.dock_record.clone();
		self.workspace.graph_panel_height = if page == "dock-result" { 280. } else { 460. };
		if page == "dock-completed" {
			self.handoffs.observe("fixture-baseline".into(), snapshot);
			for work in &mut snapshot.work_items {
				work.dispatch_state = super::AgentDispatchStateDto::Idle;
			}
		}
		if page == "dock-dependencies" || page == "dock-blocked" {
			self.selected = Some("agent".into());
			self.history = self.timeline.cache.get("agent").cloned().map(|h| ("agent".into(), h));
			self.handoffs.focus =
				Some(if page == "dock-blocked" { "impact" } else { "release" }.into());
			self.workspace.dock_completed = true;
		}

		cx.notify();
	}

	pub(super) fn graph_node_caption(&self, work: &AgentWorkItemDto) -> String {
		let Some(snapshot) = &self.snapshot else {
			return "Status unavailable".into();
		};
		let state = progress_state(snapshot, work);
		if state.group > 0 && work.dispatch_state == super::AgentDispatchStateDto::Idle {
			let children: Vec<_> = snapshot
				.work_items
				.iter()
				.filter(|child| child.parent_goal_id.as_deref() == Some(&work.id))
				.collect();
			let running =
				children.iter().filter(|child| progress_state(snapshot, child).group == 1).count();
			let waiting = children
				.iter()
				.filter(|child| !graph::blockers(snapshot, child).is_empty())
				.count();
			if running > 0 || waiting > 0 {
				return format!("{running} working · {waiting} waiting");
			}
			if !children.is_empty()
				&& children.iter().all(|child| child.status == super::AgentWorkStatusDto::Resolved)
			{
				return format!("View {} results", children.len());
			}
		}

		if state.group == 3 {
			return "View result".into();
		}
		if state.group <= 1 {
			return state.label.into();
		}
		let blockers = graph::blockers(snapshot, work);

		if !blockers.is_empty() {
			return format!(
				"Waiting for {}",
				blockers.iter().map(|w| self.work_label(w)).collect::<Vec<_>>().join(", ")
			);
		}
		state.label.into()
	}

	pub(super) fn graph_context(&self) -> String {
		let layout = self.workspace_graph_full_layout();
		let Some(snapshot) = &self.snapshot else {
			return "Loading work…".into();
		};
		let scope = self.workspace.dock_record.clone().or_else(|| self.dock_scope());
		let owner = snapshot.work_items.iter().find(|w| Some(&w.id) == scope.as_ref());
		let children: Vec<_> = snapshot
			.work_items
			.iter()
			.filter(|w| w.parent_goal_id == scope && scope.is_some())
			.collect();
		if !layout.edges.is_empty() {
			return format!(
				"{} · Arrows show what must finish first",
				owner.map(|work| self.work_label(work)).unwrap_or_else(|| "Work".into())
			);
		}
		if let Some(owner) = owner.filter(|_| !children.is_empty()) {
			let complete =
				children.iter().filter(|w| w.status == super::AgentWorkStatusDto::Resolved).count();
			format!(
				"{} · {} tasks · {} completed",
				self.work_label(owner),
				children.len(),
				complete
			)
		} else {
			"Select a task to view its activity or result".into()
		}
	}

	pub(super) fn dock_scope(&self) -> Option<String> {
		let snapshot = self.snapshot.as_ref()?;
		let root = self.root_id()?;
		let mut id = self.selected.clone().unwrap_or_else(|| root.clone());
		for _ in 0..snapshot.work_items.len() {
			let Some(work) = snapshot.work_items.iter().find(|work| work.id == id) else {
				return Some(root);
			};
			match work.parent_goal_id.as_ref() {
				Some(parent) if parent != &root => id = parent.clone(),
				_ => return Some(id),
			}
		}
		Some(root)
	}

	pub(super) fn dock_evidence_key(&self, work: &AgentWorkItemDto) -> String {
		serde_json::json!([
			work.id,
			work.codex_thread_id,
			self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref()),
			work.updated_at_micros,
			self.handoff_items().iter().find(|h| h.work == work.id).map(|h| &h.key)
		])
		.to_string()
	}

	pub(super) fn toggle_dock_record(&mut self, id: &str, cx: &mut Context<Self>) {
		if self.workspace.dock_record.as_deref() == Some(id) {
			self.workspace.dock_record = None;
			self.dock_evidence = Evidence::default();
			cx.notify();
			return;
		}
		self.workspace.dock_record = Some(id.into());
		self.handoffs.focus = Some(id.into());
		self.workspace.dock_completed = true;
		self.workspace.graph_pan = (0., 0.);
		self.workspace.graph_zoom = 1.;
		self.load_dock_evidence(id, cx);
	}

	pub(super) fn refresh_dock_evidence(&mut self, cx: &mut Context<Self>) {
		if !self.workspace.graph_visible {
			return;
		}
		let Some(id) = self.handoffs.focus.clone() else { return };
		let Some(work) =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == id))
		else {
			return;
		};
		let changed = self.dock_evidence.key.as_ref() != Some(&self.dock_evidence_key(work));
		let running = matches!(
			work.dispatch_state,
			super::AgentDispatchStateDto::Running | super::AgentDispatchStateDto::Dispatching
		);
		let due = self.dock_evidence.read_at.is_none_or(|at| at.elapsed().as_secs() >= 2);
		if changed || (running && due && self.dock_evidence.task.is_none()) {
			self.load_dock_evidence(&id, cx);
		}
	}

	pub(super) fn load_dock_evidence(&mut self, id: &str, cx: &mut Context<Self>) {
		let Some(work) =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == id))
		else {
			return;
		};
		if !self.command_connection_ready() || work.codex_thread_id.is_none() {
			return;
		}
		let Some(profile) = self.profile.clone() else { return };
		let key = self.dock_evidence_key(work);
		if self.dock_evidence.key.as_ref() != Some(&key) {
			self.dock_evidence = Evidence::default();
		}
		let Ok(owner) = super::EntityId::new(id) else { return };
		let Some(thread) =
			work.codex_thread_id.as_deref().and_then(|id| super::EntityId::new(id).ok())
		else {
			return;
		};
		self.dock_evidence.key = Some(key.clone());
		self.dock_evidence.read_at = Some(std::time::Instant::now());
		let event = self
			.snapshot
			.as_ref()
			.and_then(|s| {
				s.pending_events.iter().find(|e| {
					e.work_item_id == id
						&& ["permission_pending", "user_input_pending", "server_request_pending"]
							.contains(&e.event_kind.as_str())
				})
			})
			.map(|e| e.id);
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(async move {
				let client = super::AgentClient::new(profile);
				let request =
					if let Some(event) = event { client.request(event).await.ok() } else { None };
				let (history, timeline) = tokio::join!(
					client.history(owner.clone()),
					client.timeline(owner.clone(), thread.clone(), None)
				);
				let timeline = timeline.ok();
				let execution = timeline.as_ref().and_then(|result| {
					super::execution_dock::Execution::read(result, owner.as_str(), thread.as_str())
				});
				let report = timeline
					.and_then(|result| final_response(&result, owner.as_str(), thread.as_str()));
				Some((
					history.unwrap_or(AgentHistoryResult::Unavailable),
					request,
					report,
					execution,
				))
			})
		});
		self.dock_evidence.task = Some(cx.spawn(async move |surface, cx| {
			let (history, request, report, execution) =
				request.await.unwrap_or((AgentHistoryResult::Unavailable, None, None, None));
			let _ = surface.update(cx, |s, cx| {
				if s.dock_evidence.key.as_ref() != Some(&key) {
					return;
				}
				let current = s.snapshot.as_ref().and_then(|snapshot| {
					snapshot.work_items.iter().find(|w| {
						Some(&w.id)
							== s.handoffs.focus.as_ref().or(s.workspace.dock_record.as_ref())
					})
				});
				if current.is_none_or(|work| s.dock_evidence_key(work) != key) {
					s.dock_evidence = Evidence::default();
					return;
				}
				s.dock_evidence.history = Some(history);
				s.dock_evidence.final_response = report;
				s.dock_evidence.execution = execution;
				s.dock_evidence.request = request;
				s.dock_evidence.task = None;
				cx.notify();
			});
		}));
	}

	fn overview_history(&self, work: &AgentWorkItemDto) -> Option<&AgentHistoryResult> {
		if self.dock_evidence.key.as_ref() == Some(&self.dock_evidence_key(work))
			&& self.dock_evidence.history.is_some()
		{
			return self.dock_evidence.history.as_ref();
		}
		self.history
			.as_ref()
			.filter(|(id, _)| id == &work.id && self.native_agents.selected.is_none())
			.map(|(_, history)| history)
			.or_else(|| self.timeline.cache.get(&work.id))
	}

	pub(super) fn handoff_decision_excerpt(&self, work: &AgentWorkItemDto) -> Option<String> {
		if work.status != super::AgentWorkStatusDto::UserDecision {
			return None;
		}
		let AgentHistoryResult::Available { entries, .. } = self.overview_history(work)? else {
			return None;
		};
		entries
			.iter()
			.rev()
			.find(|entry| {
				source_matches(work, entry) && entry.kind == "assistant" && entry.activity.is_none()
			})
			.map(|entry| {
				format!(
					"From the conversation: {}",
					report_excerpt(entry.text.rsplit("\n\n").next().unwrap_or(&entry.text))
				)
			})
	}

	pub(super) fn handoff_request_excerpt(&self, work: &AgentWorkItemDto) -> Option<String> {
		if self.dock_evidence.key.as_ref() != Some(&self.dock_evidence_key(work)) {
			return None;
		}
		let super::AgentRequestResult::Available { work_id, request_json, .. } =
			self.dock_evidence.request.as_ref()?
		else {
			return None;
		};
		if work_id != &work.id {
			return None;
		}
		request_excerpt(request_json.as_str())
	}

	pub(super) fn current_dock_activity(&self, work: &AgentWorkItemDto) -> Option<String> {
		let turn = work.active_turn_id.as_deref()?;
		let AgentHistoryResult::Available { entries, .. } = self.overview_history(work)? else {
			return None;
		};
		entries.iter().rev().find_map(|entry| {
			if !source_matches(work, entry) {
				return None;
			}
			let activity = entry.activity.as_ref()?;
			(activity.turn_id == turn).then(|| format!("{} · {}", activity.label, activity.status))
		})
	}

	pub(super) fn dock_dependencies(&self, work: &AgentWorkItemDto, cx: &mut Context<Self>) -> Div {
		let mut body = gpui::div().flex().flex_col().gap_1();
		let Some(snapshot) = &self.snapshot else {
			return body;
		};
		let blockers = graph::blockers(snapshot, work);
		let dependents: Vec<_> = snapshot
			.work_items
			.iter()
			.filter(|candidate| {
				graph::blockers(snapshot, candidate).iter().any(|blocker| blocker.id == work.id)
			})
			.collect();
		for (label, tasks) in [("Waiting for", blockers), ("Holding up", dependents)] {
			if tasks.is_empty() {
				continue;
			}
			body = body.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(label));
			for task in tasks {
				let id = task.id.clone();
				body = body.child(self.workspace_action(
					format!("dock-related-{id}"),
					self.work_label(task),
					move |s, cx| s.activate_graph_node(&id, cx),
					cx,
				));
			}
		}
		body
	}

	pub(super) fn overview_evidence(&self, work: &AgentWorkItemDto, cx: &mut Context<Self>) -> Div {
		let id = work.id.clone();
		let state = self.snapshot.as_ref().map(|snapshot| progress_state(snapshot, work));
		let source_label = match state.as_ref().map(|s| s.label) {
			Some("Needs you") => "Open task to respond",
			Some("Marked complete") => "Review result in conversation",
			_ => "Open source conversation",
		};
		let source = self.workspace_action(
			format!("dock-source-{id}"),
			source_label.into(),
			move |s, cx| {
				s.open_page(&id, cx);
				s.workspace.graph_expanded = false;
				s.workspace.dock_record = None;
				s.handoffs.focus = None;
				s.sync_request(cx);
				cx.notify();
			},
			cx,
		);
		let mut body = gpui::div().w_full().min_w_0().pl_2().py_1().flex().flex_col().gap_2();
		if state.as_ref().is_some_and(|state| state.label == "Waiting on work") {
			return body.child(gpui::div().flex().child(source));
		}
		let fetched = self.dock_evidence.key.as_ref() == Some(&self.dock_evidence_key(work));
		let running = matches!(
			work.dispatch_state,
			super::AgentDispatchStateDto::Running | super::AgentDispatchStateDto::Dispatching
		);
		let excerpt = if running {
			self.current_dock_activity(work).map(|text| (text, "Latest activity in this turn"))
		} else if fetched {
			self.dock_evidence
				.final_response
				.as_deref()
				.map(|text| (text.to_owned(), "Final response"))
		} else if let Some(AgentHistoryResult::Available { entries, .. }) =
			self.overview_history(work)
		{
			entries
				.iter()
				.rev()
				.find(|entry| {
					source_matches(work, entry)
						&& entry.kind == "assistant"
						&& entry.activity.is_none()
				})
				.map(|entry| (report_excerpt(&entry.text), "Saved message"))
		} else {
			None
		};
		if let Some((text, label)) = excerpt {
			if label != "Final response" {
				body = body.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(label));
			}
			body = body.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.debug_selector(|| "dock-report-excerpt".into())
					.child(text),
			);
		} else {
			let loading = self.dock_evidence.key.as_ref() == Some(&self.dock_evidence_key(work))
				&& self.dock_evidence.task.is_some();
			body = body.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(if loading {
				"Loading task activity…"
			} else if running {
				"Running · No activity received for this turn yet."
			} else {
				"No final response is available here. Open the conversation to review its history."
			}));
		}
		body.child(gpui::div().flex().child(source))
	}
}

pub(super) struct ProgressState {
	pub(super) label: &'static str,
	pub(super) reason: String,
	pub(super) color: u32,
	pub(super) group: u8,
}

pub(super) fn progress_state(
	snapshot: &AgentSnapshotDto,
	work: &AgentWorkItemDto,
) -> ProgressState {
	use super::{AgentDispatchStateDto as Dispatch, AgentWorkStatusDto as Status};
	use crate::ui_theme::{AMBER, BLUE};
	let request = snapshot.pending_events.iter().find(|e| {
		e.work_item_id == work.id
			&& ["permission_pending", "user_input_pending", "server_request_pending"]
				.contains(&e.event_kind.as_str())
	});
	let (label, reason, color, group) = if let Some(request) = request {
		(
			"Needs you",
			match request.event_kind.as_str() {
				"permission_pending" => "A permission request needs your response.",
				"user_input_pending" => "A question needs your answer.",
				_ => "An unresolved request needs your response.",
			}
			.into(),
			AMBER,
			0,
		)
	} else if work.dispatch_state == Dispatch::Unknown {
		(
			"Status unavailable",
			"Execution could not be confirmed. Open the task to check its connection.".into(),
			TEXT_MUTED,
			0,
		)
	} else if work.dispatch_state == Dispatch::Running {
		("Running", "Execution is active. Open the conversation for live activity.".into(), BLUE, 1)
	} else if work.dispatch_state == Dispatch::Dispatching {
		("Starting", "The task is being submitted; execution is not yet confirmed.".into(), BLUE, 1)
	} else if work.status == Status::UserDecision {
		(
			"Needs you",
			"The task is waiting for your decision. Open its conversation for the question.".into(),
			AMBER,
			0,
		)
	} else if work.status == Status::Resolved {
		(
			"Marked complete",
			"Review the result and its verification in the source conversation.".into(),
			crate::ui_theme::GREEN,
			3,
		)
	} else {
		let blockers = graph::blockers(snapshot, work);
		if !blockers.is_empty() {
			(
				"Waiting on work",
				format!(
					"Waiting for: {}",
					blockers.iter().map(|w| w.title.as_str()).collect::<Vec<_>>().join(", ")
				),
				AMBER,
				2,
			)
		} else if snapshot.pending_events.iter().any(|e| e.work_item_id == work.id) {
			("Update pending", "A recorded update has not been processed yet. It is not a request for your approval.".into(), TEXT_MUTED, 2)
		} else {
			match work.status {
				Status::FollowUp => (
					"Follow-up pending",
					"A follow-up is recorded, but no execution is active.".into(),
					TEXT_MUTED,
					2,
				),
				Status::Wait if work.next_check_at_micros.is_some() => (
					"Scheduled check",
					format!(
						"Next check · {}",
						super::next_check_text(
							work.next_check_at_micros.expect("scheduled wait has a check time")
						)
					),
					TEXT_MUTED,
					2,
				),
				Status::Wait => (
					"Not running",
					"No active execution or recorded prerequisite. The reason is not available."
						.into(),
					TEXT_MUTED,
					2,
				),
				_ => (
					"Not running",
					"The task is open, but no execution is active.".into(),
					TEXT_MUTED,
					2,
				),
			}
		}
	};
	ProgressState { label, reason, color, group }
}

pub(super) fn final_response(
	result: &decodex_protocol::AgentTimelineResult,
	work: &str,
	thread: &str,
) -> Option<String> {
	final_response_text(result, work, thread).map(|text| super::execution_dock::preview(&text, 240))
}

pub(super) fn final_response_text(
	result: &decodex_protocol::AgentTimelineResult,
	work: &str,
	thread: &str,
) -> Option<String> {
	use decodex_protocol::{AgentTimelineContent, AgentTimelineResult};
	let contents: Vec<_> = match result {
		AgentTimelineResult::Available { work_id, page, .. }
			if work_id.as_str() == work && page.thread_id == thread =>
			page.entries.iter().map(|entry| &entry.content).collect(),
		AgentTimelineResult::Summary { work_id, thread_id, items, .. }
			if work_id.as_str() == work && thread_id == thread =>
			items.iter().collect(),
		_ => return None,
	};
	contents.into_iter().rev().find_map(|content| match content {
		AgentTimelineContent::Item { kind, phase, text, .. }
			if kind == "agentMessage"
				&& phase.as_deref() == Some("final_answer")
				&& !text.trim().is_empty() =>
			Some(text.clone()),
		_ => None,
	})
}

fn report_excerpt(text: &str) -> String {
	let paragraph = text.split("\n\n").find(|part| !part.trim().is_empty()).unwrap_or("");
	let plain = super::markdown::plain_text(paragraph).replace(" 。", "。").replace(" .", ".");
	let mut chars = plain.chars();
	let excerpt: String = chars.by_ref().take(240).collect();
	if chars.next().is_some() { format!("{excerpt}…") } else { excerpt }
}

fn source_matches(work: &AgentWorkItemDto, entry: &super::AgentHistoryEntryDto) -> bool {
	entry
		.native_source
		.as_ref()
		.is_none_or(|source| work.codex_thread_id.as_ref() == Some(&source.thread_id))
}

pub(super) fn request_excerpt(json: &str) -> Option<String> {
	let value: serde_json::Value = serde_json::from_str(json).ok()?;
	value["questions"]
		.as_array()
		.and_then(|q| q.first())
		.and_then(|q| q["question"].as_str())
		.or_else(|| value["reason"].as_str())
		.or_else(|| value["command"].as_str())
		.map(report_excerpt)
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::AppContext;

	#[gpui::test]
	fn running_preview_rejects_activity_from_an_old_turn(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let mut work = s
				.snapshot
				.as_ref()
				.unwrap()
				.work_items
				.iter()
				.find(|w| w.id == "improve")
				.unwrap()
				.clone();
			work.active_turn_id = Some("current".into());
			let mut history = s.timeline.cache.get("verify").unwrap().clone();
			let AgentHistoryResult::Available { entries, .. } = &mut history else {
				panic!("fixture history");
			};
			entries[0].native_source = None;
			entries[0].activity = Some(decodex_protocol::AgentActivityDto {
				turn_id: "previous".into(),
				item_id: "command".into(),
				kind: "commandExecution".into(),
				status: "completed".into(),
				label: "Run launch benchmark".into(),
				detail: String::new(),
				plugin_id: None,
				read_only_hint: None,
				native_timestamp_ms: None,
				duration_ms: None,
			});
			s.timeline.cache.insert(work.id.clone(), history.clone());
			assert_eq!(s.current_dock_activity(&work), None);
			let AgentHistoryResult::Available { entries, .. } = &mut history else {
				unreachable!()
			};
			entries[0].activity.as_mut().unwrap().turn_id = "current".into();
			s.timeline.cache.insert(work.id.clone(), history);
			assert_eq!(
				s.current_dock_activity(&work).as_deref(),
				Some("Run launch benchmark · completed")
			);
		});
	}

	#[gpui::test]
	fn dependency_preview_opens_the_blocker_in_the_same_canvas(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = AgentSurface::new(cx);
			s.visual_workspace_fixture(cx);
			s.visual_dock_page("dock-dependencies", cx);
			s.handoffs.focus = Some("impact".into());
			s.update_graph_inset(800., 400.);
			assert!(s.workspace.graph_fit_zoom < 1., "long chains must fit below 100 percent");
			s
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		visual.update(|w, cx| {
			w.refresh();
			w.draw(cx).clear();
		});
		visual.run_until_parked();
		let link = visual.debug_bounds("dock-related-improve").expect("named prerequisite");
		visual.simulate_click(link.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.handoffs.focus.as_deref(), Some("improve"));
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.workspace.dock_record.as_deref(), Some("release"));
		});
	}

	#[test]
	fn result_excerpt_keeps_the_conclusion_without_report_metadata() {
		assert_eq!(
			report_excerpt("确认 **两项缺陷**。\n\n正式 work ID: `internal-task-id`"),
			"确认 两项缺陷。"
		);
		assert_eq!(
			report_excerpt("Found **two defects**.\n\nTask ID: `internal-task-id`"),
			"Found two defects."
		);
		assert_eq!(
			report_excerpt("检查已完成。确认三项问题。\n\n记录 ID: 123"),
			"检查已完成。确认三项问题。"
		);
	}

	#[gpui::test]
	fn dock_primary_action_opens_the_request_without_switching_conversations(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = AgentSurface::new(cx);
			s.visual_workspace_fixture(cx);
			s.visual_dock_page("dock-running", cx);
			s.selected = Some("agent".into());
			s.workspace.dock_record = None;
			s.handoffs.focus = None;
			s
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		visual.update(|w, cx| {
			w.refresh();
			w.draw(cx).clear();
		});
		visual.run_until_parked();
		let action = visual.debug_bounds("dock-next-action").expect("primary action");
		visual.simulate_click(action.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.handoffs.focus.as_deref(), Some("release"));
			assert_eq!(s.workspace.dock_record.as_deref(), Some("release"));
		});
	}

	#[test]
	fn dock_results_use_native_final_answers_instead_of_later_commentary() {
		use decodex_protocol::{
			AgentTimelineContent, AgentTimelineEntry, AgentTimelinePage, AgentTimelineResult,
			EntityId,
		};
		let message = |phase: Option<&str>, text: &str| AgentTimelineContent::Item {
			collaboration: None,
			turn_id: "turn".into(),
			item_id: text.into(),
			kind: "agentMessage".into(),
			phase: phase.map(str::to_owned),
			text: text.into(),
			truncated: false,
			activity: None,
			app_ui: false,
			attachments: vec![],
		};
		let items = vec![
			message(Some("final_answer"), "Found two confirmed defects."),
			message(Some("commentary"), "I will revise the report."),
		];
		let results = [
			AgentTimelineResult::Summary {
				work_id: EntityId::new("work").unwrap(),
				account_id: EntityId::new("account").unwrap(),
				thread_id: "thread".into(),
				items: items.clone(),
			},
			AgentTimelineResult::Available {
				work_id: EntityId::new("work").unwrap(),
				account_id: EntityId::new("account").unwrap(),
				page: AgentTimelinePage {
					safety_buffering_turn_id: None,
					thread_id: "thread".into(),
					entries: items
						.into_iter()
						.enumerate()
						.map(|(position, content)| AgentTimelineEntry {
							position: position as u64,
							content,
						})
						.collect(),
					next_cursor: None,
					weather: Default::default(),
					active_realtime_session_at_page_start: None,
				},
			},
		];
		for result in results {
			assert_eq!(
				final_response(&result, "work", "thread").as_deref(),
				Some("Found two confirmed defects.")
			);
			assert_eq!(final_response(&result, "other-work", "thread"), None);
			assert_eq!(final_response(&result, "work", "other-thread"), None);
		}
		let unknown = AgentTimelineResult::Summary {
			work_id: EntityId::new("work").unwrap(),
			account_id: EntityId::new("account").unwrap(),
			thread_id: "thread".into(),
			items: vec![
				message(None, "I will check."),
				message(Some("commentary"), "Checking now."),
			],
		};
		assert_eq!(final_response(&unknown, "work", "thread"), None);
		let long = format!("{}END_OF_REPLY", "Evidence line.\n".repeat(100));
		let full = AgentTimelineResult::Summary {
			work_id: EntityId::new("work").unwrap(),
			account_id: EntityId::new("account").unwrap(),
			thread_id: "thread".into(),
			items: vec![message(Some("final_answer"), &long)],
		};
		assert_eq!(final_response_text(&full, "work", "thread"), Some(long));
	}

	#[gpui::test]
	fn graph_controls_expand_restore_and_close(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = AgentSurface::new(cx);
			s.visual_workspace_fixture(cx);
			s
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		for (control, expanded, visible) in [
			("graph-expand", true, true),
			("graph-expand", false, true),
			("graph-close", false, false),
		] {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
			std::thread::sleep(std::time::Duration::from_millis(240));
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			let bounds = visual.debug_bounds(control).expect("graph control");
			visual.simulate_click(bounds.center(), Default::default());
			surface.update(visual, |s, _| {
				assert_eq!(s.workspace.graph_expanded, expanded);
				assert_eq!(s.workspace.graph_visible, visible);
				assert_eq!(s.selected.as_deref(), Some("agent"));
			});
		}
	}

	#[gpui::test]
	fn dock_expands_in_place_and_source_navigation_follows_conversation(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = AgentSurface::new(cx);
			surface.visual_workspace_fixture(cx);
			surface.workspace.graph_panel_height = 400.;
			surface
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		let row = visual.debug_bounds("graph-node-release").expect("work node");
		visual.simulate_click(row.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.handoffs.focus.as_deref(), Some("release"));
		});
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		std::thread::sleep(std::time::Duration::from_millis(240));
		visual.update(|w, cx| {
			w.refresh();
			w.draw(cx).clear();
		});
		let source = visual.debug_bounds("handoff-open-request").expect("source link");
		visual.simulate_click(source.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("release"));
			assert_eq!(s.dock_scope().as_deref(), Some("release"));
			assert!(s.workspace.dock_record.is_none());
		});
	}
	#[gpui::test]
	fn canvas_node_selection_keeps_the_parent_graph_and_conversation(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = AgentSurface::new(cx);
			s.visual_workspace_fixture(cx);
			s.visual_dock_page("dock-running", cx);
			s.selected = Some("agent".into());
			s
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		let graph = visual.debug_bounds("work-graph-canvas").unwrap();
		let node = visual.debug_bounds("graph-node-verify").expect("visible dependency node");
		assert!(node.bottom() <= graph.bottom());
		visual.simulate_click(node.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.workspace.dock_record.as_deref(), Some("release"));
			assert_eq!(s.handoffs.focus.as_deref(), Some("verify"));
		});
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		assert_eq!(
			visual.debug_bounds("work-graph-canvas").unwrap(),
			graph,
			"opening details must not resize the graph or create a second panel"
		);
		let expanded = visual.debug_bounds("graph-node-verify").unwrap();
		let details = visual.debug_bounds("handoff-preview").unwrap();
		assert!(details.left() >= expanded.left() && details.right() <= expanded.right());
		assert!(details.top() >= expanded.top() && details.bottom() <= expanded.bottom());
		assert!(visual.debug_bounds("graph-node-flow").is_some());
		let close = visual.debug_bounds("handoff-close").unwrap();
		visual.simulate_click(close.center(), Default::default());
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		assert!(visual.debug_bounds("handoff-preview").is_none());
		std::thread::sleep(std::time::Duration::from_millis(240));
		visual.update(|w, cx| {
			w.refresh();
			w.draw(cx).clear();
		});
		assert!(
			visual.debug_bounds("work-graph-canvas").is_some(),
			"closing details preserves the graph"
		);
	}

	#[gpui::test]
	fn dock_does_not_reuse_fetched_history_after_thread_rebinding(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let work = s.snapshot.as_ref().unwrap().work_items[0].clone();
			s.dock_evidence.key = Some(s.dock_evidence_key(&work));
			s.dock_evidence.history = Some(AgentHistoryResult::Unavailable);
			assert!(matches!(s.overview_history(&work), Some(AgentHistoryResult::Unavailable)));
			let mut rebound = work;
			rebound.codex_thread_id = Some("another-thread".into());
			assert!(matches!(
				s.overview_history(&rebound),
				Some(AgentHistoryResult::Available { .. })
			));
		});
	}
	#[gpui::test]
	fn canvas_keeps_long_reports_bounded_below_the_composer(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = AgentSurface::new(cx);
			surface.visual_workspace_fixture(cx);
			surface.visual_dock_page("dock-result", cx);
			surface.selected = Some("agent".into());
			if let Some(AgentHistoryResult::Available { entries, .. }) =
				surface.timeline.cache.get_mut("website")
			{
				entries[0].text = "A long source report with substantial detail. ".repeat(200);
			}
			surface
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		let detail = visual.debug_bounds("handoff-preview").expect("handoff preview");
		assert!(detail.size.height <= gpui::px(680.));
		let excerpt = visual.debug_bounds("dock-report-excerpt").expect("bounded report");
		assert!(excerpt.size.height < gpui::px(240.));
		let strip = visual.debug_bounds("work-dock").expect("Dock");
		assert!(detail.top() >= strip.top());
		let composer = visual.debug_bounds("floating-composer").expect("floating composer");
		assert!(composer.bottom() <= strip.top(), "composer stays above the whole Dock");
		surface.update(visual, |s, _| assert_eq!(s.selected.as_deref(), Some("agent")));
		visual.simulate_resize(gpui::size(gpui::px(900.), gpui::px(840.)));
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		let preview = visual.debug_bounds("handoff-preview").unwrap();
		assert!(preview.right() <= gpui::px(900.));
	}
	#[gpui::test]
	fn progress_distinguishes_waiting_requests_and_reported_completion(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let snapshot = s.snapshot.as_mut().unwrap();
			let impact = snapshot.work_items.iter().find(|w| w.id == "impact").unwrap().clone();
			assert!(progress_state(snapshot, &impact).reason.contains("Improve launch time"));
			let improve = snapshot.work_items.iter_mut().find(|w| w.id == "improve").unwrap();
			improve.status = super::super::AgentWorkStatusDto::Resolved;
			assert_eq!(
				progress_state(snapshot, &impact).label,
				"Waiting on work",
				"a running prerequisite cannot unblock a task"
			);
			let improve = snapshot.work_items.iter_mut().find(|w| w.id == "improve").unwrap();
			improve.dispatch_state = super::super::AgentDispatchStateDto::Idle;
			assert_eq!(progress_state(snapshot, &impact).label, "Not running");
			let mut unexplained_wait = impact.clone();
			unexplained_wait.status = super::super::AgentWorkStatusDto::Wait;
			let state = progress_state(snapshot, &unexplained_wait);
			assert_eq!(state.label, "Not running");
			assert_eq!(state.color, TEXT_MUTED);
			assert!(state.reason.contains("reason is not available"));
			unexplained_wait.next_check_at_micros = Some(1);
			let state = progress_state(snapshot, &unexplained_wait);
			assert_eq!(state.label, "Scheduled check");
			assert_eq!(state.color, TEXT_MUTED);
			assert!(state.reason.contains("Due now"));
			snapshot.pending_events.push(decodex_protocol::AgentPendingEventDto {
				id: 88,
				source_event_id: "request".into(),
				work_item_id: "impact".into(),
				event_kind: "user_input_pending".into(),
				created_at_micros: 1,
				delivery_claimed: true,
			});
			assert_eq!(
				progress_state(snapshot, &impact).label,
				"Needs you",
				"delivery is not resolution"
			);
			let completed = snapshot.work_items.iter().find(|w| w.id == "flow").unwrap();
			assert_eq!(progress_state(snapshot, completed).label, "Marked complete");
		});
	}

	#[gpui::test]
	fn handoffs_remain_global_across_navigation_and_drafts(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_dock_page("dock-result", cx);
			let before = s.handoff_items();
			assert!(!before.is_empty());
			s.open_page("verify", cx);
			assert_eq!(s.handoff_items(), before);
			assert_eq!(s.workspace.dock_record.as_deref(), Some("website"));
			s.workspace.new_conversation = Some("draft".into());
			s.selected = Some("draft".into());
			assert_eq!(s.handoff_items(), before);
		});
	}

	#[gpui::test]
	fn graph_keeps_completed_prerequisites_and_hides_finished_branches(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.selected = Some("release".into());
			s.workspace.dock_record = Some("release".into());
			s.workspace.dock_completed = false;
			assert_eq!(
				s.collapse_graph_completed(&mut s.workspace_graph_full_layout()),
				0,
				"required completed prerequisites are not hidden"
			);
			let snapshot = s.snapshot.as_mut().unwrap();
			let mut isolated = snapshot.work_items.iter().find(|w| w.id == "flow").unwrap().clone();
			isolated.id = "finished-branch".into();
			snapshot.work_items.push(isolated);
			assert_eq!(s.collapse_graph_completed(&mut s.workspace_graph_full_layout()), 1);
			let layout = s.workspace_graph_layout();
			assert!(layout.nodes.iter().any(|n| n.id == "flow"));
			assert!(!layout.nodes.iter().any(|n| n.id == "finished-branch"));
			assert!(
				layout
					.edges
					.iter()
					.any(|(a, b)| layout.nodes[*a].id == "flow" && layout.nodes[*b].id == "verify")
			);
			assert!(
				layout.reports.is_empty(),
				"dependency chains must not be crossed by ownership lines"
			);
			s.workspace.dock_completed = true;
			assert!(s.workspace_graph_layout().nodes.iter().any(|n| n.id == "finished-branch"));
			s.selected = Some("verify".into());
			assert!(s.workspace_graph_layout().nodes.iter().any(|n| n.id == "flow"));
			s.snapshot.as_mut().unwrap().dependencies.clear();
			assert!(
				!s.workspace_graph_full_layout().reports.is_empty(),
				"without dependencies retain the simple parent-child graph"
			);
		});
	}

	#[gpui::test]
	fn marking_a_result_viewed_keeps_the_current_conversation(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = AgentSurface::new(cx);
			surface.visual_workspace_fixture(cx);
			surface.visual_dock_page("dock-result", cx);
			surface
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		let button = visual.debug_bounds("handoff-viewed").expect("viewed action");
		visual.simulate_click(button.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("release"));
			assert!(!s.handoff_items().iter().any(|h| h.work == "website"));
			assert!(s.handoff_items().iter().any(|h| h.work == "release"));
			assert_eq!(
				s.workspace.dock_record.as_deref(),
				Some("website"),
				"marking viewed keeps the current work scope"
			);
		});
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		let result =
			visual.debug_bounds("graph-node-website").expect("read result remains accessible");
		visual.simulate_click(result.center(), Default::default());
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		assert!(visual.debug_bounds("handoff-preview").is_some());
		assert!(visual.debug_bounds("handoff-viewed").is_none());
	}
}
