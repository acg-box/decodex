use super::AgentSurface;
use gpui::TestAppContext;

#[gpui::test]
fn new_assistant_topic_reuses_an_unsent_draft_after_navigation(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		let snapshot = s.snapshot.as_mut().unwrap();
		for work in &mut snapshot.work_items {
			if work.id == "agent" {
				work.codex_thread_id = Some("started-general".into());
			}
			if work.id == "topic-research" {
				work.codex_thread_id = None;
				work.active_turn_id = None;
				work.status = decodex_protocol::AgentWorkStatusDto::Wait;
				work.dispatch_state = decodex_protocol::AgentDispatchStateDto::Idle;
			}
		}
		s.open_navigation_conversation("topic-research", cx);
		s.composer.update(cx, |input, cx| input.set_content("Unsent research request", cx));
		s.open_navigation_conversation("agent", cx);
		let count = s.snapshot.as_ref().unwrap().work_items.len();
		for _ in 0..3 {
			s.new_assistant_conversation(cx);
		}
		assert_eq!(s.selected.as_deref(), Some("topic-research"));
		assert_eq!(s.composer.read(cx).content(), "Unsent research request");
		assert_eq!(s.snapshot.as_ref().unwrap().work_items.len(), count);
		assert!(s.workspace.assistant_open.is_none());
		assert!(!s.sending);
	});
}

#[gpui::test]
fn sidebar_contains_all_outcomes_and_searches_collapsed_sections(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		let snapshot = s.snapshot.as_mut().unwrap();
		let seed = snapshot.portfolio.works[0].clone();
		for number in 0..10 {
			let mut work = seed.clone();
			work.id = format!("extra-{number}");
			work.title = format!("Outcome {number}");
			work.state = decodex_protocol::work::WorkState::Completed;
			snapshot.portfolio.works.push(work);
		}
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("navigation-work-extra-9").is_some());
	assert!(visual.debug_bounds("project-work").is_none());
	assert!(visual.debug_bounds("new-personal-work").is_none());
	let toggle = visual.debug_bounds("navigation-toggle-work").unwrap();
	visual.simulate_click(toggle.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("navigation-work-extra-9").is_none());
	surface.update(visual, |s, cx| {
		s.work_search.update(cx, |input, cx| input.set_content("Outcome 9", cx));
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("navigation-work-extra-9").is_some());
	assert!(visual.debug_bounds("navigation-work-extra-8").is_none());
	assert!(visual.debug_bounds("assistant-topic-agent").is_none());
}

#[gpui::test]
fn sidebar_delete_requires_confirmation_and_keeps_the_selected_draft(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		s.workspace.work_detail = None;
		s.composer.update(cx, |input, cx| input.set_content("Keep my draft", cx));
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| w.id == "topic-research")
			.unwrap()
			.codex_thread_id = Some("native-research".into());
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let more = visual.debug_bounds("conversation-actions-topic-research").unwrap();
	visual.simulate_click(more.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	let delete = visual.debug_bounds("conversation-confirm-topic-research").unwrap();
	visual.simulate_click(delete.center(), Default::default());
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Keep my draft");
		assert!(s.workspace.navigation.confirm_delete);
		assert!(s.workspace.navigation.mutation.is_none());
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("conversation-delete-topic-research").is_some());
	assert!(visual.debug_bounds("conversation-cancel-topic-research").is_some());
}
