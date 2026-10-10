//! Native-shaped fixture and execution identity regressions.
use super::{
	AgentActivityDto, AgentSurface, AgentTimelineContent, AgentTimelineResult, Context, Execution,
};
use decodex_protocol::{AgentTimelineEntry, AgentTimelinePage, EntityId};
#[cfg(test)] use gpui::AppContext as _;

fn message(turn: &str, kind: &str, text: &str) -> AgentTimelineContent {
	AgentTimelineContent::Item {
		collaboration: None,
		turn_id: turn.into(),
		item_id: format!("{turn}-{kind}"),
		kind: kind.into(),
		phase: (kind == "agentMessage").then(|| "final_answer".into()),
		text: text.into(),
		truncated: false,
		activity: None,
		app_ui: false,
		attachments: vec![],
	}
}

fn step(turn: &str, id: &str, label: &str, status: &str) -> AgentTimelineContent {
	let mut item = message(turn, "commandExecution", "");
	if let AgentTimelineContent::Item { item_id, activity, .. } = &mut item {
		*item_id = id.into();
		*activity = Some(AgentActivityDto {
			turn_id: turn.into(),
			item_id: id.into(),
			kind: "commandExecution".into(),
			status: status.into(),
			label: label.into(),
			detail: String::new(),
			plugin_id: None,
			read_only_hint: None,
			native_timestamp_ms: None,
			duration_ms: Some(1_200),
		});
	}
	item
}

fn page(items: Vec<AgentTimelineContent>) -> AgentTimelineResult {
	AgentTimelineResult::Available {
		work_id: EntityId::new("website").expect("valid execution fixture"),
		account_id: EntityId::new("account").expect("valid execution fixture"),
		page: AgentTimelinePage {
			thread_id: "thread".into(),
			safety_buffering_turn_id: None,
			entries: items
				.into_iter()
				.enumerate()
				.map(|(index, content)| AgentTimelineEntry { position: index as u64, content })
				.collect(),
			next_cursor: None,
			weather: Default::default(),
			active_realtime_session_at_page_start: None,
		},
	}
}

impl AgentSurface {
	pub(crate) fn visual_execution_dock(&mut self, cx: &mut Context<Self>) {
		self.visual_dock_page("dock-result", cx);
		self.workspace.graph_panel_height = 380.;
		self.handoffs.focus = Some("website".into());
		let work = self
			.snapshot
			.as_mut()
			.expect("valid execution fixture")
			.work_items
			.iter_mut()
			.find(|work| work.id == "website")
			.expect("valid execution fixture");
		work.title = "Sign-in recovery".into();
		work.active_turn_id = Some("retry".into());
		work.dispatch_state = super::super::AgentDispatchStateDto::Running;
		let result = page(vec![
			message("first", "userMessage", "Fix canceling sign-in"),
			step("first", "read", "Read sign-in state handling", "completed"),
			step("first", "patch", "Update cancellation handler", "completed"),
			step("first", "test", "Run cancellation regression test", "failed"),
			message(
				"first",
				"agentMessage",
				"Canceling closes the dialog, but the sign-in button stays disabled. The regression test still fails.",
			),
			AgentTimelineContent::TurnBoundary {
				turn_id: "first".into(),
				completed: true,
				status: Some("completed".into()),
				duration_ms: Some(84_000),
				usage_summary: None,
				usage: None,
				error: None,
			},
			message("retry", "userMessage", "Fix the disabled button and verify again"),
			AgentTimelineContent::TurnBoundary {
				turn_id: "retry".into(),
				completed: false,
				status: None,
				duration_ms: None,
				usage_summary: None,
				usage: None,
				error: None,
			},
			step("retry", "inspect", "Inspect the failed assertion", "completed"),
			step("retry", "edit", "Reset button state when sign-in is canceled", "completed"),
			step("retry", "test", "Run cancellation regression test", "running"),
		]);
		self.dock_evidence.execution = Execution::read(&result, "website", "thread");
		cx.notify();
	}

