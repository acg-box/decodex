use super::AgentSurface;
use gpui::{AppContext as _, TestAppContext};

fn settle(visual: &mut gpui::VisualTestContext) {
	visual.update(crate::ui_frame::draw_current_frame);
	visual.executor().advance_clock(std::time::Duration::from_millis(250));
	visual.update(crate::ui_frame::draw_frame);
}

#[gpui::test]
fn projects_expand_independently_and_all_labels_share_one_origin(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_navigation_hierarchy(false, cx);
		s.composer.update(cx, |input, cx| input.set_content("Keep this project draft", cx));
	});
	settle(visual);
	let label = visual
		.debug_bounds(
			"navigation-item-label-project-navigation-project-0-navigation-project-0-agent-0",
		)
		.unwrap();
	let heading = visual.debug_bounds("navigation-project-label-navigation-project-0").unwrap();
	let category = visual.debug_bounds("navigation-section-label-projects").unwrap();
	assert_eq!(label.left(), category.left());
	assert_eq!(heading.left(), label.left());
	let row = visual
		.debug_bounds("navigation-project-navigation-project-0-navigation-project-0-agent-0")
		.unwrap();
	assert_eq!(row.size.height, gpui::px(28.));
	assert!(visual.debug_bounds("navigation-project-guide-navigation-project-0").is_none());
	assert!(visual.debug_bounds("navigation-project-back").is_none());
	assert!(visual.debug_bounds("navigation-project-switch").is_none());
	let project = visual.debug_bounds("project-header-navigation-project-1").unwrap();
	visual.simulate_click(project.center(), Default::default());
	settle(visual);
	assert!(
		visual
			.debug_bounds("navigation-project-navigation-project-0-navigation-project-0-agent-0")
			.is_some()
	);
	assert!(
		visual
			.debug_bounds("navigation-project-navigation-project-1-navigation-project-1-agent-0")
			.is_some()
	);
	let first = visual.debug_bounds("project-header-navigation-project-0").unwrap();
	visual.simulate_click(first.center(), Default::default());
	settle(visual);
	assert!(
		visual
			.debug_bounds("navigation-project-navigation-project-0-navigation-project-0-agent-0")
			.is_none()
	);
	assert!(
		visual
			.debug_bounds("navigation-project-navigation-project-1-navigation-project-1-agent-0")
			.is_some()
	);
	surface.read_with(visual, |s, cx| {
		assert!(!s.workspace.navigation.projects.contains("navigation-project-0"));
		assert!(s.workspace.navigation.projects.contains("navigation-project-1"));
		assert_eq!(s.selected.as_deref(), Some("navigation-project-0-agent-0"));
		assert_eq!(s.composer.read(cx).content(), "Keep this project draft");
		assert!(!s.sending);
	});
}

fn show_actions(visual: &mut gpui::VisualTestContext, row: &'static str) {
	visual.update(crate::ui_frame::draw_current_frame);
	let row = visual.debug_bounds(row).unwrap();
	visual.simulate_mouse_move(row.center(), None, Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	std::thread::sleep(crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(30));
	visual.update(crate::ui_frame::draw_frame);
}

fn open_actions(visual: &mut gpui::VisualTestContext, selector: &'static str) {
	visual.update(crate::ui_frame::draw_current_frame);
	let actions = visual.debug_bounds(selector).unwrap();
	visual.simulate_click(actions.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
}

#[gpui::test]
fn folding_categories_preserves_open_projects_and_the_current_editor(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_navigation_hierarchy(false, cx);
		s.workspace.navigation.projects.insert("navigation-project-1".into());
		s.composer.update(cx, |input, cx| input.set_content("Keep the current editor", cx));
	});
	settle(visual);
	let heading = visual.debug_bounds("navigation-section-projects").unwrap();
	visual.simulate_click(heading.center(), Default::default());
	settle(visual);
	assert!(visual.debug_bounds("project-header-navigation-project-0").is_none());
	assert!(visual.debug_bounds("assistant-topic-topic-research").is_some());
	let heading = visual.debug_bounds("navigation-section-projects").unwrap();
	visual.simulate_click(heading.center(), Default::default());
	settle(visual);
	assert!(
		visual
			.debug_bounds("navigation-project-navigation-project-0-navigation-project-0-agent-0")
			.is_some()
	);
	assert!(
		visual
			.debug_bounds("navigation-project-navigation-project-1-navigation-project-1-agent-0")
			.is_some()
	);
	surface.read_with(visual, |s, cx| {
		assert!(s.workspace.navigation.projects.contains("navigation-project-0"));
		assert!(s.workspace.navigation.projects.contains("navigation-project-1"));
		assert_eq!(s.selected.as_deref(), Some("navigation-project-0-agent-0"));
		assert_eq!(s.composer.read(cx).content(), "Keep the current editor");
		assert!(!s.sending);
	});
}

