use super::*;
use decodex_protocol::{
	AgentCollaborationDto, AgentCollaborationResultDto, AgentContextReferenceDto, NativeAgentDto,
};
use gpui::AppContext;

#[gpui::test]
fn live_activity_uses_only_the_current_thread_turn_and_running_action(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let snapshot = s.snapshot.as_mut().unwrap();
		let work = snapshot.work_items.iter_mut().find(|work| work.id == "release").unwrap();
		work.active_turn_id = Some("live".into());
		snapshot.agent_slots.insert("release".into(), 3);
		let mut row = s.board_rows().into_iter().find(|row| row.key == "release").unwrap();
		row.group = 1;
		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: Some(activity::Preview {
					turn: "live".into(),
					task: String::new(),
					text: "cargo test".into(),
					running: Some("cargo test".into()),
					recent: vec![],
				}),
				question: None,
				resources: None,
				relations: vec![],
				metrics: crate::ui_work_metrics::Sample {
					stats: Some(decodex_protocol::AgentThreadMetrics {
						active_ms: Some(8_000),
						..Default::default()
					}),
					..Default::default()
				},
				stamp: s.brief_stamp(&row),
				read_at: std::time::Instant::now(),
			},
		);
		let live = s.live_work_activity(&row).unwrap();
		assert_eq!((live.agent.as_str(), live.action.as_str()), ("A3", "cargo test"));
		assert_eq!(live.sample.active_time(), Some(8_000));
		s.work_board.briefs.get_mut(&row.key).unwrap().stamp = "refresh in progress".into();
		assert!(
			s.live_work_activity(&row).is_some(),
			"a refresh cannot blink a source-bound activity away"
		);
		s.work_board.briefs.get_mut(&row.key).unwrap().metrics.stats.as_mut().unwrap().generating =
			true;
		assert_eq!(s.live_work_activity(&row).unwrap().action, "Generating");
		s.work_board.briefs.get_mut(&row.key).unwrap().preview.as_mut().unwrap().turn =
			"previous".into();
		assert!(
			s.live_work_activity(&row).is_none(),
			"a prior turn cannot supply a current action or clock"
		);
		s.work_board.briefs.get_mut(&row.key).unwrap().preview.as_mut().unwrap().turn =
			"live".into();
		s.work_board.briefs.get_mut(&row.key).unwrap().source_thread = Some("old-thread".into());
		assert!(s.live_work_activity(&row).is_none());
	});
}

#[gpui::test]
fn global_activity_waits_for_archive_visibility_before_showing_cached_agents(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		assert!(s.work_statistics().is_some());
		s.snapshot.as_mut().unwrap().connection_initializing = true;
		assert!(s.work_statistics().is_none());
		s.snapshot.as_mut().unwrap().connection_initializing = false;
		assert!(s.work_statistics().is_some());
	});
}

#[gpui::test]
fn graph_preview_rejects_old_turns_and_threads_without_repositioning_other_nodes(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let row = s.board_rows().into_iter().find(|row| row.key == "release").unwrap();
		let positions = || {
			s.relation_graph()
				.nodes
				.iter()
				.filter(|node| node.key != "release")
				.map(|node| (node.key.clone(), node.x, node.y))
				.collect::<Vec<_>>()
		};
		let before = positions();
		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: Some(activity::Preview {
					running: None,
					recent: vec![],
					turn: "latest".into(),
					task: "Check sign-in".into(),
					text: "cargo test sign_in".into(),
				}),
				question: None,
				resources: None,
				relations: vec![],
				metrics: Default::default(),
				stamp: s.brief_stamp(&row),
				read_at: std::time::Instant::now(),
			},
		);
		assert_eq!(s.graph_preview(&row), ("Check sign-in".into(), "cargo test sign_in".into()));
		let mut quiet = row.clone();
		quiet.group = 2;
		assert!(s.graph_operations(&quiet).1.is_empty(), "a quiet reply is not live activity");
		assert_eq!(s.graph_preview(&quiet).1, "cargo test sign_in", "the source remains readable");
		quiet.group = 1;
		assert_eq!(s.graph_operations(&quiet).1[0].text, "cargo test sign_in");
		s.work_board.briefs.get_mut(&row.key).unwrap().question =
			Some("Which format do you prefer?".into());
		assert_eq!(
			s.graph_preview(&row),
			("Check sign-in".into(), "Which format do you prefer?".into())
		);
		s.work_board.briefs.get_mut(&row.key).unwrap().question = None;
		assert_eq!(
			before,
			s.relation_graph()
				.nodes
				.iter()
				// The updated card can grow for its first real activity row.
				.filter(|node| node.key != "release")
				.map(|node| (node.key.clone(), node.x, node.y))
				.collect::<Vec<_>>()
		);
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == row.work)
			.unwrap()
			.active_turn_id = Some("next".into());
		assert_eq!(s.graph_preview(&row), (row.title.clone(), String::new()));
		s.work_board.briefs.get_mut(&row.key).unwrap().source_thread = Some("other-thread".into());
		assert_eq!(s.graph_preview(&row), (row.title.clone(), String::new()));
	});
}

