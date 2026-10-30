use std::time::Instant;

use gpui::{AppContext, TestAppContext};

use crate::shell::agent_surface::work_board::{
	Brief,
	activity::Output,
	workbench::{AgentSurface, Context},
};
use decodex_protocol::{AgentPendingEventDto, AgentWorkKindDto, AgentWorkStatusDto};

fn fixture(cx: &mut Context<AgentSurface>) -> AgentSurface {
	let mut surface = AgentSurface::new(cx);

	surface.visual_workspace_fixture(cx);

	for work in &mut surface.snapshot.as_mut().unwrap().work_items {
		if ["agent", "release"].contains(&work.id.as_str()) {
			work.kind = AgentWorkKindDto::Manager;
		}
	}

	surface
}

#[gpui::test]
fn work_activity_keeps_outputs_bound_to_their_thread_and_turn(cx: &mut TestAppContext) {
	let surface = cx.new(fixture);

	surface.update(cx, |s, _| {
		let mut row = s.board_rows().into_iter().find(|row| row.key == "release").unwrap();

		row.group = 2;

		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				review: None,
				source_thread: row.thread.clone(),
				output: Some(Output {
					truncated: false,
					turn: "done".into(),
					text: "Compatibility checks passed.".into(),
				}),
				assignments: Default::default(),
				outcome: Some(("done".into(), "completed".into())),
				preview: None,
				question: None,
				resources: None,
				relations: vec![],
				metrics: Default::default(),
				stamp: s.brief_stamp(&row),
				read_at: Instant::now(),
			},
		);

		assert_eq!(
			s.work_activity_summary(&row),
			("Latest result".into(), "Compatibility checks passed.".into())
		);

		let (_, operations) = s.graph_operations(&row);

		assert_eq!(operations[0].kind, super::super::activity::OperationKind::Result);
		assert_eq!(operations[0].text, "Compatibility checks passed.");

		let complete_answer =
			format!("{}Final verification result.", "Detailed evidence. ".repeat(80));

		s.work_board.briefs.get_mut(&row.key).unwrap().output.as_mut().unwrap().text =
			complete_answer.clone();

		assert_eq!(s.work_output(&row).unwrap().text, complete_answer);
		assert!(s.work_activity_summary(&row).1.chars().count() <= 181);

		s.work_board.briefs.get_mut(&row.key).unwrap().source_thread = Some("other-thread".into());

		assert!(s.work_activity_summary(&row).1.is_empty());
		assert!(s.graph_operations(&row).1.is_empty());

		s.work_board.briefs.get_mut(&row.key).unwrap().source_thread = row.thread.clone();
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == row.work)
			.unwrap()
			.active_turn_id = Some("next".into());

		assert!(s.work_activity_summary(&row).1.is_empty());
	});
}

#[gpui::test]
fn inspection_preserves_the_conversation_and_drafts_until_explicit_open(cx: &mut TestAppContext) {
	let surface = cx.new(fixture);

	surface.update(cx, |s, cx| {
		s.composer.update(cx, |input, cx| input.set_content("Main draft to retain", cx));

		s.workspace.graph_expanded = true;
		s.workspace.agent_panel_width = 256.;

		s.select_workbench_agent("release", cx);

		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Main draft to retain");
		assert_eq!(s.workspace.agent_panel_width, 256.);
		assert!(s.graph_has_composer());
		assert!(!s.workbench_has_recipient());
		assert!(s.workspace.graph_expanded);

		let row = s.board_rows().into_iter().find(|row| row.key == "release").unwrap();

		s.open_graph_agent(&row, cx);

		assert_eq!(s.selected.as_deref(), Some("release"));

		s.composer.update(cx, |input, cx| input.set_content("Release guidance", cx));
		s.select_workbench_agent("agent", cx);

		assert_eq!(s.composer.read(cx).content(), "Release guidance");

		let row = s.board_rows().into_iter().find(|row| row.key == "agent").unwrap();

		s.open_graph_agent(&row, cx);

		assert_eq!(s.composer.read(cx).content(), "Main draft to retain");

		s.clear_workbench_agent(cx);

		assert_eq!(s.composer.read(cx).content(), "Main draft to retain");
	});
}

#[gpui::test]
fn inspecting_a_returned_result_does_not_acknowledge_or_resolve_it(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);

		let snapshot = s.snapshot.as_mut().unwrap();
		let mut result = snapshot.work_items.iter().find(|work| work.id == "flow").unwrap().clone();

		result.id = "returned-result".into();
		result.parent_goal_id = Some("agent".into());

		snapshot.work_items.push(result);
		s.handoffs.fixture();

		let before = s.snapshot.clone();

		assert!(
			s.workbench_entries()
				.iter()
				.any(|entry| entry.row.key == "returned-result" && entry.result)
		);

		s.select_workbench_agent("returned-result", cx);

		assert!(s.handoffs.read_on_open.is_none());
		assert_eq!(s.snapshot, before);
		assert!(
			s.workbench_entries()
				.iter()
				.any(|entry| entry.row.key == "returned-result" && entry.result)
		);
	});
}

