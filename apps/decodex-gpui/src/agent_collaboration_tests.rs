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
		.debug_bounds("collaboration-call:ref-sage:request-sage-iris:ref-iris")
		.expect("recorded exchange is visible without selecting its owner");
	visual.simulate_click(header.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Keep the current draft");
		assert!(s.work_board.collaboration_expanded.is_some());
		let before: Vec<_> = exchanges(&s.relation_graph(), &s.factory_rows(), &[], &[])
			.into_iter()
			.map(|case| case.id)
			.collect();
		assert!(before.contains(&"call:ref-mira:request-mira-nora:ref-nora".into()));
		assert!(before.contains(&"call:ref-sage:request-sage-iris:ref-iris".into()));
	});
	let participant = visual
		.debug_bounds(
			"collaboration-participant-call:ref-sage:request-sage-iris:ref-iris-native:sage:ref-iris",
		)
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
	assert!(
		visual.debug_bounds("collaboration-call:ref-mira:request-mira-nora:ref-nora").is_some()
	);
	assert!(visual.debug_bounds("workbench-history").is_none());
	assert!(visual.debug_bounds("workbench-composer").is_none());
}

#[gpui::test]
fn native_calls_keep_exact_sources_and_never_join_unrelated_snapshots(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(|cx| {
		let mut s = AgentSurface::new(cx);
		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);
		s
	});
	surface.read_with(cx, |s, _| {
		let mut graph = s.relation_graph();
		let rows = s.factory_rows();
		let original = exchanges(&graph, &rows, &[], &[])
			.into_iter()
			.find(|case| {
				case.from.key == "mira"
					&& case.to.as_ref().and_then(|row| row.thread.as_deref()) == Some("ref-nora")
			})
			.unwrap();
		assert_eq!(original.state, "Recorded");
		assert_eq!(original.request.as_deref(), Some("Check reconnect preserves the full reply."));
		let mut snapshot = original.edge.clone().unwrap();
		snapshot.id = "unlinked-snapshot".into();
		snapshot.from = original.to.as_ref().unwrap().key.clone();
		snapshot.to = original.from.key.clone();
		snapshot.kind = Kind::Returned;
		snapshot.excerpt = Some("Previous result".into());
		let mut opaque = original.edge.clone().unwrap();
		opaque.id = "opaque-interaction".into();
		opaque.excerpt = None;
		graph.edges.push(opaque);
		graph.edges.push(snapshot.clone());
		let cases = exchanges(&graph, &rows, &[], &[]);
		assert!(cases.iter().find(|case| case.id == "opaque-interaction").unwrap().earlier);
		assert_eq!(cases.iter().find(|case| case.id == original.id).unwrap().state, "Recorded");
		assert_eq!(
			cases.iter().find(|case| case.id == "unlinked-snapshot").unwrap().state,
			"Reported"
		);
		snapshot.kind = Kind::Context;
		snapshot.source = None;
		graph.edges = vec![snapshot];
		assert!(exchanges(&graph, &rows, &[], &[]).is_empty());
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
	let row_id = "collaboration-row-call:ref-sage:request-sage-iris:ref-iris";
	let control_id = "collaboration-call:ref-sage:request-sage-iris:ref-iris";
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

fn receipt(id: i64, kind: decodex_protocol::AgentExchangeKind) -> AgentExchangeDto {
	AgentExchangeDto {
		event_id: id,
		source_event_id: format!("source-{id}"),
		work_id: "sage".into(),
		other_work_id: Some("agent".into()),
		target_dispatch_state: None,
		thread_id: Some("ref-sage".into()),
		turn_id: Some(format!("turn-{id}")),
		item_id: None,
		instruction_event_id: None,
		excerpt: "Use Maple".into(),
		truncated: false,
		kind,
		delivered_turn_id: None,
		assessment: None,
		assessment_note: None,
		outcome: None,
		created_at_micros: id,
	}
}

#[gpui::test]
fn managed_receipts_keep_busy_delivery_and_assessment_separate(cx: &mut gpui::TestAppContext) {
	use decodex_protocol::{AgentExchangeAssessment as A, AgentExchangeKind as K};
	let surface = cx.new(|cx| {
		let mut s = AgentSurface::new(cx);
		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);
		s
	});
	surface.read_with(cx, |s, _| {
		let mut graph = s.relation_graph();
		graph.edges.clear();
		let rows = s.factory_rows();
		let mut busy = receipt(10, K::NotSent);
		busy.work_id = "agent".into();
		busy.thread_id = Some("ref-agent".into());
		busy.other_work_id = Some("sage".into());
		let mut instruction = receipt(11, K::Instruction);
		let cases = exchanges(&graph, &rows, &[busy.clone(), instruction.clone()], &[]);
		assert_eq!(
			cases.iter().find(|case| case.state == "Not sent").unwrap().summary,
			"Busy · instruction not sent or queued"
		);
		assert_eq!(
			cases.iter().find(|case| case.id == "managed:sage:11").unwrap().state,
			"Unconfirmed"
		);
		instruction.delivered_turn_id = Some("next-turn".into());
		let cases = exchanges(&graph, &rows, &[instruction.clone()], &[]);
		assert_eq!(cases[0].state, "Continuing");
		let mut result = receipt(12, K::Result);
		result.instruction_event_id = Some(11);
		result.outcome = Some("completed".into());
		result.delivered_turn_id = Some("manager-review".into());
		let cases = exchanges(&graph, &rows, &[instruction.clone(), result.clone()], &[]);
		assert_eq!(cases.len(), 1);
		assert_eq!(cases[0].id, "managed:sage:11");
		assert_eq!(cases[0].state, "To review");
		assert!(!cases[0].earlier);
		for (assessment, state, earlier) in [
			(A::FollowUp, "Changes asked", false),
			(A::Wait, "Waiting", false),
			(A::UserDecision, "Decision", false),
			(A::Accepted, "Accepted", true),
		] {
			result.assessment = Some(assessment);
			let cases = exchanges(&graph, &rows, &[instruction.clone(), result.clone()], &[]);
			assert_eq!(cases[0].state, state);
			assert_eq!(cases[0].earlier, earlier);
		}
		result.assessment = None;
		result.instruction_event_id = None;
		assert_eq!(
			exchanges(&graph, &rows, &[instruction, result], &[]).len(),
			2,
			"missing linkage must not be guessed from participants or text"
		);
	});
}

#[gpui::test]
fn question_retirement_keeps_later_questions_and_rejects_rebound_sources(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(|cx| {
		let mut s = AgentSurface::new(cx);
		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);
		s
	});
	surface.read_with(cx, |s, _| {
		let mut graph = s.relation_graph();
		graph.edges.clear();
		let rows = s.factory_rows();
		let question = |item: &str, pending| AgentExchangeQuestionDto {
			work_id: "sage".into(),
			thread_id: "ref-sage".into(),
			turn_id: "turn".into(),
			item_id: item.into(),
			question: decodex_protocol::AgentAsyncQuestionDto {
				arrived_live: false,
				id: decodex_protocol::agent_async_question_id(item, 0),
				title: "Which name?".into(),
				options: vec![],
			},
			pending,
		};
		let mut old = question("old", false);
		let later = question("later", true);
		let cases = exchanges(&graph, &rows, &[], &[old.clone(), later]);
		assert_eq!(cases.iter().filter(|case| case.state == "Question").count(), 1);
		assert_eq!(cases.iter().filter(|case| case.earlier).count(), 1);
		assert_ne!(cases[0].id, cases[1].id);
		old.thread_id = "another-native-thread".into();
		assert!(exchanges(&graph, &rows, &[], &[old]).is_empty());
	});
}

