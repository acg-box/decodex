//! Work facts and source records in the resizable bottom dock.
use crate::ui_scroll::SmoothScrollArea;
use gpui::{AnyElement, Div};

use crate::shell::agent_surface::{
	AgentHistoryResult, AgentSnapshotDto, AgentSurface, AgentWorkItemDto, Context,
	InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled, graph,
	ui_theme::TEXT_MUTED,
};

#[derive(Default)]
pub(super) struct Evidence {
	key: Option<String>,
	history: Option<AgentHistoryResult>,
	task: Option<gpui::Task<()>>,
}

impl AgentSurface {
	#[cfg(any(test, feature = "visual-capture"))]
	pub(super) fn visual_dock_page(&mut self, page: &str, cx: &mut Context<Self>) {
		self.selected = Some("release".into());
		self.workspace.dock_compact = page == "dock-compact";
		self.workspace.graph_panel_height = 300.;
		self.history = self.timeline.cache.get("agent").cloned().map(|h| ("release".into(), h));
		if page == "dock-result" {
			self.workspace.dock_record = Some("verify".into());
		}
		if page == "dock-completed" {
			for work in &mut self.snapshot.as_mut().expect("fixture snapshot").work_items {
				work.status = super::AgentWorkStatusDto::Resolved;
				work.dispatch_state = super::AgentDispatchStateDto::Idle;
			}
		}
		if page == "dock-dependencies" {
			self.workspace.dock_relations = true;
			self.workspace.graph_scope = Some("release".into());
		}
		cx.notify();
	}

	pub(super) fn dock_scope(&self) -> Option<String> {
		if self.is_new_conversation() {
			return None;
		}
		self.selected.clone().or_else(|| self.root_id())
	}

	pub(super) fn dock_has_dependencies(&self) -> bool {
		self.snapshot.as_ref().is_some_and(|snapshot| {
			let work = scoped_work(snapshot, self.dock_scope().as_deref());
			snapshot.dependencies.iter().any(|edge| work.iter().any(|w| w.id == edge.work_item_id))
		})
	}

	pub(super) fn dock_summary(&self) -> String {
		let Some(snapshot) = &self.snapshot else {
			return "Connecting…".into();
		};
		let work = scoped_work(snapshot, self.dock_scope().as_deref());
		if work.is_empty() {
			return "No task started".into();
		}
		let mut counts = [0; 4];
		for item in &work {
			counts[progress_state(snapshot, item).group as usize] += 1;
		}
		if counts[0] + counts[1] + counts[2] == 0 {
			return format!("{} marked complete", counts[3]);
		}
		format!("{} need attention · {} running · {} waiting", counts[0], counts[1], counts[2])
	}

	pub(super) fn work_overview(&self, cx: &mut Context<Self>) -> AnyElement {
		let Some(snapshot) = &self.snapshot else { return gpui::div().into_any_element() };
		let mut work = scoped_work(snapshot, self.dock_scope().as_deref());
		// Keep rows stable as updates arrive; only a change of work state moves a row.
		work.sort_by_key(|w| {
			(progress_state(snapshot, w).group, w.created_at_micros, w.id.as_str())
		});
		let mut body = gpui::div()
			.id("work-overview")
			.debug_selector(|| "work-overview".into())
			.flex()
			.flex_col()
			.flex_1()
			.h_0()
			.min_h_0()
			.overflow_y_scroll()
			.px_2()
			.pb_2()
			.text_size(gpui::px(12.));
		let active = work.iter().filter(|w| progress_state(snapshot, w).group == 1).count();
		let attention = work.iter().filter(|w| progress_state(snapshot, w).group == 0).count();
		let waiting = work.iter().filter(|w| progress_state(snapshot, w).group == 2).count();
		let complete = work.iter().filter(|w| progress_state(snapshot, w).group == 3).count();
		let summary = if work.is_empty() {
			"Work will appear here when this conversation starts a task.".into()
		} else if active + attention + waiting == 0 {
			"No work is running. Completed work is available below.".into()
		} else {
			format!("{active} running · {waiting} waiting · {attention} need attention")
		};
		body = body
			.child(gpui::div().flex_none().py_2().text_color(gpui::rgb(TEXT_MUTED)).child(summary));
		for item in work.iter().filter(|w| progress_state(snapshot, w).group != 3) {
			body = body.child(self.overview_record(snapshot, item, cx));
		}
		if complete > 0 {
			body = body.child(gpui::div().flex_none().flex().pt_2().child(self.workspace_action(
				"dock-completed".into(),
				format!(
					"{} completed work ({complete})",
					if self.workspace.dock_completed { "Hide" } else { "Show" }
				),
				|s, cx| {
					s.workspace.dock_completed = !s.workspace.dock_completed;
					cx.notify();
				},
				cx,
			)));
			if self.workspace.dock_completed {
				for item in work.iter().filter(|w| progress_state(snapshot, w).group == 3) {
					body = body.child(self.overview_record(snapshot, item, cx));
				}
			}
		}
		body.smooth_scroll("work-overview-scroll").into_any_element()
	}

