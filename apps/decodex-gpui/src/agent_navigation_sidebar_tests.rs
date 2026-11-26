use super::AgentSurface;
use decodex_protocol::{AgentDispatchStateDto, AgentWorkStatusDto};
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
		let snapshot = s.snapshot.as_mut().unwrap();
		let completed = snapshot
			.work_items
			.iter_mut()
			.find(|work| work.id == "navigation-project-0-agent-0")
			.unwrap();
		completed.status = AgentWorkStatusDto::Resolved;
		let completed = completed.clone();
		let result = s.handoffs.item_for(snapshot, &completed).unwrap();
		s.handoffs.acknowledge(&result.work, &result.key, snapshot);
		snapshot
			.work_items
			.iter_mut()
			.find(|work| work.id == "navigation-project-0-agent-1")
			.unwrap()
			.dispatch_state = AgentDispatchStateDto::Running;
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
	for (title, slot, icon, visible) in [
		(
			"navigation-item-label-chats-research-chat",
			"navigation-leading-chats-research-chat",
			"navigation-chats-research-chat-mark",
			false,
		),
		(
			"navigation-item-label-project-navigation-project-0-navigation-project-0-agent-0",
			"navigation-leading-project-navigation-project-0-navigation-project-0-agent-0",
			"navigation-project-navigation-project-0-navigation-project-0-agent-0-mark",
			false,
		),
		(
			"navigation-item-label-project-navigation-project-0-navigation-project-0-agent-1",
			"navigation-leading-project-navigation-project-0-navigation-project-0-agent-1",
			"navigation-project-navigation-project-0-navigation-project-0-agent-1-mark",
			true,
		),
	] {
		let title = visual.debug_bounds(title).unwrap();
		let slot = visual.debug_bounds(slot).unwrap();
		let icon = visual.debug_bounds(icon);
		assert_eq!(title.left(), category.left());
		assert_eq!(slot.right(), title.left());
		if visible {
			let icon = icon.unwrap();
			let edge = gpui::px(crate::ui_theme::NAVIGATION_ACTION_SIZE);
			assert_eq!(icon.center(), slot.center());
			assert_eq!(icon.size, gpui::size(edge, edge));
		} else {
			assert!(icon.is_none());
		}
	}
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

#[gpui::test]
fn sidebar_result_dot_clears_after_opening_and_loading_the_conversation(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_navigation_hierarchy(false, cx);
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == "research-chat")
			.unwrap()
			.status = AgentWorkStatusDto::Resolved;
	});
	settle(visual);
	let mark = "navigation-chats-research-chat-mark";
	let title = "navigation-item-label-chats-research-chat";
	let origin = visual.debug_bounds(title).unwrap().left();
	assert!(visual.debug_bounds(mark).is_some());

	let row = visual.debug_bounds("navigation-chats-research-chat").unwrap();
	visual.simulate_click(row.center(), Default::default());
	settle(visual);
	assert!(visual.debug_bounds(mark).is_some());
	surface.update(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("research-chat"));
		assert!(s.acknowledge_open_handoff("research-chat"));
		cx.notify();
	});
	settle(visual);
	assert!(visual.debug_bounds(mark).is_none());
	assert_eq!(visual.debug_bounds(title).unwrap().left(), origin);
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
	assert!(visual.debug_bounds("navigation-chats-research-chat").is_some());
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
	let row = visual.debug_bounds("navigation-chats-research-chat").unwrap();
	let actions = visual.debug_bounds("navigation-actions-chats-research-chat").unwrap();
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
fn assistant_entry_opens_the_same_conversation_and_preserves_each_draft(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		s.open_navigation_conversation("agent", cx);
		s.composer.update(cx, |i, cx| i.set_content("Assistant draft", cx));
		s.open_navigation_conversation("research-chat", cx);
		s.composer.update(cx, |i, cx| i.set_content("Research draft", cx));
	});
	settle(visual);
	let home = visual.debug_bounds("navigation-section-assistant").unwrap();
	visual.simulate_mouse_move(home.center(), None, Default::default());
	settle(visual);
	assert!(visual.debug_bounds("navigation-heading-actions-assistant").is_none());
	assert!(visual.debug_bounds("navigation-chevron-assistant").is_none());
	assert!(visual.debug_bounds("navigation-chats-agent").is_none());
	visual.simulate_click(home.center(), Default::default());
	surface.update(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Assistant draft");
		s.open_assistant(cx);
		assert_eq!(s.selected.as_deref(), Some("agent"));
		s.open_navigation_conversation("research-chat", cx);
		assert_eq!(s.composer.read(cx).content(), "Research draft");
		assert!(!s.sending);
	});
}