struct WireView(gpui::Entity<AgentSurface>);
impl gpui::Render for WireView {
	fn render(&mut self, _: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| s.render_agent_workbench(cx))
	}
}

#[gpui::test]
fn source_receipts_cross_the_local_wire_without_switching_the_active_chat(
	cx: &mut gpui::TestAppContext,
) {
	use crate::shell::agent_surface::wire_test_support;
	use decodex_protocol::{
		AgentExchangeKind, CURRENT_VERSION, ClientMessage, QueryPayload, QueryResultEnvelope,
		QueryResultPayload, ServerId, ServerMessage,
	};
	use futures_util::{SinkExt as _, StreamExt as _};
	use tokio_tungstenite::tungstenite::Message;
	let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("query frame")
		};
		let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
			panic!("read-only query")
		};
		assert!(matches!(query.payload, QueryPayload::GetAgentExchanges { before: None }));
		let mut entry = receipt(70, AgentExchangeKind::NotSent);
		entry.work_id = "agent".into();
		entry.thread_id = Some("ref-agent".into());
		entry.other_work_id = Some("sage".into());
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(wire_test_support::SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentExchanges(AgentExchangesResult::Available {
				source: EntityId::new("collaboration-binding").unwrap(),
				entries: vec![entry],
				questions: vec![],
				questions_truncated: false,
				next_before: None,
			}),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	});
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(|cx| {
			let mut s = AgentSurface::new(cx);
			s.visual_workspace_fixture(cx);
			s.visual_workbench_page("workbench", cx);
			s.snapshot.as_mut().unwrap().runtime_source =
				Some(EntityId::new("collaboration-binding").unwrap());
			s.composer.update(cx, |input, cx| input.set_content("Keep the Main draft", cx));
			s.profile = Some(profile);
			s
		});
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		WireView(surface)
	});
	let surface = view.read_with(visual, |view, _| view.0.clone());
	surface.update(visual, |s, cx| s.read_collaboration(None, true, cx));
	visual.run_until_parked();
	server.join().unwrap();
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.work_board.collaboration.entries.len(), 1);
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Keep the Main draft");
		assert_eq!(
			s.collaboration_cases(&s.relation_graph(), &s.factory_rows())[0].state,
			"Not sent"
		);
	});
	assert!(visual.debug_bounds("collaboration-unsent:70").is_some());
}

