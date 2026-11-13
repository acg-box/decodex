//! Search navigation retains the current draft and command ownership.
use super::*;

fn bind_commands(cx: &mut gpui::TestAppContext) {
	cx.update(|cx| {
		crate::composer_input::bind_keys(cx);
		AgentSurface::bind_command_keys(cx);
	});
}

#[gpui::test]
fn command_search_reaches_work_and_keeps_the_original_input(cx: &mut gpui::TestAppContext) {
	bind_commands(cx);
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		s.composer.update(cx, |i, cx| i.set_content("Do not send this draft", cx));
		cx.notify();
	});
	visual.update(|window, cx| surface.update(cx, |s, cx| s.open_global_commands(window, cx)));
	surface.update(visual, |s, cx| {
		s.workspace
			.navigation
			.search
			.as_ref()
			.unwrap()
			.update(cx, |i, cx| i.set_content("Choose a research stack", cx));
		assert_eq!(s.global_commands(cx).len(), 1);
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let result = visual.debug_bounds("global-command-result-0").unwrap();
	visual.simulate_click(result.center(), Default::default());
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.workspace.work_detail.as_deref(), Some("research"));
		assert_eq!(s.composer.read(cx).content(), "Do not send this draft");
		assert!(!s.workspace.navigation.command_open);
		assert!(!s.sending);
	});
}
#[gpui::test]
fn command_keyboard_selects_a_topic_without_submitting_chat(cx: &mut gpui::TestAppContext) {
	bind_commands(cx);
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_personal_navigation(cx);
		s.composer.update(cx, |i, cx| i.set_content("Keep original input", cx));
	});
	visual.update(|window, cx| surface.update(cx, |s, cx| s.open_global_commands(window, cx)));
	surface.update(visual, |s, cx| {
		s.workspace
			.navigation
			.search
			.as_ref()
			.unwrap()
			.update(cx, |i, cx| i.set_content("Working preferences", cx))
	});
	visual.update(crate::ui_frame::draw_current_frame);
	visual.simulate_keystrokes("enter");
	surface.update(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("topic-preferences"));
		assert!(!s.sending);
		assert!(!s.workspace.navigation.command_open);
		s.open_navigation_conversation("agent", cx);
		assert_eq!(s.composer.read(cx).content(), "Keep original input");
	});
}

#[test]
fn exact_project_names_outrank_incidental_path_matches() {
	let project = |label: &str, path: &str| Command {
		label: label.into(),
		detail: path.into(),
		action: Action::Project(label.into()),
	};
	let commands = ranked(
		vec![
			project("Notes", "/Decodex/notes"),
			project("Decodex tools", "/tools"),
			project("Decodex", "/workspace"),
		],
		"decodex",
	);
	assert_eq!(
		commands.iter().map(|command| command.label.as_str()).collect::<Vec<_>>(),
		["Decodex", "Decodex tools", "Notes"]
	);
}

#[gpui::test]
fn project_picker_returns_to_commands_without_changing_the_draft(cx: &mut gpui::TestAppContext) {
	bind_commands(cx);
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(920.)));
	surface.update(visual, |s, cx| {
		s.visual_navigation_hierarchy(false, cx);
		s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
	});
	visual.update(|window, cx| surface.update(cx, |s, cx| s.open_global_commands(window, cx)));
	surface.update(visual, |s, cx| {
		s.workspace
			.navigation
			.search
			.as_ref()
			.unwrap()
			.update(cx, |input, cx| input.set_content("New project Agent", cx));
	});
	visual.update(crate::ui_frame::draw_current_frame);
	visual.simulate_keystrokes("enter");
	surface.update(visual, |s, cx| {
		assert!(s.workspace.navigation.choose_project);
		assert!(s.workspace.navigation.command_open);
		assert!(
			s.global_commands(cx)
				.iter()
				.any(|command| matches!(command.action, Action::NewAgent(_)))
		);
	});
	visual.update(crate::ui_frame::draw_current_frame);
	visual.simulate_keystrokes("escape");
	surface.read_with(visual, |s, cx| {
		assert!(!s.workspace.navigation.choose_project);
		assert!(s.workspace.navigation.command_open);
		assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		assert!(!s.sending);
	});
}