#[gpui::test]
fn global_attention_count_uses_the_same_source_bound_question_as_the_graph(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let snapshot = s.snapshot.as_mut().unwrap();
		snapshot.work_items.retain(|w| w.id == "release");
		snapshot.dependencies.clear();
		snapshot.pending_events.clear();
		let work = &mut snapshot.work_items[0];
		work.parent_goal_id = None;
		work.status = super::super::AgentWorkStatusDto::Open;
		work.dispatch_state = super::super::AgentDispatchStateDto::Idle;
		work.active_turn_id = None;
		s.native_agents.lists.clear();
		let row = s.factory_rows().remove(0);
		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: None,
				question: Some("Which outing would you prefer?".into()),
				resources: None,
				relations: vec![],
				metrics: Default::default(),
				stamp: s.brief_stamp(&row),
				read_at: std::time::Instant::now(),
			},
		);
		assert_eq!(s.graph_question(&row), Some("Which outing would you prefer?"));
		s.work_board.briefs.get_mut(&row.key).unwrap().source_thread = Some("old-thread".into());
		assert_eq!(s.graph_question(&row), None);
		s.work_board.briefs.get_mut(&row.key).unwrap().source_thread = row.thread.clone();
		s.work_board.briefs.get_mut(&row.key).unwrap().question = None;
		assert_eq!(s.graph_question(&row), None);
	});
}

#[gpui::test]
fn selecting_usage_opens_the_workbench_without_a_popover(cx: &mut gpui::TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut s = fixture(cx);
		s.workspace.graph_expanded = true;
		s
	});
	visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
	visual.update(|w, cx| w.draw(cx).clear(cx));
	visual.run_until_parked();
	visual.update(|w, cx| w.draw(cx).clear(cx));
	surface.update(visual, |s, cx| {
		let row = s.board_rows().into_iter().find(|r| r.native).unwrap();
		let before = s.relation_graph();
		let bounds = before.nodes.iter().map(|n| (n.x, n.y)).collect::<Vec<_>>();
		s.select_workbench_agent(&row.key, cx);
		assert_eq!(bounds, s.relation_graph().nodes.iter().map(|n| (n.x, n.y)).collect::<Vec<_>>());
		assert_eq!(s.work_board.view.picked_agent.as_deref(), Some(row.key.as_str()));
		assert_eq!(s.work_board.focus.as_deref(), Some(row.key.as_str()));
		assert!(s.work_board.view.edge.is_none());
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert!(
			!s.workbench_has_recipient(),
			"an unavailable child must not borrow its parent's input"
		);
	});
	visual.update(|w, cx| w.draw(cx).clear(cx));
	assert!(visual.debug_bounds("relation-inspector-scroll").is_none());
	assert!(visual.debug_bounds("agent-usage-popover").is_none());
}

#[gpui::test]
fn graph_distribution_keeps_recorded_totals_in_the_activity_island(cx: &mut gpui::TestAppContext) {
	let (_, visual) = cx.add_window_view(|_, cx| {
		let mut s = fixture(cx);
		s.workspace.graph_expanded = true;
		s
	});
	visual.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(800.)));
	visual.update(crate::ui_frame::draw_frame);
	assert!(visual.debug_bounds("graph-usage-footer").is_some());
	assert!(visual.debug_bounds("relations-usage").is_some());
	assert!(visual.debug_bounds("graph-usage-tokens").is_none());
	assert!(visual.debug_bounds("graph-usage-time").is_none());
	assert!(visual.debug_bounds("graph-usage-total").is_none());
	assert!(visual.debug_bounds("relations-canvas").is_some());
}