#[gpui::test]
fn switching_conversations_preserves_other_open_projects_and_each_draft(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_navigation_hierarchy(false, cx);
		s.workspace.navigation.projects.insert("navigation-project-1".into());
		s.open_navigation_conversation("navigation-project-0-agent-0", cx);
		s.composer.update(cx, |input, cx| input.set_content("Project zero draft", cx));
	});
	settle(visual);
	let other = visual
		.debug_bounds("navigation-project-navigation-project-1-navigation-project-1-agent-0")
		.unwrap();
	visual.simulate_click(other.center(), Default::default());
	settle(visual);
	surface.update(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("navigation-project-1-agent-0"));
		assert!(s.composer.read(cx).content().is_empty());
		s.composer.update(cx, |input, cx| input.set_content("Project one draft", cx));
	});
	let original = visual
		.debug_bounds("navigation-project-navigation-project-0-navigation-project-0-agent-0")
		.unwrap();
	visual.simulate_click(original.center(), Default::default());
	settle(visual);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("navigation-project-0-agent-0"));
		assert_eq!(s.composer.read(cx).content(), "Project zero draft");
		assert!(s.workspace.navigation.projects.contains("navigation-project-0"));
		assert!(s.workspace.navigation.projects.contains("navigation-project-1"));
		assert!(!s.sending);
	});
}

#[gpui::test]
fn metadata_tooltip_stays_beside_the_row_and_cannot_cover_its_actions(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let row = visual.debug_bounds("assistant-topic-topic-research").unwrap();
	let actions = visual.debug_bounds("navigation-actions-assistant-topic-research").unwrap();
	visual.simulate_mouse_move(
		gpui::point(row.left() + gpui::px(24.), row.center().y),
		None,
		Default::default(),
	);
	visual.run_until_parked();
	visual.executor().advance_clock(std::time::Duration::from_secs(1));
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_current_frame);
	let tooltip = visual.debug_bounds("hover-hint-surface").unwrap();
	assert_eq!(tooltip.left(), row.left());
	assert_eq!(tooltip.top(), row.bottom() + gpui::px(crate::ui_theme::POPOVER_GAP));
	assert!(tooltip.intersect(&actions).is_empty());
}

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
fn pin_adds_a_shortcut_without_changing_scope_conversation_or_draft(cx: &mut TestAppContext) {
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
	show_actions(visual, "assistant-topic-topic-research");
	assert_eq!(
		visual.debug_bounds("navigation-actions-assistant-topic-research").unwrap().size.width,
		gpui::px(72.)
	);
	let pin = visual.debug_bounds("navigation-pin-assistant-topic-research").unwrap();
	visual.simulate_click(pin.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Keep the selected draft");
		assert!(s.workspace.navigation.preferences.pinned.contains("topic-research"));
	});
	assert!(visual.debug_bounds("assistant-topic-topic-research").is_some());
	let pin = visual.debug_bounds("navigation-section-pin").unwrap();
	visual.simulate_click(pin.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("navigation-pin-topic-research").is_some());
	assert!(visual.debug_bounds("assistant-topic-topic-research").is_some());
}

