//! Work facts and source records in the resizable bottom dock.
use crate::ui_scroll::SmoothScrollArea;
use gpui::{AnyElement, Div};

use crate::shell::agent_surface::{
	AgentHistoryResult, AgentSnapshotDto, AgentSurface, AgentWorkItemDto, Context,
	InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled, graph,
	markdown, ui_theme::TEXT_MUTED,
};

#[derive(Default)]
pub(super) struct Evidence {
	key: Option<String>,
	history: Option<AgentHistoryResult>,
	task: Option<gpui::Task<()>>,
}

impl AgentSurface {
	pub(super) fn work_overview(&self, cx: &mut Context<Self>) -> AnyElement {
		let Some(snapshot) = &self.snapshot else { return gpui::div().into_any_element() };
		let scope = self.workspace.graph_scope.clone().or_else(|| self.root_id());
		let mut work = scoped_work(snapshot, scope.as_deref());
		work.sort_by_key(|w| {
			let state = overview_state(snapshot, w).0;
			(
				match state {
					"Needs you" | "Needs attention" => 0,
					"Blocked" => 1,
					"Running" | "Starting" => 2,
					"Follow-up" | "Waiting" | "Open" => 3,
					_ => 4,
				},
				std::cmp::Reverse(w.updated_at_micros),
			)
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
		if work.is_empty() {
			body = body.child(
				gpui::div().py_2().text_color(gpui::rgb(TEXT_MUTED)).child("No work records yet."),
			);
		}
		let running = work
			.iter()
			.filter(|w| {
				matches!(
					w.dispatch_state,
					super::AgentDispatchStateDto::Running
						| super::AgentDispatchStateDto::Dispatching
				)
			})
			.count();
		let blocked = work.iter().filter(|w| graph::state_in(snapshot, w).0 == "Blocked").count();
		body = body.child(
			gpui::div()
				.py_2()
				.text_color(gpui::rgb(TEXT_MUTED))
				.child(format!("Work records: {running} active · {blocked} blocked")),
		);
		let mut threads: std::collections::BTreeSet<_> =
			work.iter().filter_map(|w| w.codex_thread_id.clone()).collect();
		for item in work {
			body = body.child(self.overview_record(snapshot, item, cx));
			for agent in self.native_agents.lists.get(&item.id).into_iter().flatten() {
				if !threads.insert(agent.thread_id.clone()) {
					continue;
				}
				let owner = item.id.clone();
				let thread = agent.thread_id.clone();
				body = body.child(gpui::div().py_1().child(self.workspace_action(
					format!("dock-native-{thread}"),
					format!("{} · {}", agent.title, agent.status),
					move |s, cx| s.open_native_agent(&owner, &thread, cx),
					cx,
				)));
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
		let (status, color) = overview_state(snapshot, work);
		let id = work.id.clone();
		let open = self.workspace.dock_record.as_ref() == Some(&id);
		let events = snapshot.pending_events.iter().filter(|e| e.work_item_id == id).count();
		let blockers =
			graph::blockers(snapshot, work).iter().map(|w| self.work_label(w)).collect::<Vec<_>>();
		let label = self.work_label(work);
		let mut record =
			gpui::div().flex_none().min_w_0().w_full().flex().flex_col().py_1().gap_1().child(
				gpui::div()
					.flex()
					.items_center()
					.gap_2()
					.child(gpui::div().text_color(gpui::rgb(color)).child(status))
					.child(self.workspace_action(
						format!("dock-record-{id}"),
						label,
						move |s, cx| s.toggle_dock_record(&id, cx),
						cx,
					)),
			);
		if !blockers.is_empty() {
			record = record.child(
				gpui::div()
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(format!("Waiting for {}", blockers.join(", "))),
			);
		}
		if events > 0 {
			record = record.child(
				gpui::div()
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(format!("{events} pending event{}", if events == 1 { "" } else { "s" })),
			);
		}
		if !open
			&& let Some(AgentHistoryResult::Available { entries, .. }) = self.overview_history(work)
			&& let Some(entry) = entries
				.iter()
				.rev()
				.find(|entry| entry.kind == "assistant" && source_matches(work, entry))
		{
			record = record.child(
				gpui::div()
					.text_color(gpui::rgb(TEXT_MUTED))
					.overflow_hidden()
					.text_ellipsis()
					.whitespace_nowrap()
					.child(format!(
						"Latest saved response: {}",
						entry.text.lines().next().unwrap_or_default()
					)),
			);
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
		let history = self.overview_history(work);
		let id = work.id.clone();
		let source = self.workspace_action(
			format!("dock-source-{id}"),
			"Open conversation".into(),
			move |s, cx| {
				let scope = s.workspace.graph_scope.clone();
				s.open_page(&id, cx);
				s.workspace.graph_scope = scope;
				s.workspace.dock_record = Some(id.clone());
				cx.notify();
			},
			cx,
		);
		let mut body = gpui::div().min_w_0().pl_2().py_1().flex().flex_col().gap_2().child(source);
		let mut found = false;
		if let Some(AgentHistoryResult::Available { entries, .. }) = history {
			// Saved text is evidence of what was reported, never a verification verdict.
			for entry in entries
				.iter()
				.rev()
				.filter(|entry| {
					source_matches(work, entry)
						&& (entry.activity.is_some() || entry.kind == "assistant")
				})
				.take(3)
			{
				found = true;
				if let Some(activity) = &entry.activity {
					body = body
						.child(self.detail_row(
							work,
							activity,
							gpui::div().child(format!("{} · {}", activity.label, activity.status)),
							cx,
						))
						.child(markdown::render_process(
							&activity.detail,
							&format!("dock-activity-{}-{}", work.id, entry.id),
						));
				} else {
					body = body
						.child(
							gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child("Saved response"),
						)
						.child(markdown::render_process(
							&entry.text,
							&format!("dock-response-{}-{}", work.id, entry.id),
						));
				}
			}
		}
		if !found {
			body = body.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child(
				if self.dock_evidence.key.as_ref() == Some(&self.dock_evidence_key(work))
					&& self.dock_evidence.task.is_some()
				{
					"Loading source records…"
				} else {
					"No source records are available here. Open the conversation to inspect it."
				},
			));
		}
		body
	}
}

fn overview_state(snapshot: &AgentSnapshotDto, work: &AgentWorkItemDto) -> (&'static str, u32) {
	if snapshot.pending_events.iter().any(|event| {
		event.work_item_id == work.id
			&& ["permission_pending", "user_input_pending", "server_request_pending"]
				.contains(&event.event_kind.as_str())
	}) {
		("Needs you", crate::ui_theme::AMBER)
	} else {
		graph::state_in(snapshot, work)
	}
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
	fn dock_expands_in_place_and_source_navigation_keeps_scope(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| s.visual_workspace_fixture(cx));
		visual.update(|w, cx| w.draw(cx).clear());
		let row = visual.debug_bounds("dock-record-release").expect("overview row");
		visual.simulate_click(row.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.workspace.dock_record.as_deref(), Some("release"));
		});
		visual.update(|w, cx| w.draw(cx).clear());
		let source = visual.debug_bounds("dock-source-release").expect("source link");
		visual.simulate_click(source.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("release"));
			assert_eq!(s.workspace.graph_scope.as_deref(), Some("release"));
			assert_eq!(s.workspace.dock_record.as_deref(), Some("release"));
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
	fn long_dock_evidence_scrolls_inside_the_reserved_panel(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.dock_record = Some("verify".into());
			let Some(AgentHistoryResult::Available { entries, .. }) =
				s.timeline.cache.get_mut("verify")
			else {
				panic!("fixture history")
			};
			entries[0].text = "Saved evidence line.\n\n".repeat(100);
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		visual.update(|w, cx| w.draw(cx).clear());

		let bounds = visual.debug_bounds("work-overview").unwrap();
		let before = visual.debug_bounds("dock-record-release").unwrap();
		assert!(bounds.size.height < gpui::px(400.));
		visual.simulate_event(gpui::ScrollWheelEvent {
			position: bounds.center(),
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100.))),
			..Default::default()
		});
		visual.update(|w, cx| w.draw(cx).clear());
		let after = visual.debug_bounds("dock-record-release").unwrap();
		assert_eq!(before.top() - after.top(), gpui::px(100.));
	}
}