#[gpui::test]
fn missing_token_statistics_notify_only_after_a_successful_completed_turn(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		for row in s.factory_rows() {
			s.work_board.briefs.insert(
				row.key.clone(),
				Brief {
					output: None,
					source_thread: row.thread.clone(),
					assignments: Default::default(),
					outcome: Some(("turn".into(), "completed".into())),
					preview: None,
					question: None,
					resources: None,
					relations: vec![],
					metrics: crate::ui_work_metrics::Sample {
						tokens: Some(0),
						stats: Some(decodex_protocol::AgentThreadMetrics {
							completed_ms: Some(1000),
							..Default::default()
						}),
						..Default::default()
					},
					stamp: s.brief_stamp(&row),
					read_at: std::time::Instant::now(),
				},
			);
		}
		s.work_board.briefs.get_mut("release").unwrap().metrics.tokens = None;
		s.publish_usage();
		assert_eq!(s.token_statistics_gap(), 1);
		assert!(
			s.operation_notices().iter().any(|(title, _)| *title == "Token statistics unavailable")
		);
		s.work_board.briefs.get_mut("release").unwrap().metrics.stats.as_mut().unwrap().active_ms =
			Some(500);
		s.publish_usage();
		assert_eq!(
			s.token_statistics_gap(),
			0,
			"an active response has not supplied its final counters yet"
		);
		s.work_board.briefs.get_mut("release").unwrap().metrics.stats.as_mut().unwrap().active_ms =
			None;
		s.work_board.briefs.get_mut("release").unwrap().outcome =
			Some(("turn".into(), "failed".into()));
		s.publish_usage();
		assert_eq!(s.token_statistics_gap(), 0, "a failed request need not have generated tokens");
		s.work_board.briefs.get_mut("release").unwrap().outcome =
			Some(("turn".into(), "completed".into()));
		s.work_board.briefs.get_mut("release").unwrap().metrics.tokens = Some(0);
		s.publish_usage();
		assert_eq!(s.token_statistics_gap(), 0, "a reported zero is available data");
		s.work_board.briefs.get_mut("release").unwrap().metrics.tokens = None;
		s.publish_usage();
		assert_eq!(s.token_statistics_gap(), 1);
		s.snapshot.as_mut().unwrap().work_items.clear();
		s.native_agents.lists.clear();
		s.publish_usage();
		assert_eq!(
			s.token_statistics_gap(),
			0,
			"archiving conversations clears their statistics notice"
		);
		assert!(s.work_board.usage.as_ref().unwrap().is_empty());
	});
}