#[gpui::test]
fn native_pins_route_registered_and_external_threads_without_changing_scope(
	cx: &mut TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		s.workspace.work_detail = None;
		s.composer.update(cx, |i, cx| i.set_content("Keep the selected draft", cx));
		s.workspace.navigation.pins.rows = Some(
			["fixture-research-chat", "external"]
				.map(|id| decodex_protocol::NativeThreadCandidate {
					thread_id: id.into(),
					title: id.into(),
					preview: None,
					updated_at_micros: 0,
				})
				.to_vec(),
		);
		cx.notify();
	});
	settle(visual);
	let heading = visual.debug_bounds("navigation-section-pin").unwrap();
	visual.simulate_click(heading.center(), Default::default());
	settle(visual);
	assert!(visual.debug_bounds("navigation-chats-research-chat").is_some());
	let external = visual.debug_bounds("navigation-pin-external").unwrap();
	let count = surface.read_with(visual, |s, _| s.snapshot.as_ref().unwrap().work_items.len());
	visual.simulate_click(external.center(), Default::default());
	surface.read_with(visual, |s, cx| {
		assert!(s.workspace.registration.is_some());
		assert_eq!(s.snapshot.as_ref().unwrap().work_items.len(), count);
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Keep the selected draft");
	});
	let registered = visual.debug_bounds("navigation-pin-research-chat").unwrap();
	visual.simulate_click(registered.center(), Default::default());
	surface.read_with(visual, |s, _| assert_eq!(s.selected.as_deref(), Some("research-chat")));
}

#[gpui::test]
fn opening_an_empty_chat_editor_does_not_publish_or_create_a_conversation(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		let count = s.snapshot.as_ref().unwrap().work_items.len();
		s.new_project_agent(None, cx);
		let draft = s.selected.clone().unwrap();
		assert!(s.is_new_conversation());
		assert!(!s.navigation_items(cx).iter().any(|item| item.id == draft));
		s.new_project_agent(None, cx);
		assert_eq!(s.selected.as_ref(), Some(&draft));
		assert_eq!(s.snapshot.as_ref().unwrap().work_items.len(), count);
		assert!(!s.sending);
		let action = s.build_submission("First submitted content", cx).unwrap();
		assert!(matches!(action, decodex_protocol::AgentActionDto::NewConversation {
			project_id: None, work_id, ..
		} if work_id.as_str() == draft));
	});
}

#[gpui::test]
fn chats_add_opens_a_folder_picker_and_fits_long_paths_within_the_chat(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(760.)));
	surface.update(visual, |s, cx| s.visual_navigation_hierarchy(false, cx));
	show_actions(visual, "navigation-section-chats");
	let add = visual.debug_bounds("navigation-create-chat").unwrap();
	visual.simulate_click(add.center(), Default::default());
	settle(visual);
	let folder = visual.debug_bounds("choose-chat-folder").unwrap();
	let project = visual.debug_bounds("workspace-picker-trigger").unwrap();
	assert!(project.bottom() <= folder.top());
	assert!(visual.debug_bounds("clear-chat-folder").is_none());

	let long = format!("/workspace/{}{}", "parent/".repeat(18), "longname".repeat(20));
	visual.simulate_click(folder.center(), Default::default());
	visual.simulate_path_prompt_response(|options| {
		assert!(options.directories && !options.files && !options.multiple);
		Some(vec![std::path::PathBuf::from(&long)])
	});
	visual.run_until_parked();
	settle(visual);
	let chat = visual.debug_bounds("conversation-start").unwrap();
	let folder = visual.debug_bounds("choose-chat-folder").unwrap();
	let reset = visual.debug_bounds("clear-chat-folder").unwrap();
	assert!(folder.left() >= chat.left() && reset.right() <= chat.right());
	assert!(folder.right() <= reset.left() && folder.size.width <= gpui::px(180.));
	surface.update(visual, |s, cx| {
		assert!(matches!(s.build_submission("Hello", cx).unwrap(), decodex_protocol::AgentActionDto::NewConversation { project_id: None, cwd: Some(cwd), .. } if cwd.as_str() == long));
	});
	let scope = visual.debug_bounds("workspace-picker-trigger").unwrap();
	visual.simulate_click(scope.center(), Default::default());
	settle(visual);
	let project = visual.debug_bounds("workspace-choice-navigation-project-0").unwrap();
	visual.simulate_click(project.center(), Default::default());
	settle(visual);
	surface.update(visual, |s, cx| {
		assert!(matches!(s.build_submission("Hello", cx).unwrap(), decodex_protocol::AgentActionDto::NewConversation { project_id: Some(project), cwd: Some(cwd), .. } if project.as_str() == "navigation-project-0" && cwd.as_str() == long));
	});
	let reset = visual.debug_bounds("clear-chat-folder").unwrap();
	visual.simulate_click(reset.center(), Default::default());
	settle(visual);
	assert!(visual.debug_bounds("clear-chat-folder").is_none());
	surface.read_with(visual, |s, _| assert!(s.draft_chat_folder().is_none()));
}