	pub(crate) fn visual_execution_failure(&mut self, cx: &mut Context<Self>) {
		self.visual_execution_dock(cx);
		let snapshot = self.snapshot.as_mut().expect("fixture snapshot");
		snapshot.runtime_source = Some(EntityId::new("source").expect("source id"));
		snapshot
			.work_items
			.iter_mut()
			.find(|work| work.id == "website")
			.expect("fixture work")
			.codex_thread_id = Some("thread".into());
		self.dock_evidence
			.execution
			.as_mut()
			.expect("execution fixture")
			.runs
			.retain(|run| run.id == "first");
		self.snapshot
			.as_mut()
			.expect("snapshot")
			.work_items
			.iter_mut()
			.find(|work| work.id == "website")
			.expect("work")
			.dispatch_state = super::super::AgentDispatchStateDto::Idle;
		let key = self
			.activity_detail_key(&("website".into(), "first".into(), "test".into()))
			.expect("fixture activity binding");
		self.activity_detail.value = Some((key, Some(decodex_protocol::AgentActivityDetailResult::Available {
			text: "FAIL sign_in_cancel_restores_button\nExpected: button.enabled = true\nActual: button.enabled = false\nExit code: 1".into(),
			truncated: false, offset: 0, next: None,
		})));
	}
}

#[cfg(test)]
mod checks {
	use super::*;

	#[test]
	fn native_runs_keep_retries_and_failures_separate() {
		let result = page(vec![
			step("first", "same", "Test", "running"),
			step("first", "same", "Test", "failed"),
			step("retry", "same", "Test", "completed"),
		]);
		let execution =
			Execution::read(&result, "website", "thread").expect("valid execution fixture");
		assert_eq!(execution.runs.len(), 2);
		assert_eq!(execution.runs[0].steps.len(), 1);
		assert_eq!(execution.runs[0].steps[0].status, "failed");
		assert_eq!(execution.runs[1].steps[0].status, "completed");
		assert!(Execution::read(&result, "other", "thread").is_none());
		assert!(Execution::read(&result, "website", "other").is_none());
	}

	#[test]
	fn delegated_input_does_not_become_a_tool_step() {
		let mut input = step("turn", "instruction", "Tool result", "completed");
		if let AgentTimelineContent::Item { kind, text, .. } = &mut input {
			*kind = "agentInput".into();
			*text = "Review sign-in recovery".into();
		}
		let execution =
			Execution::read(&page(vec![input]), "website", "thread").expect("native page");
		assert!(execution.runs[0].steps.is_empty());
	}

	#[test]
	fn summary_and_partial_pages_do_not_invent_execution() {
		let summary = AgentTimelineResult::Summary {
			work_id: EntityId::new("website").expect("valid execution fixture"),
			account_id: EntityId::new("account").expect("valid execution fixture"),
			thread_id: "thread".into(),
			items: vec![step("first", "same", "Test", "completed")],
		};
		assert!(Execution::read(&summary, "website", "thread").is_none());
		let mut result = page(vec![step("first", "test", "Test", "completed")]);
		if let AgentTimelineResult::Available { page, .. } = &mut result {
			page.next_cursor = Some("older".into());
		}
		let execution =
			Execution::read(&result, "website", "thread").expect("valid execution fixture");
		assert_eq!(
			execution.runs[0].status, "Recorded",
			"no terminal boundary means unknown run outcome"
		);
	}