#[gpui::test]
fn graph_nodes_select_the_exact_agent_and_keep_the_graph_open(cx: &mut gpui::TestAppContext) {
	let (_root, profile, server) = super::super::wire_test_support::fixture(|_| async {});
	server.join().unwrap();
	for (key, selector) in [
		("release", "relation-node-release"),
		("native:release:native-child", "relation-node-native:release:native-child"),
	] {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = fixture(cx);
			s.profile = Some(profile.clone());
			s.workspace.graph_expanded = true;
			s
		});
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
		visual.update(|w, cx| w.draw(cx).clear(cx));
		visual.run_until_parked();
		visual.update(|w, cx| w.draw(cx).clear(cx));
		let node = visual.debug_bounds(selector).unwrap();
		let canvas = visual.debug_bounds("relations-canvas").unwrap();
		assert!(canvas.contains(&node.center()), "the selected card must be visible");
		visual.simulate_mouse_down(node.center(), gpui::MouseButton::Left, Default::default());
		let positions = surface.read_with(visual, |s, _| {
			s.relation_graph().nodes.into_iter().map(|n| (n.key, n.x, n.y)).collect::<Vec<_>>()
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		assert_eq!(
			visual.debug_bounds(selector),
			Some(node),
			"a pending fit cannot move the pressed card"
		);
		visual.simulate_mouse_up(node.center(), gpui::MouseButton::Left, Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.work_board.view.picked_agent.as_deref(), Some(key));
			assert_eq!(
				s.relation_graph().nodes.into_iter().map(|n| (n.key, n.x, n.y)).collect::<Vec<_>>(),
				positions
			);
			assert!(s.handoffs.focus.is_none());
			assert_eq!(s.work_board.focus.as_deref(), Some(key));
			assert!(s.workspace.graph_expanded, "inspection must keep the collaboration map open");
			assert!(!s.workspace_connecting(), "conversation reads stay inside the workspace");
			assert!(
				s.native_agents.selected.is_none(),
				"single-click inspection does not change the recipient"
			);
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		assert!(visual.debug_bounds("agent-usage-popover").is_none());
		let position = visual.debug_bounds(selector).unwrap().center();
		visual.simulate_event(gpui::MouseDownEvent {
			position,
			button: gpui::MouseButton::Left,
			modifiers: Default::default(),
			click_count: 2,
			first_mouse: false,
		});
		visual.simulate_event(gpui::MouseUpEvent {
			position,
			button: gpui::MouseButton::Left,
			modifiers: Default::default(),
			click_count: 2,
		});
		surface.read_with(visual, |s, _| {
			assert!(!s.workspace.graph_expanded);
			assert_eq!(s.work_board.view.picked_agent.as_deref(), Some(key));
			if key.starts_with("native:") {
				assert_eq!(
					s.native_agents.selected.as_ref(),
					Some(&("release".into(), "native-child".into()))
				);
			}
		});
	}
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
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: None,
				question: None,
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
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: None,
				question: None,
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
	visual.update(|w, cx| w.draw(cx).clear(cx));
	visual.run_until_parked();
	visual.update(|w, cx| w.draw(cx).clear(cx));
	visual.run_until_parked();
	visual.update(|w, cx| w.draw(cx).clear(cx));
	assert!(visual.debug_bounds("relations-canvas").is_some());
	// History is disclosed after selecting an agent; both underlying facts survive.
	assert!(visual.debug_bounds("relation-edge-context:42:verify:thread-flow").is_none());
	surface.update(visual, |s, cx| {
		let graph = s.relation_graph();
		assert!(graph.edges.iter().any(|e| e.id == "context:42:verify:thread-flow"));
		assert!(graph.edges.iter().any(|e| e.id == "dependency:flow:verify"));
		s.select_workbench_agent("verify", cx);
	});
	visual.update(|w, cx| w.draw(cx).clear(cx));
	std::thread::sleep(std::time::Duration::from_millis(220));
	visual.update(|w, cx| w.draw(cx).clear(cx));
	let edge = visual.debug_bounds("relation-edge-context:42:verify:thread-flow").unwrap();
	visual.simulate_click(edge.center(), Default::default());
	surface.read_with(visual, |s, _| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(
			s.work_board.view.edge.as_ref().map(|e| e.id.as_str()),
			Some("context:42:verify:thread-flow")
		);
	});
}

#[gpui::test]
fn dragging_a_node_changes_layout_without_switching_the_conversation(
	cx: &mut gpui::TestAppContext,
) {
	for steps in [1, 128] {
		let (surface, visual) = cx.add_window_view(|_, cx| {
			let mut s = fixture(cx);
			s.workspace.graph_expanded = true;
			s
		});
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
		visual.update(|w, cx| w.draw(cx).clear(cx));
		visual.run_until_parked();
		visual.update(|w, cx| w.draw(cx).clear(cx));
		let start = visual.debug_bounds("relation-node-release").unwrap().center();
		visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
		// Pressing freezes the presented layout before measuring world movement.
		let before = surface.read_with(visual, |s, _| {
			let g = s.relation_graph();
			let n = g.nodes.iter().find(|n| n.key == "release").unwrap();
			(n.x, n.y)
		});
		for step in 1..=steps {
			let progress = step as f32 / steps as f32;
			visual.simulate_mouse_move(
				start + gpui::point(gpui::px(50. * progress), gpui::px(30. * progress)),
				gpui::MouseButton::Left,
				Default::default(),
			);
		}
		visual.simulate_mouse_up(
			start + gpui::point(gpui::px(50.), gpui::px(30.)),
			gpui::MouseButton::Left,
			Default::default(),
		);
		surface.read_with(visual, |s, _| {
			let g = s.relation_graph();
			let n = g.nodes.iter().find(|n| n.key == "release").unwrap();
			assert!(
				n.x > before.0 && n.y > before.1,
				"drag with {steps} events: {before:?} -> {:?}",
				(n.x, n.y)
			);
			assert_eq!(s.selected.as_deref(), Some("agent"), "drag with {steps} events");
		});
	}
}

