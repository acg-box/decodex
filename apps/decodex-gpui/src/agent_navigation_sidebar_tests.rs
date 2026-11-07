use super::AgentSurface;
use gpui::TestAppContext;

#[gpui::test]
fn new_assistant_topic_reuses_an_unsent_draft_after_navigation(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		let snapshot = s.snapshot.as_mut().unwrap();
		snapshot.work_items.iter_mut().find(|w| w.id == "agent").unwrap().codex_thread_id =
			Some("started-general".into());
		s.open_navigation_conversation("topic-research", cx);
		s.composer.update(cx, |i, cx| i.set_content("Unsent research request", cx));
		s.open_navigation_conversation("agent", cx);
		let count = s.snapshot.as_ref().unwrap().work_items.len();
		for _ in 0..3 {
			s.new_assistant_conversation(cx);
		}
		assert_eq!(s.selected.as_deref(), Some("topic-research"));
		assert_eq!(s.composer.read(cx).content(), "Unsent research request");
		assert_eq!(s.snapshot.as_ref().unwrap().work_items.len(), count);
		assert!(!s.sending);
	});
}

#[gpui::test]
fn pin_moves_one_reference_and_keeps_the_conversation_and_draft(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		s.workspace.work_detail = None;
		s.composer.update(cx, |i, cx| i.set_content("Keep the selected draft", cx));
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let pin = visual.debug_bounds("navigation-pin-recent-topic-research").unwrap();
	visual.simulate_click(pin.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Keep the selected draft");
		assert!(s.workspace.navigation.preferences.pinned.contains("topic-research"));
	});
	assert!(visual.debug_bounds("navigation-pin-topic-research").is_some());
	assert!(visual.debug_bounds("navigation-recent-topic-research").is_none());
}

#[gpui::test]
fn project_creation_controls_reuse_a_distinct_draft_for_each_project(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		let count = s.snapshot.as_ref().unwrap().work_items.len();
		s.new_project_agent(Some("alpha"), cx);
		let alpha = s.selected.clone();
		s.composer.update(cx, |i, cx| i.set_content("Alpha draft", cx));
		s.new_project_agent(Some("beta"), cx);
		let beta = s.selected.clone();
		assert_ne!(alpha, beta);
		assert!(s.composer.read(cx).content().is_empty());
		s.composer.update(cx, |i, cx| i.set_content("Beta draft", cx));
		s.new_project_agent(Some("alpha"), cx);
		s.new_project_agent(Some("alpha"), cx);
		assert_eq!(s.selected, alpha);
		assert_eq!(s.composer.read(cx).content(), "Alpha draft");
		assert_eq!(s.workspace.new_conversation_workspace.as_deref(), Some("alpha"));
		s.new_project_agent(Some("beta"), cx);
		assert_eq!(s.selected, beta);
		assert_eq!(s.composer.read(cx).content(), "Beta draft");
		assert_eq!(s.snapshot.as_ref().unwrap().work_items.len(), count);
		assert!(!s.sending);
	});
}

#[gpui::test]
fn delete_arms_the_same_icon_and_outside_click_cancels_without_changing_a_draft(
	cx: &mut TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		s.workspace.work_detail = None;
		s.composer.update(cx, |i, cx| i.set_content("Keep my draft", cx));
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let before = visual.debug_bounds("navigation-delete-recent-topic-research").unwrap();
	visual.simulate_click(before.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert_eq!(visual.debug_bounds("navigation-delete-recent-topic-research").unwrap(), before);
	surface.read_with(visual, |s, cx| {
		assert!(s.workspace.navigation.confirm_delete);
		assert_eq!(s.workspace.navigation.menu.as_deref(), Some("recent-topic-research"));
		assert!(s.workspace.navigation.mutation.is_none());
		assert_eq!(s.composer.read(cx).content(), "Keep my draft");
	});
	let heading = visual.debug_bounds("navigation-section-projects").unwrap();
	visual.simulate_click(heading.center(), Default::default());
	surface.read_with(visual, |s, cx| {
		assert!(!s.workspace.navigation.confirm_delete);
		assert_eq!(s.composer.read(cx).content(), "Keep my draft");
	});
}

#[gpui::test]
fn recent_renders_every_available_topic_and_sections_keep_the_compact_rhythm(
	cx: &mut TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		let snapshot = s.snapshot.as_mut().unwrap();
		let seed = snapshot.work_items[0].clone();
		for index in 0..20 {
			let mut topic = seed.clone();
			topic.id = format!("topic-{index}");
			topic.title = format!("Topic {index}");
			snapshot.assistant.topic_ids.push(topic.id.clone());
			snapshot.work_items.push(topic);
		}
		s.workspace.navigation.collapsed =
			std::collections::HashSet::from(["pin", "recent", "assistant", "projects"]);
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let mut previous = None;
	for section in [
		"navigation-section-pin",
		"navigation-section-recent",
		"navigation-section-assistant",
		"navigation-section-projects",
	] {
		let bounds = visual.debug_bounds(section).unwrap();
		assert_eq!(f32::from(bounds.size.height), 28.);
		if let Some(top) = previous {
			assert_eq!(f32::from(bounds.origin.y) - top, 28.);
		}
		previous = Some(f32::from(bounds.origin.y));
	}
	surface.update(visual, |s, cx| {
		assert!(s.navigation_items(cx).iter().any(|item| item.id == "topic-19"));
		s.workspace.navigation.collapsed.remove("recent");
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("navigation-recent-topic-19").is_some());
}
