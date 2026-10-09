use super::*;
use decodex_protocol::{
	AgentCollaborationDto, AgentCollaborationResultDto, AgentContextReferenceDto, NativeAgentDto,
};
use gpui::AppContext;

#[gpui::test]
fn selecting_agent_focuses_graph_without_opening_a_second_sidebar(cx: &mut gpui::TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut s = fixture(cx);
		s.workspace.graph_expanded = true;
		s
	});
	visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
	visual.update(|w, cx| w.draw(cx).clear());
	visual.run_until_parked();
	visual.update(|w, cx| w.draw(cx).clear());
	surface.update(visual, |s, cx| {
		let row = s.board_rows().into_iter().find(|r| r.native).unwrap();
		let before = s.relation_graph();
		let bounds = before.nodes.iter().map(|n| (n.x, n.y)).collect::<Vec<_>>();
		s.inspect_station(&row, cx);
		assert_eq!(bounds, s.relation_graph().nodes.iter().map(|n| (n.x, n.y)).collect::<Vec<_>>());
		assert_eq!(s.work_board.focus.as_deref(), Some(row.key.as_str()));
		assert!(s.work_board.view.edge.is_none());
		assert_eq!(s.selected.as_deref(), Some("agent"));
	});
	visual.update(|w, cx| w.draw(cx).clear());
	assert!(visual.debug_bounds("relation-inspector-scroll").is_none());
}

#[gpui::test]
fn current_native_activity_creates_interaction_and_reverse_completion_edges(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let row = s.board_rows().into_iter().find(|r| r.key == "release").unwrap();
		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				metrics: Default::default(),
				stamp: s.brief_stamp(&row),
				resources: None,
				read_at: std::time::Instant::now(),
				relations: ["started", "interacted", "completed"]
					.into_iter()
					.map(|kind| {
						(
							kind.into(),
							AgentCollaborationDto {
								sender_thread_id: "thread-release".into(),
								receiver_thread_ids: vec!["native-child".into()],
								tool: format!("subAgentActivity/{kind}"),
								status: kind.into(),
								prompt: String::new(),
								results: vec![],
							},
						)
					})
					.collect(),
			},
		);
		let graph = s.relation_graph();
		assert!(graph.edges.iter().any(|e| e.kind == relations::Kind::Message
			&& e.from == "release"
			&& e.to == "native:release:native-child"));
		assert!(graph.edges.iter().any(|e| e.kind == relations::Kind::Returned
			&& e.to == "release"
			&& e.from == "native:release:native-child"));
		assert_eq!(graph.edges.iter().filter(|e| e.kind == relations::Kind::Spawned).count(), 1);
	});
}

pub(super) fn fixture(cx: &mut Context<AgentSurface>) -> AgentSurface {
	let mut s = AgentSurface::new(cx);
	s.visual_workspace_fixture(cx);
	s.work_board.graph = false;
	for work in &mut s.snapshot.as_mut().unwrap().work_items {
		work.codex_thread_id = Some(format!("thread-{}", work.id));
	}
	s.native_agents.lists.insert(
		"release".into(),
		vec![NativeAgentDto {
			thread_id: "native-child".into(),
			parent_thread_id: "thread-release".into(),
			title: "Reviewer".into(),
			task: "Review sign-in changes".into(),
			status: "idle".into(),
		}],
	);
	s.snapshot.as_mut().unwrap().context_references.push(AgentContextReferenceDto {
		event_id: 42,
		recipient_work_id: "verify".into(),
		source_work_id: "flow".into(),
		source_thread_id: "thread-flow".into(),
		delivery_turn_id: None,
	});
	s
}

#[gpui::test]
fn relations_keep_exact_context_identity_and_global_layout(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let before = s.relation_graph();
		assert!(before.edges.iter().any(|e| e.kind == relations::Kind::Spawned
			&& e.from == "release"
			&& e.to == "native:release:native-child"));
		let reference = before.edges.iter().find(|e| e.kind == relations::Kind::Context).unwrap();
		assert_eq!(reference.from, "flow");
		assert_eq!(reference.to, "verify");
		let positions: Vec<_> = before.nodes.iter().map(|n| (n.key.clone(), n.x, n.y)).collect();
		s.selected = Some("verify".into());
		assert_eq!(
			positions,
			s.relation_graph().nodes.iter().map(|n| (n.key.clone(), n.x, n.y)).collect::<Vec<_>>()
		);
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| w.id == "flow")
			.unwrap()
			.codex_thread_id = Some("replacement".into());
		let after = s.relation_graph();
		let reference = after.edges.iter().find(|e| e.kind == relations::Kind::Context).unwrap();
		assert_eq!(reference.from, "context:flow:thread-flow");
	});
}