#[test]
fn native_states_keep_requests_errors_and_inactivity_distinct() {
	for (native, label, color) in [
		("active", "Running", crate::ui_theme::BLUE),
		("running", "Running", crate::ui_theme::BLUE),
		("waitingOnApproval", "Approval", AMBER),
		("waitingOnUserInput", "Input needed", AMBER),
		("systemError", "Error", crate::ui_theme::ERROR),
		("idle", "Idle", TEXT_MUTED),
		("notLoaded", "Not running", TEXT_MUTED),
		("new-upstream-state", "Unknown", TEXT_MUTED),
	] {
		let state = native_state(native);
		assert_eq!((state.0, state.2), (label, color));
	}
}

#[gpui::test]
fn outcome_preserves_current_requests_and_invalidates_old_turns(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let mut row = s.board_rows().remove(0);
		for status in [
			"Running",
			"Starting",
			"Needs you",
			"Approval",
			"Input needed",
			"Waiting on work",
			"Status unavailable",
			"Error",
		] {
			row.status = status.into();
			row.review = true;
			apply_outcome(&mut row, "failed");
			assert_eq!(row.status, status);
			assert!(row.review, "unread state must stay independent");
		}
		for (outcome, label, color) in [
			("failed", "Failed", crate::ui_theme::ERROR),
			("interrupted", "Interrupted", TEXT_MUTED),
		] {
			row.status = "Not running".into();
			row.review = true;
			apply_outcome(&mut row, outcome);
			assert_eq!(row.status, label);
			assert_eq!(row.color, color);
			assert!(!row.review);
		}
		let snapshot = s.snapshot.as_mut().unwrap();
		snapshot.pending_events.clear();
		snapshot.dependencies.clear();
		let work = &mut snapshot.work_items[0];
		let key = work.id.clone();
		work.dispatch_state = super::super::AgentDispatchStateDto::Idle;
		work.status = super::super::AgentWorkStatusDto::Open;
		work.active_turn_id = Some("latest".into());
		let row = s.board_rows().into_iter().find(|r| r.key == key).unwrap();
		s.work_board.briefs.insert(
			key.clone(),
			Brief {
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: Some(("old".into(), "failed".into())),
				preview: None,
				question: None,
				resources: None,
				relations: vec![],
				metrics: Default::default(),
				stamp: s.brief_stamp(&row),
				read_at: std::time::Instant::now(),
			},
		);
		assert_eq!(
			s.board_rows().into_iter().find(|r| r.key == key).unwrap().status,
			"Not running"
		);
		s.work_board.briefs.get_mut(&key).unwrap().outcome =
			Some(("latest".into(), "failed".into()));
		assert_eq!(s.board_rows().into_iter().find(|r| r.key == key).unwrap().status, "Failed");
		assert_eq!(
			s.board_rows().into_iter().find(|r| r.key == key).unwrap().status,
			"Failed",
			"derived status must not invalidate its own cache"
		);
		s.snapshot.as_mut().unwrap().work_items[0].updated_at_micros += 1;
		assert_eq!(
			s.board_rows().into_iter().find(|r| r.key == key).unwrap().status,
			"Not running"
		);
	});
}

#[test]
fn new_turn_start_clears_previous_terminal_outcome() {
	use decodex_protocol::{AgentTimelineContent, AgentTimelineEntry};
	let boundary = |position, completed, status: &str| AgentTimelineEntry {
		position,
		content: AgentTimelineContent::TurnBoundary {
			turn_id: position.to_string(),
			completed,
			status: Some(status.into()),
			duration_ms: None,
			usage_summary: None,
			usage: None,
			error: None,
		},
	};
	let entries = vec![boundary(1, true, "failed"), boundary(2, false, "")];
	assert_eq!(latest_outcome(&entries), Some(("2".into(), "inProgress".into())));
}

#[gpui::test]
fn work_status_matrix_prioritizes_live_execution(cx: &mut gpui::TestAppContext) {
	use super::super::{AgentDispatchStateDto as D, AgentWorkStatusDto as S};
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let snapshot = s.snapshot.as_mut().unwrap();
		snapshot.pending_events.clear();
		snapshot.dependencies.clear();
		let mut work = snapshot.work_items[0].clone();
		for (status, idle_label) in [
			(S::Open, "Not running"),
			(S::Resolved, "Marked complete"),
			(S::FollowUp, "Follow-up pending"),
			(S::Wait, "Not running"),
			(S::UserDecision, "Needs you"),
		] {
			work.status = status;
			work.next_check_at_micros = None;
			for (dispatch, label) in [
				(D::Idle, idle_label),
				(D::Dispatching, "Starting"),
				(D::Running, "Running"),
				(D::Unknown, "Status unavailable"),
			] {
				work.dispatch_state = dispatch;
				assert_eq!(
					super::super::dock::progress_state(snapshot, &work).label,
					label,
					"{status:?} / {dispatch:?}"
				);
			}
		}
	});
}

