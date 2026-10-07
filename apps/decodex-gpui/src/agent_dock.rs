//! Source records for global handoff previews.
use gpui::Div;

use crate::shell::agent_surface::{
	AgentHistoryResult, AgentSnapshotDto, AgentSurface, AgentWorkItemDto, Context,
	InteractiveElement, ParentElement, Styled, graph, ui_theme::TEXT_MUTED,
};

#[derive(Default)]
pub(super) struct Evidence {
	key: Option<String>,
	history: Option<AgentHistoryResult>,
	request: Option<super::AgentRequestResult>,
	task: Option<gpui::Task<()>>,
}

impl AgentSurface {
	#[cfg(any(test, feature = "visual-capture"))]
	pub(super) fn visual_dock_page(&mut self, page: &str, cx: &mut Context<Self>) {
		self.selected = Some("release".into());
		self.handoffs.fixture();
		self.history = self.timeline.cache.get("agent").cloned().map(|h| ("release".into(), h));
		let snapshot = self.snapshot.as_mut().expect("fixture snapshot");
		let mut ready =
			snapshot.work_items.iter().find(|w| w.id == "flow").expect("fixture work").clone();
		ready.id = "website".into();
		ready.title = "Documentation update".into();
		ready.parent_goal_id = Some("agent".into());
		snapshot.work_items.push(ready);
		if let Some(mut history) = self.timeline.cache.get("verify").cloned() {
			if let AgentHistoryResult::Available { entries, .. } = &mut history {
				entries[0].text =
					"The setup guide now includes the new sign-in flow and recovery steps.".into();
			}
			self.timeline.cache.insert("website".into(), history);
		}
		self.workspace.dock_record = match page {
			"dock-result" => Some("website".into()),
			"dock-running" | "dock-dependencies" => Some("release".into()),
			_ => None,
		};
		if page == "dock-completed" {
			self.handoffs.observe("fixture-baseline".into(), snapshot);
			for work in &mut snapshot.work_items {
				work.dispatch_state = super::AgentDispatchStateDto::Idle;
			}
		}
		if page == "dock-dependencies" {
			self.handoffs.relations = true;
			self.workspace.dock_completed = true;
		}

		cx.notify();
	}

	pub(super) fn dock_scope(&self) -> Option<String> {
		if self.is_new_conversation() {
			return None;
		}
		self.selected.clone().or_else(|| self.root_id())
	}

	fn dock_evidence_key(&self, work: &AgentWorkItemDto) -> String {
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
		self.dock_evidence = Evidence::default();
		cx.notify();
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
		let Ok(owner) = super::EntityId::new(id) else { return };
		self.dock_evidence.key = Some(key.clone());
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
				let history = client.history(owner).await.ok()?;
				Some((history, request))
			})
		});
		self.dock_evidence.task = Some(cx.spawn(async move |surface, cx| {
			let (history, request) =
				request.await.unwrap_or((AgentHistoryResult::Unavailable, None));
			let _ = surface.update(cx, |s, cx| {
				if s.dock_evidence.key.as_ref() != Some(&key) {
					return;
				}
				let current = s.snapshot.as_ref().and_then(|snapshot| {
					snapshot
						.work_items
						.iter()
						.find(|w| Some(&w.id) == s.workspace.dock_record.as_ref())
				});
				if current.is_none_or(|work| s.dock_evidence_key(work) != key) {
					return;
				}
				s.dock_evidence.history = Some(history);
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
		let value: serde_json::Value = serde_json::from_str(request_json.as_str()).ok()?;
		let text = value["questions"]
			.as_array()
			.and_then(|q| q.first())
			.and_then(|q| q["question"].as_str())
			.or_else(|| value["reason"].as_str())
			.or_else(|| value["command"].as_str());
		text.map(report_excerpt)
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
				s.handoffs.relations = false;
				s.sync_request(cx);
				cx.notify();
			},
			cx,
		);
		let mut body = gpui::div()
			.w_full()
			.min_w_0()
			.pl_2()
			.py_1()
			.flex()
			.flex_col()
			.gap_2()
			.child(gpui::div().flex().child(source));
		let history = self.overview_history(work);
		let latest = if let Some(AgentHistoryResult::Available { entries, .. }) = history {
			entries.iter().rev().find(|entry| {
				source_matches(work, entry) && entry.kind == "assistant" && entry.activity.is_none()
			})
		} else {
			None
		};
		if let Some(entry) = latest {
			let date =
				time::OffsetDateTime::from_unix_timestamp(entry.created_at_micros / 1_000_000)
					.ok()
					.map(|d| format!("{} {:02}:{:02} UTC", d.date(), d.hour(), d.minute()))
					.unwrap_or_default();
			body = body
				.child(
					gpui::div()
						.text_color(gpui::rgb(TEXT_MUTED))
						.child(format!("Saved report · {date}")),
				)
				.child(
					gpui::div()
						.w_full()
						.min_w_0()
						.debug_selector(|| "dock-report-excerpt".into())
						.child(report_excerpt(&entry.text)),
				);
		} else {
			let loading = self.dock_evidence.key.as_ref() == Some(&self.dock_evidence_key(work))
				&& self.dock_evidence.task.is_some();
			body = body.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(if loading {
				"Loading latest report…"
			} else {
				"No saved report is available for this task."
			}));
		}
		body
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
	use crate::ui_theme::{AMBER, BLUE, GREEN};
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
			AMBER,
			0,
		)
	} else if work.dispatch_state == Dispatch::Running {
		(
			"Running",
			"Execution is active. Open the conversation for live activity.".into(),
			GREEN,
			1,
		)
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
			TEXT_MUTED,
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
			("Update pending", "A recorded update has not been processed yet. It is not a request for your approval.".into(), BLUE, 2)
		} else {
			match work.status {
				Status::FollowUp => (
					"Follow-up pending",
					"A follow-up is recorded, but no execution is active.".into(),
					BLUE,
					2,
				),
				Status::Wait => (
					"Waiting",
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

fn report_excerpt(text: &str) -> String {
	let paragraph = text.split("\n\n").find(|part| !part.trim().is_empty()).unwrap_or("");
	let plain = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
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

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::AppContext;

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
		let row = visual.debug_bounds("handoff-release").expect("handoff chip");
		visual.simulate_click(row.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.workspace.dock_record.as_deref(), Some("release"));
		});
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		let source = visual.debug_bounds("handoff-open-request").expect("source link");
		visual.simulate_click(source.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("release"));
			assert_eq!(s.dock_scope().as_deref(), Some("release"));
			assert!(s.workspace.dock_record.is_none());
		});
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
	fn long_report_preview_stays_bounded_above_the_strip(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = AgentSurface::new(cx);
			surface.visual_workspace_fixture(cx);
			surface.visual_dock_page("dock-result", cx);
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
		assert!(excerpt.size.height < gpui::px(100.));
		let strip = visual.debug_bounds("handoff-strip").expect("handoff strip");
		assert!(detail.bottom() <= strip.top());
		surface.update(visual, |s, _| assert_eq!(s.selected.as_deref(), Some("release")));
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
			assert!(layout.reports.is_empty());
			s.workspace.dock_completed = true;
			assert!(s.workspace_graph_layout().nodes.iter().any(|n| n.id == "finished-branch"));
			s.selected = Some("verify".into());
			assert!(s.workspace_graph_layout().nodes.iter().any(|n| n.id == "flow"));
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
			assert!(s.workspace.dock_record.is_none());
		});
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		let result =
			visual.debug_bounds("handoff-website").expect("read result remains accessible");
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