#[gpui::test]
fn shared_resources_use_exact_identity_and_reject_stale_associations(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		s.visual_relation_evidence();
		let graph = s.relation_graph();
		assert_eq!(graph.edges.iter().filter(|e| e.kind == relations::Kind::Resource).count(), 2);
		assert_eq!(graph.nodes.iter().filter(|n| n.key.starts_with("resource:")).count(), 1);
		let count = graph.agent_count();
		assert_eq!(count, s.board_rows().len());
		// A changed conversation must not inherit the former thread's resource associations.
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| w.id == "flow")
			.unwrap()
			.codex_thread_id = Some("rebound-thread".into());
		let graph = s.relation_graph();
		assert_eq!(graph.edges.iter().filter(|e| e.kind == relations::Kind::Resource).count(), 1);
		assert!(
			graph.edges.iter().any(|e| e.kind == relations::Kind::Resource && e.from == "verify")
		);
	});
}

#[gpui::test]
fn native_reply_requires_observed_call_and_rejects_changed_binding(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		assert!(!s.relation_graph().edges.iter().any(|e| e.kind == relations::Kind::Returned));
		let row = s.board_rows().into_iter().find(|r| r.key == "release").unwrap();
		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				metrics: Default::default(),
				resources: None,
				stamp: s.brief_stamp(&row),
				read_at: std::time::Instant::now(),
				relations: vec![(
					"call-1".into(),
					AgentCollaborationDto {
						sender_thread_id: "thread-release".into(),
						receiver_thread_ids: vec!["native-child".into()],
						tool: "wait".into(),
						status: "completed".into(),
						prompt: String::new(),
						results: vec![AgentCollaborationResultDto {
							thread_id: "native-child".into(),
							status: "completed".into(),
							message: "One regression found".into(),
						}],
					},
				)],
			},
		);
		assert!(s.relation_graph().edges.iter().any(|e| e.kind == relations::Kind::Returned
			&& e.from == "native:release:native-child"
			&& e.to == "release"));
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| w.id == "release")
			.unwrap()
			.codex_thread_id = Some("changed".into());
		assert!(!s.relation_graph().edges.iter().any(|e| e.kind == relations::Kind::Returned));
	});
}

#[gpui::test]
fn canvas_edge_inspection_preserves_conversation(cx: &mut gpui::TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut s = fixture(cx);
		s.workspace.graph_expanded = true;
		s
	});
	visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
	visual.update(|w, cx| w.draw(cx).clear());
	visual.run_until_parked();
	visual.update(|w, cx| w.draw(cx).clear());
	visual.run_until_parked();
	visual.update(|w, cx| w.draw(cx).clear());
	assert!(visual.debug_bounds("relations-canvas").is_some());
	// Context and dependency share one visible connection; both facts survive.
	let representative = surface.read_with(visual, |s, _| {
		let graph = s.relation_graph();
		assert!(graph.edges.iter().any(|e| e.id == "context:42:verify:thread-flow"));
		graph
			.edges
			.iter()
			.find(|e| {
				(e.from == "flow" && e.to == "verify") || (e.from == "verify" && e.to == "flow")
			})
			.unwrap()
			.id
			.clone()
	});
	let edge = visual.debug_bounds("relation-edge-dependency:flow:verify").unwrap();
	visual.simulate_click(edge.center(), Default::default());
	surface.read_with(visual, |s, _| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(
			s.work_board.view.edge.as_ref().map(|e| e.id.as_str()),
			Some(representative.as_str())
		);
	});
}

#[gpui::test]
fn dragging_a_node_changes_layout_without_switching_the_conversation(
	cx: &mut gpui::TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut s = fixture(cx);
		s.workspace.graph_expanded = true;
		s
	});
	visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
	visual.update(|w, cx| w.draw(cx).clear());
	visual.run_until_parked();
	visual.update(|w, cx| w.draw(cx).clear());
	let start = visual.debug_bounds("relation-node-agent").unwrap().center();
	let before = surface.read_with(visual, |s, _| {
		let g = s.relation_graph();
		let n = g.nodes.iter().find(|n| n.key == "agent").unwrap();
		(n.x, n.y)
	});
	visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
	visual.simulate_mouse_move(
		start + gpui::point(gpui::px(50.), gpui::px(30.)),
		gpui::MouseButton::Left,
		Default::default(),
	);
	visual.simulate_mouse_up(
		start + gpui::point(gpui::px(50.), gpui::px(30.)),
		gpui::MouseButton::Left,
		Default::default(),
	);
	surface.read_with(visual, |s, _| {
		let g = s.relation_graph();
		let n = g.nodes.iter().find(|n| n.key == "agent").unwrap();
		assert!(n.x > before.0 && n.y > before.1);
		assert_eq!(s.selected.as_deref(), Some("agent"));
	});
}

#[test]
fn compact_status_does_not_turn_inactivity_into_attention() {
	assert!(
		compact_signals(
			["Waiting", "Not running", "Idle", "Marked complete", "Follow-up pending"].into_iter()
		)
		.is_empty()
	);
	assert_eq!(
		compact_signals(
			["Approval", "Input needed", "Waiting on work", "Review result", "Running"].into_iter()
		),
		vec![
			(crate::ui_theme::BLUE, "1 active".into()),
			(AMBER, "2 need you".into()),
			(AMBER, "1 blocked".into()),
			(GREEN, "1 to review".into()),
		]
	);
}