#[gpui::test]
fn usage_waits_for_complete_batches_and_retains_the_published_read(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let rows = s.factory_rows();
		assert!(rows.len() > 4);
		for (index, row) in rows.iter().enumerate() {
			s.work_board.briefs.insert(
				row.key.clone(),
				Brief {
					output: None,
					source_thread: row.thread.clone(),
					assignments: Default::default(),
					outcome: None,
					preview: None,
					question: None,
					resources: None,
					relations: vec![],
					metrics: Default::default(),
					stamp: s.brief_stamp(row),
					read_at: std::time::Instant::now(),
				},
			);
			// A running task can change its stamp before other batches finish.
			s.work_board.briefs.get_mut(&row.key).unwrap().stamp = "older-read".into();
			s.publish_usage();
			if index + 1 < rows.len() {
				assert!(s.work_board.usage.is_none(), "partial reads must not resize the strip");
			}
		}
		let published = s.work_board.usage.as_ref().unwrap();
		assert_eq!(published.len(), rows.len(), "missing token counters also complete a read");
		s.debug_reload_graph();
		assert_eq!(s.work_board.usage.as_ref().unwrap().len(), rows.len());
		assert!(s.work_board.briefs.values().all(|brief| !brief.stamp.is_empty()));
		assert_eq!(s.work_board.reload_pending.len(), rows.len());
		for (index, row) in rows.iter().enumerate() {
			s.work_board.briefs.get_mut(&row.key).unwrap().stamp = s.brief_stamp(row);
			s.work_board.reload_pending.remove(&row.key);
			s.publish_usage();
			assert_eq!(
				s.work_board.usage.as_ref().unwrap().len(),
				rows.len(),
				"reload batch {index} must retain the complete strip"
			);
		}
		let first = &rows[0];
		s.work_board.briefs.get_mut(&first.key).unwrap().source_thread = Some("replaced".into());
		s.publish_usage();
		assert_eq!(s.work_board.usage.as_ref().unwrap().len(), rows.len());
		assert_eq!(s.work_board.usage.as_ref().unwrap()[&first.key].0, first.thread);
	});
}

#[gpui::test]
fn archived_coordinator_stays_out_of_the_active_graph_and_statistics(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let root = s.root_id().unwrap();
		s.snapshot.as_mut().unwrap().archived_work_ids.insert(root.clone());
		let rows = s.factory_rows();
		assert!(rows.iter().all(|row| row.key != root));
		for row in &rows {
			s.work_board.briefs.insert(
				row.key.clone(),
				Brief {
					output: None,
					source_thread: row.thread.clone(),
					assignments: Default::default(),
					outcome: None,
					preview: None,
					question: None,
					resources: None,
					relations: vec![],
					metrics: Default::default(),
					stamp: s.brief_stamp(row),
					read_at: std::time::Instant::now(),
				},
			);
		}
		assert!(s.publish_usage());
		assert_eq!(s.work_board.usage.as_ref().unwrap().len(), rows.len());
		let graph = s.relation_graph();
		assert!(graph.nodes.iter().all(|node| node.key != root));
		assert!(graph.edges.iter().all(|edge| edge.from != root && edge.to != root));
		assert!(
			s.snapshot.as_ref().unwrap().work_items.iter().any(|work| {
				work.id == "release" && work.parent_goal_id.as_ref() == Some(&root)
			}),
			"hiding an archived conversation must not rewrite task ownership"
		);
		let row = s.board_rows().into_iter().find(|row| row.key == root).unwrap();
		assert_eq!(row.status, "Archived");
		assert_eq!(s.graph_preview(&row).1, "Archived conversation");
		s.selected = Some(root);
		assert!(s.selected_is_archived());
	});
}