#[gpui::test]
fn unbound_child_cannot_reuse_its_manager_composer(cx: &mut TestAppContext) {
	let surface = cx.new(super::super::tests::fixture);

	surface.update(cx, |s, cx| {
		s.profile = None;

		s.select_workbench_agent("native:release:native-child", cx);

		assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("native:release:native-child"));
		assert!(!s.workbench_has_recipient());

		// A registered Task also needs its exact native conversation binding.
		s.select_workbench_agent("verify", cx);

		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert!(!s.workbench_has_recipient());
	});
}

#[gpui::test]
fn async_questions_enter_the_desk_without_a_pending_server_event(cx: &mut TestAppContext) {
	let surface = cx.new(fixture);

	surface.update(cx, |s, _| {
		let work = s
			.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == "release")
			.unwrap();

		work.status = AgentWorkStatusDto::Open;

		let row = s.board_rows().into_iter().find(|row| row.key == "release").unwrap();

		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				review: None,
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: None,
				question: Some("Ship sign-in separately?".into()),
				resources: None,
				relations: vec![],
				metrics: Default::default(),
				stamp: s.brief_stamp(&row),
				read_at: Instant::now(),
			},
		);

		let entries = s.workbench_entries();
		let entry = entries.iter().find(|entry| entry.row.key == "release").unwrap();

		assert!(!entry.result);
		assert_eq!(entry.reason, "Ship sign-in separately?");

		s.work_board.briefs.get_mut("release").unwrap().source_thread =
			Some("rebound-thread".into());

		assert!(!s.workbench_entries().iter().any(|entry| entry.row.key == "release"));
	});
}

#[gpui::test]
fn pending_requests_use_exact_events_instead_of_child_status(cx: &mut TestAppContext) {
	let surface = cx.new(super::super::tests::fixture);

	surface.update(cx, |s, _| {
		s.native_agents.lists.get_mut("release").unwrap()[0].status = "waitingOnApproval".into();
		s.snapshot.as_mut().unwrap().pending_events = [901, 902]
			.into_iter()
			.map(|id| AgentPendingEventDto {
				id,
				source_event_id: format!("request-{id}"),
				work_item_id: "release".into(),
				event_kind: "permission_pending".into(),
				created_at_micros: 1,
				delivery_claimed: false,
			})
			.collect();

		let entries = s.workbench_entries();
		let requests: Vec<_> = entries.iter().filter_map(|entry| entry.request).collect();

		assert_eq!(requests, vec![901, 902]);
		assert!(
			entries.iter().filter(|entry| entry.request.is_some()).all(|entry| !entry.row.native)
		);
		assert!(!entries.iter().any(|entry| entry.row.key == "native:release:native-child"));
	});
}

#[gpui::test]
fn selecting_agents_keeps_the_single_composer_in_the_center(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| fixture(cx));

	visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));

	surface.update(visual, |s, cx| {
		s.workspace.graph_visible = true;
		s.work_board.graph = false;
		s.workspace.graph_expanded = false;
		s.workspace.graph_panel_height = 600.;

		s.select_workbench_agent("release", cx);
		// This test checks the full-detail HUD, not automatic overview fitting.
		s.visual_relation_zoom(1., cx);
	});

	visual.update(crate::ui_frame::draw_frame);
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_frame);

	assert!(visual.debug_bounds("workbench-body").is_some());
	assert!(visual.debug_bounds("workbench-composer").is_none());
	assert!(visual.debug_bounds("floating-composer").is_some());

	let composer = visual.debug_bounds("floating-composer").unwrap();
	let sidebar = visual.debug_bounds("workbench-body").unwrap();

	assert!(composer.right() <= sidebar.left());
	assert!(visual.debug_bounds("graph-usage-distribution").is_none());

	let metrics = visual.debug_bounds("relation-metrics-release").unwrap();

	assert!(metrics.size.height > gpui::px(0.));
	assert!(visual.debug_bounds("workbench-clear").is_none(), "inspection has no Back page");
	assert!(
		visual.debug_bounds("workbench-history").is_none(),
		"collaboration has no duplicate history navigation"
	);
	assert!(visual.debug_bounds("workbench-body").is_some());
	assert!(visual.debug_bounds("workbench-composer").is_none());
	assert!(visual.debug_bounds("floating-composer").is_some());

	surface.read_with(visual, |s, _| {
		assert!(
			s.graph_has_composer(),
			"collaboration does not change the central message recipient"
		);
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("release"));
	});

	surface.update(visual, |s, cx| {
		s.workspace.agent_panel_width = 240.;

		cx.notify();
	});

	visual.update(crate::ui_frame::draw_frame);
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_frame);

	assert!(visual.debug_bounds("workbench-body").is_some());
	assert!(visual.debug_bounds("workbench-composer").is_none());
	assert!(visual.debug_bounds("floating-composer").is_some());

	surface.update(visual, |s, cx| {
		s.clear_workbench_agent(cx);
	});
	visual.update(crate::ui_frame::draw_frame);

	assert!(visual.debug_bounds("workbench-body").is_some());
	assert!(visual.debug_bounds("workbench-composer").is_none());
	assert!(visual.debug_bounds("floating-composer").is_some());
}