	#[gpui::test]
	fn execution_evidence_stays_inside_the_work_node(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = AgentSurface::new(cx);
			s.visual_workspace_fixture(cx);
			s.visual_execution_failure(cx);
			s.clear_activity_detail();
			s
		});
		visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
		visual.update(|w, cx| {
			w.refresh();
			w.draw(cx).clear(cx);
		});
		visual.run_until_parked();
		assert!(visual.debug_bounds("dock-run-steps").is_none(), "no separate execution browser");
		assert!(
			visual.debug_bounds("dock-step-first-read").is_none(),
			"successful operations are collapsed"
		);
		let node = visual.debug_bounds("graph-node-website").expect("work node");
		let details = visual.debug_bounds("handoff-preview").expect("node contents");
		assert!(details.top() >= node.top() && details.bottom() <= node.bottom());
		let before = visual.debug_bounds("work-dock").expect("Dock");
		let close = visual.debug_bounds("handoff-close").expect("close node");
		visual.simulate_click(close.center(), Default::default());
		visual.update(|w, cx| {
			w.refresh();
			w.draw(cx).clear(cx);
		});
		assert_eq!(before, visual.debug_bounds("work-dock").expect("same Dock"));
		surface.update(visual, |s, _| {
			assert!(s.handoffs.focus.is_none());
			assert_eq!(s.selected.as_deref(), Some("release"));
		});
	}
}

#[gpui::test]
fn dock_uses_live_entries_only_for_the_exact_conversation(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(|cx| {
		let mut s = AgentSurface::new(cx);
		s.visual_workspace_fixture(cx);
		s
	});
	surface.update(cx, |s, _| {
		let mut work = s.snapshot.as_ref().unwrap().work_items[0].clone();
		work.id = "website".into();
		work.codex_thread_id = Some("thread".into());
		s.timeline.native.binding = Some(super::super::native_timeline::Binding {
			work: "website".into(),
			thread: "thread".into(),
			account: "account".into(),
		});
		let AgentTimelineResult::Available { page, .. } =
			page(vec![message("latest", "agentMessage", "Current result")])
		else {
			unreachable!()
		};
		s.timeline.native.entries = page.entries;
		assert_eq!(
			s.live_dock_execution(&work).unwrap().runs[0].result.as_deref(),
			Some("Current result")
		);
		work.codex_thread_id = Some("other-thread".into());
		assert!(s.live_dock_execution(&work).is_none());
	});
}

#[gpui::test]
fn a_previous_task_cannot_enter_the_current_graph(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(|cx| {
		let mut s = AgentSurface::new(cx);
		s.visual_workspace_fixture(cx);
		s.visual_dock_page("dock-running", cx);
		s
	});
	surface.update(cx, |s, _| {
		s.handoffs.focus = Some("website".into());
		let layout = s.workspace_graph_full_layout();
		assert!(!layout.nodes.iter().any(|node| node.id == "website"));
		assert!(layout.nodes.iter().any(|node| node.id == "verify"));
	});
}

#[gpui::test]
fn source_action_reveals_the_current_conversation_from_fullscreen(cx: &mut gpui::TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut s = AgentSurface::new(cx);
		s.visual_workspace_fixture(cx);
		s.visual_dock_page("dock-result", cx);
		s.selected = Some("website".into());
		s.workspace.graph_expanded = true;
		s
	});
	visual.simulate_resize(gpui::size(gpui::px(1248.), gpui::px(840.)));
	for _ in 0..3 {
		visual.update(|window, cx| {
			window.refresh();
			window.draw(cx).clear(cx);
		});
		visual.run_until_parked();
	}
	let source = visual.debug_bounds("dock-open-source").expect("source action");
	visual.simulate_click(source.center(), Default::default());
	surface.update(visual, |s, _| {
		assert_eq!(s.selected.as_deref(), Some("website"));
		assert!(!s.workspace.graph_expanded);
	});
}

#[test]
fn result_preview_keeps_the_finding_after_an_introductory_paragraph() {
	let result = page(vec![message(
		"done",
		"agentMessage",
		"One issue needs a fix:\n\nP2: Opening the source leaves the conversation hidden.",
	)]);
	let execution = Execution::read(&result, "website", "thread").unwrap();
	assert!(execution.runs[0].result.as_deref().unwrap().contains("conversation hidden"));
}