#[gpui::test]
fn parent_progress_keeps_child_preview_and_geometry(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let mut row = s.board_rows().into_iter().find(|row| row.native).unwrap();
		// Some provider records contain only a nickname. The task comes from
		// this child's native input rather than its parent's reply or another child.
		row.title = row.owner.clone();
		let stamp = s.brief_stamp(&row);
		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: Some(activity::Preview {
					running: None,
					recent: vec![],
					turn: "child-turn".into(),
					task: "Review the route".into(),
					text: "route.rs".into(),
				}),
				question: None,
				resources: None,
				relations: vec![],
				metrics: Default::default(),
				stamp: stamp.clone(),
				read_at: std::time::Instant::now(),
			},
		);
		let before = s
			.relation_graph()
			.nodes
			.into_iter()
			.map(|node| (node.key, node.x, node.y))
			.collect::<Vec<_>>();
		let parent = s
			.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == row.work)
			.unwrap();
		parent.updated_at_micros += 1;
		parent.active_turn_id = Some("next-parent-step".into());
		assert_eq!(s.brief_stamp(&row), stamp);
		assert_eq!(s.graph_preview(&row), ("Review the route".into(), "route.rs".into()));
		assert_eq!(
			s.relation_graph()
				.nodes
				.into_iter()
				.map(|node| (node.key, node.x, node.y))
				.collect::<Vec<_>>(),
			before
		);
		s.work_board.briefs.get_mut(&row.key).unwrap().source_thread = Some("other-child".into());
		assert_eq!(s.graph_preview(&row), (String::new(), String::new()));
	});
}

#[gpui::test]
fn graph_recovers_native_responsibility_from_exact_parent_assignment(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let mut row = s.board_rows().into_iter().find(|row| row.native).unwrap();
		row.title = row.owner.clone();
		let parent = s.board_rows().into_iter().find(|r| r.key == row.work).unwrap();
		s.work_board.briefs.insert(
			parent.key.clone(),
			Brief {
				output: None,
				source_thread: parent.thread.clone(),
				assignments: BTreeMap::from([(
					row.thread.clone().unwrap(),
					"Review connector routing".into(),
				)]),
				outcome: None,
				preview: None,
				question: None,
				resources: None,
				relations: vec![],
				metrics: Default::default(),
				stamp: s.brief_stamp(&parent),
				read_at: std::time::Instant::now(),
			},
		);
		assert_eq!(s.graph_preview(&row), ("Review connector routing".into(), String::new()));
		assert_eq!(s.graph_purpose(&row), "Review connector routing");
		let brief = s.work_board.briefs.get_mut(&parent.key).unwrap();
		brief.assignments = BTreeMap::from([("sibling".into(), "Review connector routing".into())]);
		assert_eq!(s.graph_preview(&row), (String::new(), String::new()));
		assert!(s.graph_purpose(&row).is_empty(), "a sibling's assignment cannot name this agent");
		let brief = s.work_board.briefs.get_mut(&parent.key).unwrap();
		brief.assignments =
			BTreeMap::from([(row.thread.clone().unwrap(), "Review connector routing".into())]);
		brief.source_thread = Some("previous-parent-thread".into());
		assert_eq!(s.graph_preview(&row), (String::new(), String::new()));
	});
}

#[gpui::test]
fn graph_purposes_keep_manual_names_and_replace_only_incomplete_legacy_prefixes(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let mut row = s.board_rows().into_iter().find(|row| row.key == "release").unwrap();
		row.title = "Reconnect checks".into();
		let request = "Please verify that a browser reconnect preserves the final paragraph and every code block. Report the evidence.";
		s.work_board.briefs.insert(row.key.clone(), Brief {
			output: None, source_thread: row.thread.clone(), assignments: Default::default(), outcome: None,
			preview: Some(activity::Preview { turn: "reference".into(), task: request.into(), text: "Checks passed".into(), running: None, recent: vec![] }),
			question: None, resources: None, relations: vec![], metrics: Default::default(), stamp: s.brief_stamp(&row), read_at: std::time::Instant::now(),
		});
		assert_eq!(s.graph_purpose(&row), "Reconnect checks");
		row.title = request.chars().take(64).collect();
		assert_eq!(s.graph_purpose(&row), "Verify that a browser reconnect preserves the final paragraph and every code block");
		row.title = "Help me plan a small soup dinner for two. Have two agents work i".into();
		assert_eq!(s.graph_purpose(&row), "Plan a small soup dinner for two");
		row.title = s.relation_agent_number(&row);
		assert!(s.graph_purpose(&row).is_empty(), "do not repeat the agent's number as its purpose");
	});
}