#[gpui::test]
fn native_final_reports_keep_their_own_turn_and_do_not_invent_acceptance(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(|cx| {
		let mut s = AgentSurface::new(cx);
		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);
		s
	});
	surface.update(cx, |s, _| {
		let key = "native:sage:ref-iris";
		s.native_agents
			.lists
			.get_mut("sage")
			.unwrap()
			.iter_mut()
			.find(|agent| agent.thread_id == "ref-iris")
			.unwrap()
			.status = "idle".into();
		let row = s.factory_rows().into_iter().find(|row| row.key == key).unwrap();
		let stamp = s.brief_stamp(&row);
		let brief = s.work_board.briefs.get_mut(key).unwrap();
		brief.stamp = stamp;
		brief.outcome = Some(("report-turn".into(), "completed".into()));
		brief.preview.as_mut().unwrap().turn = "report-turn".into();
		brief.preview.as_mut().unwrap().task = "Check compact spacing".into();
		brief.output = Some(super::super::super::activity::Output {
			turn: "report-turn".into(),
			text: "The compact layout fits 24 agents.".into(),
			truncated: false,
		});
		let cases = s.collaboration_cases(&s.relation_graph(), &s.factory_rows());
		let report =
			cases.iter().find(|case| case.id == "native-report:ref-iris:report-turn").unwrap();
		assert_eq!(report.state, "Reported");
		assert_eq!(report.title, "Check compact spacing");
		assert_eq!(report.to.as_ref().unwrap().work, "sage");
		assert_eq!(report.source_thread.as_deref(), Some("ref-iris"));
		assert_eq!(report.source_turn.as_deref(), Some("report-turn"));
		assert!(!report.earlier);
		assert!(report.summary.contains("24 agents"));
		s.work_board.briefs.get_mut(key).unwrap().output.as_mut().unwrap().turn =
			"unrelated-turn".into();
		assert!(
			!s.collaboration_cases(&s.relation_graph(), &s.factory_rows())
				.iter()
				.any(|case| case.id.starts_with("native-report:"))
		);
	});
}