#[gpui::test]
fn opening_an_empty_personal_editor_does_not_publish_or_create_a_conversation(
	cx: &mut TestAppContext,
) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		let count = s.snapshot.as_ref().unwrap().work_items.len();
		s.new_assistant_conversation(cx);
		let draft = s.selected.clone().unwrap();
		assert!(s.is_new_conversation());
		assert!(!s.navigation_items(cx).iter().any(|item| item.id == draft));
		s.new_assistant_conversation(cx);
		assert_eq!(s.selected.as_ref(), Some(&draft));
		assert_eq!(s.snapshot.as_ref().unwrap().work_items.len(), count);
		assert!(!s.sending);
		let action = s.build_submission("First submitted content", cx).unwrap();
		assert!(matches!(action, decodex_protocol::AgentActionDto::NewConversation {
			assistant_id: Some(owner), project_id: None, work_id, ..
		} if owner.as_str() == "agent" && work_id.as_str() == draft));
	});
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
fn delete_requires_confirmation_and_outside_click_keeps_the_original_draft(
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
	show_actions(visual, "assistant-topic-topic-research");
	let row = visual.debug_bounds("assistant-topic-topic-research").unwrap();
	let before = visual.debug_bounds("navigation-delete-assistant-topic-research").unwrap();
	visual.simulate_click(before.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert_eq!(visual.debug_bounds("assistant-topic-topic-research").unwrap(), row);
	surface.read_with(visual, |s, cx| {
		assert!(s.workspace.navigation.confirm_delete);
		assert_eq!(s.workspace.navigation.menu.as_deref(), Some("assistant-topic-research"));
		assert!(s.workspace.navigation.mutation.is_none());
		assert_eq!(s.composer.read(cx).content(), "Keep my draft");
	});
	let outside = visual.debug_bounds("navigation-section-pin").unwrap();
	visual.simulate_click(outside.center(), Default::default());
	surface.read_with(visual, |s, cx| {
		assert!(!s.workspace.navigation.confirm_delete);
		assert_eq!(s.composer.read(cx).content(), "Keep my draft");
	});
}

#[gpui::test]
fn long_sections_scroll_together_while_search_and_archived_stay_fixed(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(500.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		let snapshot = s.snapshot.as_mut().unwrap();
		let seed = snapshot.work_items[0].clone();
		for index in 0..20 {
			let mut topic = seed.clone();
			topic.id = format!("topic-{index}");
			topic.title = format!("Topic {index}");
			topic.codex_thread_id = Some(format!("thread-{index}"));
			topic.updated_at_micros = (index + 1) * 60_000_000;
			snapshot.assistant.topic_ids.push(topic.id.clone());
			snapshot.work_items.push(topic);
		}
		s.workspace.navigation.collapsed.remove(super::Section::Assistant.key());
		cx.notify();
	});
	settle(visual);
	assert!(visual.debug_bounds("navigation-section-recent").is_none());
	assert!(visual.debug_bounds("assistant-topic-topic-19").is_some());
	let latest = visual.debug_bounds("assistant-topic-topic-19").unwrap();
	let older = visual.debug_bounds("assistant-topic-topic-18").unwrap();
	assert_eq!(latest.size.height, gpui::px(28.));
	assert_eq!(older.top() - latest.top(), gpui::px(28.));
	let search = visual.debug_bounds("navigation-command-entry").unwrap();
	let archived = visual.debug_bounds("archive-browser").unwrap();
	let assistant = visual.debug_bounds("navigation-section-assistant").unwrap();
	visual.simulate_event(gpui::ScrollEvent {
		position: latest.center(),
		delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-350.))),
		..Default::default()
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("assistant-topic-topic-19").unwrap().top() < latest.top());
	assert!(visual.debug_bounds("navigation-section-assistant").unwrap().top() < assistant.top());
	assert_eq!(visual.debug_bounds("navigation-command-entry"), Some(search));
	assert_eq!(visual.debug_bounds("archive-browser"), Some(archived));
}

#[gpui::test]
fn creation_actions_share_hover_footprints_and_footer_alignment(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| s.visual_navigation_hierarchy(false, cx));
	settle(visual);
	let category = visual.debug_bounds("navigation-section-label-projects").unwrap();
	let footer = visual.debug_bounds("archive-browser-label").unwrap();
	assert_eq!(category.left(), footer.left());
	assert_eq!(category.size.height, footer.size.height);
	for (row_id, actions_id, label_id, width) in [
		(
			"navigation-section-assistant",
			"navigation-heading-actions-assistant",
			"navigation-section-label-assistant",
			48.,
		),
		(
			"navigation-section-projects",
			"navigation-heading-actions-projects",
			"navigation-section-label-projects",
			48.,
		),
		(
			"project-header-navigation-project-0",
			"navigation-project-actions-navigation-project-0",
			"navigation-project-label-navigation-project-0",
			48.,
		),
	] {
		let before = visual.debug_bounds(label_id).unwrap();
		assert_eq!(visual.debug_bounds(actions_id).unwrap().size.width, gpui::px(0.));
		let row = visual.debug_bounds(row_id).unwrap();
		visual.simulate_mouse_move(row.center(), None, Default::default());
		visual.update(crate::ui_frame::draw_current_frame);
		std::thread::sleep(crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(30));
		visual.update(crate::ui_frame::draw_frame);
		let actions = visual.debug_bounds(actions_id).unwrap();
		let shown = visual.debug_bounds(label_id).unwrap();
		assert_eq!(actions.size.width, gpui::px(width));
		assert_eq!(shown.left(), before.left());
		assert_eq!(before.size.width - shown.size.width, gpui::px(width));
		visual.simulate_mouse_move(
			gpui::point(gpui::px(1000.), gpui::px(450.)),
			None,
			Default::default(),
		);
		visual.update(crate::ui_frame::draw_current_frame);
		std::thread::sleep(crate::ui_theme::MOTION_PANEL + std::time::Duration::from_millis(30));
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds(actions_id).unwrap().size.width, gpui::px(0.));
		assert_eq!(visual.debug_bounds(label_id), Some(before));
	}
	surface.read_with(visual, |s, _| {
		assert_eq!(s.selected.as_deref(), Some("navigation-project-0-agent-0"))
	});
}

