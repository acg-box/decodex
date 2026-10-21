use super::*;

#[gpui::test]
fn collaboration_stays_global_and_expands_without_switching_chat_or_draft(
	cx: &mut gpui::TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut surface = AgentSurface::new(cx);
		surface.visual_workspace_fixture(cx);
		surface.visual_workbench_page("workbench", cx);
		surface
	});
	visual.simulate_resize(gpui::size(gpui::px(1536.), gpui::px(1024.)));
	surface.update(visual, |s, cx| {
		s.composer.update(cx, |input, cx| input.set_content("Keep the current draft", cx));
	});
	visual.update(crate::ui_frame::draw_frame);
	let header = visual
		.debug_bounds("collaboration-sage|native:sage:ref-iris")
		.or_else(|| visual.debug_bounds("collaboration-native:sage:ref-iris|sage"))
		.expect("recorded exchange is visible without selecting its owner");
	visual.simulate_click(header.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Keep the current draft");
		assert!(s.work_board.collaboration_expanded.is_some());
		let before: Vec<_> = exchanges(&s.relation_graph(), &s.factory_rows())
			.into_iter()
			.map(|case| case.id)
			.collect();
		assert!(before.contains(&"mira|native:mira:ref-nora".into()));
		assert!(before.contains(&"native:sage:ref-iris|sage".into()));
	});
	let participant = visual
		.debug_bounds("collaboration-participant-native:sage:ref-iris|sage-native:sage:ref-iris")
		.unwrap();
	visual.simulate_click(participant.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.work_board.view.picked_agent.as_deref(), Some("native:sage:ref-iris"));
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Keep the current draft");
		assert!(s.work_board.collaboration_expanded.is_some());
	});
	surface.update(visual, |s, cx| s.select_workbench_agent("ravi", cx));
	visual.update(crate::ui_frame::draw_frame);
	assert!(visual.debug_bounds("collaboration-mira|native:mira:ref-nora").is_some());
	assert!(visual.debug_bounds("workbench-history").is_none());
	assert!(visual.debug_bounds("workbench-composer").is_none());
}

#[gpui::test]
fn tool_completion_is_not_delivery_and_context_reference_is_not_a_message(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(|cx| {
		let mut surface = AgentSurface::new(cx);
		surface.visual_workspace_fixture(cx);
		surface.visual_workbench_page("workbench", cx);
		surface
	});
	surface.read_with(cx, |s, _| {
		let mut graph = s.relation_graph();
		let rows = s.factory_rows();
		let cases = exchanges(&graph, &rows);
		let case = cases
			.iter()
			.find(|case| case.from.key == "mira" && case.to.thread.as_deref() == Some("ref-nora"))
			.unwrap();
		assert_eq!(case.state().0, "Sent");
		let previous_count = case.messages.len();
		assert!(
			case.messages
				.iter()
				.any(|message| message.text == "Check reconnect preserves the full reply.")
		);
		let mut snapshot = case.edge.clone();
		snapshot.id = "reply-snapshot".into();
		snapshot.from = case.to.key.clone();
		snapshot.to = case.from.key.clone();
		snapshot.kind = Kind::Returned;
		snapshot.excerpt = Some("Previous result".into());
		graph.edges.push(snapshot);
		let cases = exchanges(&graph, &rows);
		let observed = cases.iter().find(|case| case.id == "mira|native:mira:ref-nora").unwrap();
		assert_eq!(observed.state().0, "Observed");
		assert_eq!(observed.from.thread.as_deref(), Some("ref-nora"));
		assert_eq!(observed.to.key, "mira");
		assert_eq!(observed.messages.len(), previous_count + 1);
		let mut reference = observed.edge.clone();
		reference.kind = Kind::Context;
		reference.source = None;
		reference.excerpt = Some("Not a sent message".into());
		graph.edges = vec![reference];
		assert!(exchanges(&graph, &rows).is_empty());
	});
}

#[gpui::test]
fn exchange_disclosure_animates_in_both_directions_without_changing_the_draft(
	cx: &mut gpui::TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut s = AgentSurface::new(cx);
		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);
		s.work_board.collaboration_expanded = None;
		s.workspace.agent_panel_width = 240.;
		s.composer.update(cx, |input, cx| input.set_content("Preserve this draft", cx));
		s
	});
	visual.simulate_resize(gpui::size(gpui::px(1536.), gpui::px(1024.)));
	visual.update(crate::ui_frame::draw_frame);
	visual.update(crate::ui_frame::draw_frame);
	std::thread::sleep(std::time::Duration::from_millis(240));
	visual.update(crate::ui_frame::draw_frame);
	let row_id = "collaboration-row-native:sage:ref-iris|sage";
	let control_id = "collaboration-native:sage:ref-iris|sage";
	let height = |visual: &mut gpui::VisualTestContext| {
		f32::from(visual.debug_bounds(row_id).unwrap().size.height)
	};
	let closed = height(visual);
	let header = visual.debug_bounds(control_id).unwrap();
	visual.simulate_click(header.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);
	visual.update(crate::ui_frame::draw_frame);
	std::thread::sleep(std::time::Duration::from_millis(55));
	visual.update(crate::ui_frame::draw_frame);
	let middle = height(visual);
	std::thread::sleep(std::time::Duration::from_millis(240));
	visual.update(crate::ui_frame::draw_frame);
	let open = height(visual);
	assert!(open > closed + 5., "recorded exchange must open: {closed}, {open}");
	assert!(middle > closed && middle < open, "opening must interpolate: {middle}");
	let row = visual.debug_bounds(row_id).unwrap();
	let panel = visual.debug_bounds("workbench-body").unwrap();
	assert!(row.left() >= panel.left() && row.right() <= panel.right());
	let header = visual.debug_bounds(control_id).unwrap();
	visual.simulate_click(header.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);
	std::thread::sleep(std::time::Duration::from_millis(55));
	visual.update(crate::ui_frame::draw_frame);
	let closing = height(visual);
	assert!(closing > closed && closing < open, "closing must interpolate: {closing}");
	std::thread::sleep(std::time::Duration::from_millis(240));
	visual.update(crate::ui_frame::draw_frame);
	assert!((height(visual) - closed).abs() <= 1.);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Preserve this draft");
		assert!(s.work_board.collaboration_expanded.is_none());
	});
}