	fn overview_record(
		&self,
		snapshot: &AgentSnapshotDto,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Div {
		let state = progress_state(snapshot, work);
		let id = work.id.clone();
		let open = self.workspace.dock_record.as_ref() == Some(&id);
		let mut record =
			gpui::div().flex_none().min_w_0().w_full().flex().flex_col().py_1().gap_1().child(
				gpui::div()
					.flex()
					.items_center()
					.gap_2()
					.child(self.workspace_action(
						format!("dock-record-{id}"),
						format!("{} {}", if open { "▾" } else { "▸" }, self.work_label(work)),
						move |s, cx| s.toggle_dock_record(&id, cx),
						cx,
					))
					.child(gpui::div().text_color(gpui::rgb(state.color)).child(state.label)),
			);
		if state.group != 1 {
			record = record
				.child(gpui::div().pl_2().text_color(gpui::rgb(TEXT_MUTED)).child(state.reason));
		}
		if open {
			record = record.child(self.overview_evidence(work, cx));
		}
		record
	}

	fn dock_evidence_key(&self, work: &AgentWorkItemDto) -> String {
		serde_json::json!([
			work.id,
			work.codex_thread_id,
			self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref())
		])
		.to_string()
	}

	fn toggle_dock_record(&mut self, id: &str, cx: &mut Context<Self>) {
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
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(super::AgentClient::new(profile).history(owner)).ok()
		});
		self.dock_evidence.task = Some(cx.spawn(async move |surface, cx| {
			let history = request.await.unwrap_or(AgentHistoryResult::Unavailable);
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

	fn overview_evidence(&self, work: &AgentWorkItemDto, cx: &mut Context<Self>) -> Div {
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
						.child(format!("Latest report · {date}")),
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
		if let Some(AgentHistoryResult::Available { entries, .. }) = history
			&& let Some(activity) = entries
				.iter()
				.rev()
				.filter(|e| source_matches(work, e))
				.find_map(|e| e.activity.as_ref())
		{
			body = body.child(self.detail_row(
				work,
				activity,
				gpui::div().child(format!(
					"Last recorded action: {} · {}",
					activity.label, activity.status
				)),
				cx,
			));
		}
		body
	}
}

struct ProgressState {
	label: &'static str,
	reason: String,
	color: u32,
	group: u8,
}

fn progress_state(snapshot: &AgentSnapshotDto, work: &AgentWorkItemDto) -> ProgressState {
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
	let plain = text.split_whitespace().collect::<Vec<_>>().join(" ");
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

fn scoped_work<'a>(
	snapshot: &'a AgentSnapshotDto,
	scope: Option<&str>,
) -> Vec<&'a AgentWorkItemDto> {
	let Some(scope) = scope else { return Vec::new() };
	let mut ids = std::collections::BTreeSet::from([scope]);
	loop {
		let count = ids.len();
		for work in &snapshot.work_items {
			if work.parent_goal_id.as_deref().is_some_and(|parent| ids.contains(parent)) {
				ids.insert(work.id.as_str());
			}
		}
		if ids.len() == count {
			break;
		}
	}
	snapshot.work_items.iter().filter(|w| ids.contains(w.id.as_str())).collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::AppContext;

	#[gpui::test]
	fn dock_scope_keeps_nested_work_and_excludes_other_roots(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let snapshot = s.snapshot.as_mut().unwrap();
			let mut nested = snapshot.work_items.iter().find(|w| w.id == "verify").unwrap().clone();
			nested.id = "nested".into();
			nested.parent_goal_id = Some("verify".into());
			snapshot.work_items.push(nested.clone());
			nested.id = "unrelated".into();
			nested.parent_goal_id = None;
			snapshot.work_items.push(nested);
			let ids = scoped_work(snapshot, Some("release"))
				.iter()
				.map(|w| w.id.as_str())
				.collect::<Vec<_>>();
			assert!(ids.contains(&"nested"));
			assert!(!ids.contains(&"unrelated"));
			assert!(!ids.contains(&"agent"));
			assert!(scoped_work(snapshot, None).is_empty());
		});
	}
	#[gpui::test]
	fn dock_expands_in_place_and_source_navigation_follows_conversation(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = AgentSurface::new(cx);
			surface.visual_workspace_fixture(cx);
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
		let row = visual.debug_bounds("dock-record-release").expect("overview row");
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
		let source = visual.debug_bounds("dock-source-release").expect("source link");
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
	fn long_report_stays_bounded_and_work_scrolls_inside_the_panel(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = AgentSurface::new(cx);
			surface.visual_workspace_fixture(cx);
			surface
		});
		surface.update(visual, |s, _| {
			s.workspace.dock_record = Some("verify".into());
			let Some(AgentHistoryResult::Available { entries, .. }) =
				s.timeline.cache.get_mut("verify")
			else {
				panic!("fixture history")
			};
			entries[0].text = format!("{}\n1. **原生下属草稿丢失。** 输入未发送文字后返回 Chief，再打开下属，草稿被空串或旧内容覆盖。\n2. **全局后退不能恢复原生下属对话。** 连续进入同一 owner 的下属，历史只记录 owner。\n\n{}", "Saved evidence line.\n\n".repeat(100), "Saved evidence line.\n\n".repeat(100));
			entries[0].id = 999;
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}

		let bounds = visual.debug_bounds("work-overview").unwrap();
		let excerpt = visual.debug_bounds("dock-report-excerpt").expect("bounded report excerpt");
		assert!(excerpt.size.height < gpui::px(180.), "report must not become a second transcript");
		let before = visual.debug_bounds("dock-record-release").unwrap();
		assert!(bounds.size.height < gpui::px(400.));
		visual.simulate_event(gpui::ScrollWheelEvent {
			position: bounds.center(),
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100.))),
			..Default::default()
		});
		visual.update(|w, cx| {
			w.refresh();
			w.draw(cx).clear();
		});
		let after = visual.debug_bounds("dock-record-release").unwrap();
		let after_bounds = visual.debug_bounds("work-overview").unwrap();
		assert_eq!(
			(before.top() - bounds.top()) - (after.top() - after_bounds.top()),
			gpui::px(100.)
		);
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
	fn progress_scope_follows_selection_and_drafts_have_no_unrelated_work(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.open_page("verify", cx);
			assert_eq!(s.dock_scope().as_deref(), Some("verify"));
			assert_eq!(
				scoped_work(s.snapshot.as_ref().unwrap(), s.dock_scope().as_deref()).len(),
				1
			);
			s.workspace.new_conversation = Some("draft".into());
			assert_eq!(s.dock_scope().as_deref(), Some("verify"));
			s.selected = Some("draft".into());
			assert!(s.dock_scope().is_none());
		});
	}

	#[gpui::test]
	fn completed_work_is_hidden_until_requested_and_compact_bar_keeps_conversation(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut surface = AgentSurface::new(cx);
			surface.visual_workspace_fixture(cx);
			surface.visual_dock_page("dock-completed", cx);
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
		assert!(visual.debug_bounds("dock-record-verify").is_none());
		let button = visual.debug_bounds("dock-completed").unwrap();
		visual.simulate_click(button.center(), Default::default());
		surface.update(visual, |s, _| {
			assert!(s.workspace.dock_completed, "completion disclosure click at {button:?}")
		});
		for _ in 0..3 {
			visual.update(|w, cx| {
				w.refresh();
				w.draw(cx).clear();
			});
			visual.run_until_parked();
		}
		assert!(visual.debug_bounds("dock-record-flow").is_some());
		let button = visual.debug_bounds("dock-toggle").unwrap();
		visual.simulate_click(button.center(), Default::default());
		surface.update(visual, |s, _| {
			assert!(s.workspace.dock_compact);
			assert!(!s.workspace.graph_expanded);
		});
	}
}