#[gpui::test]
fn native_completion_requests_fresh_metrics_without_blank_preview(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let row = s.board_rows().into_iter().find(|row| row.native).unwrap();
		let stamp = s.brief_stamp(&row);
		s.work_board.briefs.insert(
			row.key.clone(),
			Brief {
				output: None,
				source_thread: row.thread.clone(),
				assignments: Default::default(),
				outcome: None,
				preview: Some(activity::Preview {
					running: None,
					recent: vec![],
					turn: "child-turn".into(),
					task: "Review the route".into(),
					text: "route.rs".into(),
				}),
				question: None,
				resources: None,
				relations: vec![],
				metrics: Default::default(),
				stamp: stamp.clone(),
				read_at: std::time::Instant::now(),
			},
		);
		let agent = s
			.native_agents
			.lists
			.get_mut(&row.work)
			.unwrap()
			.iter_mut()
			.find(|a| Some(&a.thread_id) == row.thread.as_ref())
			.unwrap();
		agent.status = if agent.status == "idle" { "active" } else { "idle" }.into();
		assert_ne!(
			s.brief_stamp(&row),
			stamp,
			"a status transition must read the final counters immediately"
		);
		assert_eq!(
			s.graph_preview(&row).1,
			"route.rs",
			"keep the last valid preview until its replacement arrives"
		);
	});
}

#[gpui::test]
fn moving_across_card_children_does_not_focus_or_rearrange_the_graph(
	cx: &mut gpui::TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut s = fixture(cx);
		s.workspace.graph_expanded = true;
		s
	});
	visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(900.)));
	// Finish the workspace's opening resize before testing pointer-only redraws.
	visual.update(|w, cx| w.draw(cx).clear(cx));
	std::thread::sleep(std::time::Duration::from_millis(250));
	for _ in 0..2 {
		visual.update(|w, cx| w.draw(cx).clear(cx));
		visual.run_until_parked();
	}
	let bounds = visual.debug_bounds("relation-node-release").unwrap();
	let layout = surface.read_with(visual, |s, _| s.relation_graph().nodes);
	for fraction in [0.15, 0.35, 0.65, 0.85, 0.35] {
		let p = bounds.origin + gpui::point(bounds.size.width * 0.5, bounds.size.height * fraction);
		visual.simulate_mouse_move(p, None, Default::default());
		visual.update(|w, cx| w.draw(cx).clear(cx));
		surface.read_with(visual, |s, _| assert_eq!(s.relation_context(), None));
		// Pointer-only movement must not open an inspection or move the graph.
		assert_eq!(visual.debug_bounds("relation-node-release").unwrap().origin, bounds.origin);
		surface.read_with(visual, |s, _| {
			for (before, after) in layout.iter().zip(s.relation_graph().nodes) {
				assert_eq!((&before.key, before.x, before.y), (&after.key, after.x, after.y));
			}
		});
	}
}

#[gpui::test]
fn observed_collaboration_survives_page_rollover_but_not_another_thread(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(fixture);
	surface.update(cx, |s, _| {
		let row = s.board_rows().into_iter().find(|row| row.key == "release").unwrap();
		let make = |id: usize, status: &str| {
			(
				format!("item-{id}"),
				AgentCollaborationDto {
					sender_thread_id: row.thread.clone().unwrap(),
					receiver_thread_ids: vec!["target".into()],
					tool: "sendInput".into(),
					status: status.into(),
					prompt: format!("Request {id}"),
					results: vec![],
				},
			)
		};
		let mut previous = Brief {
			source_thread: row.thread.clone(),
			output: None,
			assignments: Default::default(),
			outcome: None,
			preview: None,
			question: None,
			resources: None,
			relations: (0..64).map(|id| make(id, "inProgress")).collect(),
			metrics: Default::default(),
			stamp: s.brief_stamp(&row),
			read_at: std::time::Instant::now(),
		};
		let mut next = Brief {
			source_thread: row.thread.clone(),
			output: None,
			assignments: Default::default(),
			outcome: None,
			preview: None,
			question: None,
			resources: None,
			relations: vec![make(63, "completed"), make(64, "completed")],
			metrics: Default::default(),
			stamp: s.brief_stamp(&row),
			read_at: std::time::Instant::now(),
		};
		next.retain_relations(&previous);
		assert_eq!(next.relations.len(), 64);
		assert_eq!(next.relations[0].0, "item-1");
		assert_eq!(next.relations[62].1.status, "completed");
		previous.source_thread = Some("another-thread".into());
		next.relations = vec![make(65, "completed")];
		next.retain_relations(&previous);
		assert_eq!(next.relations.len(), 1);
	});
}