#[gpui::test]
fn keyboard_selection_accounts_for_group_headers_and_repaint_keeps_scroll(
	cx: &mut gpui::TestAppContext,
) {
	bind_commands(cx);
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(760.)));
	surface.update(visual, |s, cx| s.visual_navigation_hierarchy(false, cx));
	visual.update(|window, cx| surface.update(cx, |s, cx| s.open_global_commands(window, cx)));
	surface.update(visual, |s, cx| s.reset_command_search(true, cx));
	visual.update(crate::ui_frame::draw_current_frame);
	let last = surface.update(visual, |s, cx| s.global_commands(cx).len() - 1);
	for _ in 0..last {
		visual.simulate_keystrokes("down");
	}
	visual.update(crate::ui_frame::draw_current_frame);
	surface.read_with(visual, |s, _| assert_eq!(s.workspace.navigation.command_index, last));
	let viewport = visual.debug_bounds("global-command-results").unwrap();
	let selector = Box::leak(format!("global-command-result-{last}").into_boxed_str());
	let result = visual.debug_bounds(selector).unwrap();
	assert!(result.top() >= viewport.top());
	assert!(result.bottom() <= viewport.bottom());
	let before = surface.read_with(visual, |s, _| s.workspace.navigation.command_scroll.offset());
	assert!(before.y < gpui::px(0.));
	surface.update(visual, |s, cx| {
		s.workspace.navigation.search.as_ref().unwrap().update(cx, |_, cx| cx.notify())
	});
	visual.update(crate::ui_frame::draw_current_frame);
	surface.read_with(visual, |s, _| {
		assert_eq!(s.workspace.navigation.command_index, last);
		assert_eq!(s.workspace.navigation.command_scroll.offset(), before);
	});
}

#[gpui::test]
fn search_reveals_pinned_topics_and_project_agents_without_losing_drafts(
	cx: &mut gpui::TestAppContext,
) {
	bind_commands(cx);
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
	surface.update(visual, |s, cx| {
		s.visual_navigation_overview(false, cx);
		s.open_navigation_conversation("agent", cx);
		s.composer.update(cx, |input, cx| input.set_content("Keep the original draft", cx));
	});
	for (query, id, section) in [
		("Weekly priorities 1", "personal-topic-0", "pin"),
		("Review next steps Observability", "navigation-project-11-agent-0", "projects"),
	] {
		visual.update(|window, cx| {
			surface.update(cx, |s, cx| {
				s.workspace.navigation.collapsed.insert(section);
				s.workspace.navigation.projects.clear();
				s.open_global_commands(window, cx);
			})
		});
		surface.update(visual, |s, cx| {
			s.workspace
				.navigation
				.search
				.as_ref()
				.unwrap()
				.update(cx, |input, cx| input.set_content(query, cx));
			let index = s
				.global_commands(cx)
				.iter()
				.position(
					|command| matches!(&command.action, Action::Conversation(item) if item.id == id),
				)
				.unwrap();
			s.workspace.navigation.command_index = index;
		});
		visual.update(crate::ui_frame::draw_current_frame);
		visual.simulate_keystrokes("enter");
		surface.update(visual, |s, cx| {
			assert_eq!(s.selected.as_deref(), Some(id));
			assert!(!s.workspace.navigation.collapsed.contains(section));
			if section == "projects" {
				assert!(s.workspace.navigation.projects.contains("navigation-project-11"));
			}
			assert!(!s.workspace.navigation.command_open);
			assert!(!s.sending);
			s.open_navigation_conversation("agent", cx);
			assert_eq!(s.composer.read(cx).content(), "Keep the original draft");
		});
	}
}