#[gpui::test]
fn project_delete_confirmation_keeps_its_target_and_cancels_outside(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| s.visual_navigation_hierarchy(false, cx));
	settle(visual);
	show_actions(visual, "project-header-navigation-project-0");
	let selector = "navigation-delete-project-navigation-project-0";
	let before = visual.debug_bounds(selector).unwrap();
	open_actions(visual, selector);
	assert_eq!(visual.debug_bounds(selector), Some(before));
	assert!(visual.debug_bounds("project-action-content").is_none());
	surface.read_with(visual, |s, _| {
		assert!(s.workspace.navigation.confirm_delete);
		assert!(s.workspace.navigation.confirm_started.is_some());
		assert!(s.workspace.navigation.mutation.is_none());
	});
	open_actions(visual, "navigation-section-pin");
	surface.read_with(visual, |s, _| {
		assert!(!s.workspace.navigation.confirm_delete);
		assert!(s.workspace.navigation.menu.is_none());
		assert!(
			s.snapshot.as_ref().unwrap().projects.iter().any(|p| p.id == "navigation-project-0")
		);
		assert_eq!(s.selected.as_deref(), Some("navigation-project-0-agent-0"));
	});
}

#[gpui::test]
fn deleted_project_moves_chats_and_current_draft_without_changing_the_editor(
	cx: &mut TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_navigation_hierarchy(false, cx);
		s.new_project_agent(Some("navigation-project-0"), cx);
		s.composer.update(cx, |input, cx| input.set_content("Keep unsent input", cx));
		let before = s.snapshot.as_ref().unwrap().work_items.clone();
		s.detach_navigation_project("navigation-project-0", cx);
		assert_eq!(s.snapshot.as_ref().unwrap().work_items, before);
		assert_eq!(s.workspace.new_conversation_workspace, None);
		assert_eq!(s.composer.read(cx).content(), "Keep unsent input");
		assert!(
			s.navigation_items(cx)
				.iter()
				.filter(|i| i.id.starts_with("navigation-project-0-"))
				.all(|i| i.project.is_none())
		);
	});
	settle(visual);
	assert!(visual.debug_bounds("project-header-navigation-project-0").is_none());
	assert!(visual.debug_bounds("project-header-").is_some());
}

#[gpui::test]
fn compact_sidebar_fully_covers_labels_and_centers_the_shared_icon_column(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1000.)));
	visual.simulate_mouse_move(
		gpui::point(gpui::px(1000.), gpui::px(800.)),
		None,
		Default::default(),
	);
	surface.update(visual, |s, cx| s.visual_navigation_hierarchy(true, cx));
	settle(visual);
	let rail = visual.debug_bounds("workspace-navigation-pane").unwrap();
	let right = rail.right() - gpui::px(crate::ui_theme::SIDEBAR_INSET);
	for name in [
		"navigation-section-label-pin",
		"navigation-section-label-assistant",
		"navigation-section-label-projects",
		"navigation-project-label-navigation-project-0",
		"navigation-item-label-assistant-topic-research",
	] {
		let label = visual.debug_bounds(name).unwrap();
		assert!(label.left() >= right, "{name} leaks into the compact rail: {label:?}, {rail:?}");
	}
	let command = visual.debug_bounds("navigation-command-entry").unwrap();
	assert_eq!(command.center().x, rail.center().x);
	assert_eq!(command.size.width, gpui::px(crate::ui_theme::NAVIGATION_LEADING_SIZE));
	for initial in
		["initial-project-navigation-project-0", "initial-navigation-assistant-topic-research"]
	{
		let mark = visual.debug_bounds(initial).unwrap();
		assert_eq!(mark.center().x, rail.center().x);
		assert!(mark.right() <= right);
	}
	surface.update(visual, |s, cx| {
		s.workspace.sidebar_visible = true;
		cx.notify();
	});
	settle(visual);
	assert!(visual.debug_bounds("initial-project-navigation-project-0").is_none());
	assert!(visual.debug_bounds("initial-navigation-assistant-topic-research").is_none());
}

#[gpui::test]
fn empty_project_does_not_leave_a_blank_row_in_the_compact_rail(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_navigation_hierarchy(true, cx);
		let projects = &mut s.snapshot.as_mut().unwrap().projects;
		let mut empty = projects[0].clone();
		empty.id = "navigation-empty".into();
		empty.name = "Empty project".into();
		empty.agent_ids.clear();
		projects.push(empty);
		s.workspace.navigation.projects.insert("navigation-empty".into());
	});
	settle(visual);
	let selector = "navigation-project-group-navigation-empty";
	assert_eq!(visual.debug_bounds(selector).unwrap().size.height, gpui::px(28.));
	surface.update(visual, |s, cx| {
		s.workspace.sidebar_visible = true;
		cx.notify();
	});
	settle(visual);
	assert!(visual.debug_bounds(selector).unwrap().size.height > gpui::px(28.));
	surface.read_with(visual, |s, _| {
		assert!(s.workspace.navigation.projects.contains("navigation-empty"));
	});
}