#[gpui::test]
fn project_creation_controls_reuse_a_distinct_draft_for_each_project(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		let count = s.snapshot.as_ref().unwrap().work_items.len();
		s.new_project_agent(Some("alpha"), cx);
		let alpha = s.selected.clone();
		let folder = decodex_protocol::ConversationWorkingDirectory::new("/chosen/alpha").unwrap();
		s.draft_profiles.chat_folders.insert(alpha.clone().unwrap(), folder.clone());
		s.composer.update(cx, |i, cx| i.set_content("Alpha draft", cx));
		s.new_project_agent(Some("beta"), cx);
		let beta = s.selected.clone();
		assert_ne!(alpha, beta);
		assert!(s.draft_chat_folder().is_none());
		assert!(s.composer.read(cx).content().is_empty());
		s.composer.update(cx, |i, cx| i.set_content("Beta draft", cx));
		s.new_project_agent(Some("alpha"), cx);
		s.new_project_agent(Some("alpha"), cx);
		assert_eq!(s.selected, alpha);
		assert!(matches!(s.build_submission("Continue", cx).unwrap(), decodex_protocol::AgentActionDto::NewConversation { project_id: Some(project), cwd: Some(cwd), .. } if project.as_str() == "alpha" && cwd == folder));
		assert_eq!(s.composer.read(cx).content(), "Alpha draft");
		assert_eq!(s.workspace.new_conversation_workspace.as_deref(), Some("alpha"));
		s.new_project_agent(Some("beta"), cx);
		assert_eq!(s.selected, beta);
		assert!(s.draft_chat_folder().is_none());
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
	show_actions(visual, "navigation-chats-research-chat");
	let row = visual.debug_bounds("navigation-chats-research-chat").unwrap();
	let before = visual.debug_bounds("navigation-delete-chats-research-chat").unwrap();
	visual.simulate_click(before.center(), Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert_eq!(visual.debug_bounds("navigation-chats-research-chat").unwrap(), row);
	surface.read_with(visual, |s, cx| {
		assert!(s.workspace.navigation.confirm_delete);
		assert_eq!(s.workspace.navigation.menu.as_deref(), Some("chats-research-chat"));
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
		s.visual_navigation_overview(false, cx);
		cx.notify();
	});
	settle(visual);
	assert!(visual.debug_bounds("navigation-section-recent").is_none());
	assert!(visual.debug_bounds("navigation-chats-chat-0").is_some());
	let latest = visual.debug_bounds("navigation-chats-chat-0").unwrap();
	let older = visual.debug_bounds("navigation-chats-chat-1").unwrap();
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
	assert!(visual.debug_bounds("navigation-chats-chat-0").unwrap().top() < latest.top());
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
			"navigation-section-chats",
			"navigation-heading-actions-chats",
			"navigation-section-label-chats",
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
		s.workspace.navigation.collapsed.insert(super::Section::Chats.key());
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
	assert!(visual.debug_bounds("navigation-chats-navigation-project-0-agent-0").is_some());
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
		"navigation-section-label-chats",
		"navigation-section-label-projects",
		"navigation-project-label-navigation-project-0",
		"navigation-item-label-chats-research-chat",
	] {
		let label = visual.debug_bounds(name).unwrap();
		assert!(label.left() >= right, "{name} leaks into the compact rail: {label:?}, {rail:?}");
	}
	let command = visual.debug_bounds("navigation-command-entry").unwrap();
	assert_eq!(command.center().x, rail.center().x);
	assert_eq!(command.size.width, gpui::px(crate::ui_theme::NAVIGATION_LEADING_SIZE));
	for initial in
		["initial-project-navigation-project-0", "initial-navigation-chats-research-chat"]
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
	assert!(visual.debug_bounds("initial-navigation-chats-research-chat").is_none());
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
