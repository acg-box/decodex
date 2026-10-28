use std::{thread, time::Duration};

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{
	AppContext as _, Entity, Render, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext,
	Window,
};
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::{
	wire_test_support::{self, SERVER},
	work_board::{
		activity::Output,
		workbench::collaboration::{
			AgentExchangeDto, AgentExchangeQuestionDto, AgentExchangesResult, AgentSurface,
			Context, EntityId, IntoElement, Kind, cases,
		},
	},
};
use decodex_protocol::{
	AgentAsyncQuestionDto, AgentExchangeAssessment, AgentPeerExchangeDto, AgentQuestionAnswerDto,
	CURRENT_VERSION, ClientMessage, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	ServerId, ServerMessage,
};

struct WireView(Entity<AgentSurface>);
impl Render for WireView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| s.render_agent_workbench(cx))
	}
}

fn review_capture(completed: bool) -> crate::shell::agent_surface::work_board::review::Capture {
	use serde_json::json;
	let item = |turn: &str, id: &str, kind: &str, text: &str, review| {
		json!({"position":1,"content":{
			"type":"item","turn_id":turn,"item_id":id,"kind":kind,"phase":"final_answer","text":text,"truncated":false,"attachments":[],
			"activity": if kind=="commandExecution" {json!({"turn_id":turn,"item_id":id,"kind":kind,"status":"completed","label":"File read","detail":"Exit code 0","review":review})} else {json!(null)}
		}})
	};
	let boundary = |done| json!({"position":3,"content":{"type":"turn_boundary","turn_id":"review-turn","completed":done,"status":if done {Some("completed")} else {None}}});
	serde_json::from_value(json!({
		"thread":"lead-thread","agents":[{"thread_id":"reviewer-thread","parent_thread_id":"lead-thread","title":"Reviewer","task":"Check the implementation design","status":if completed {"idle"} else {"active"}}],
		"pages":[
			{"thread_id":"lead-thread","entries":[item("lead-turn","prepare","commandExecution","",json!({"kind":"prepare"}))],"next_cursor":null},
			{"thread_id":"reviewer-thread","entries":[item("review-turn","persona","commandExecution","",json!({"kind":"reviewer","role":"implementation_readiness"})),item("review-turn","report","agentMessage",if completed {"Read the original instructions.\n\nOne blocker: completion does not establish acceptance."} else {"Checking the current design."},json!(null)),boundary(completed)],"next_cursor":null}
		]
	})).expect("review fixture")
}

#[gpui::test]
fn review_events_share_graph_and_sidebar_without_claiming_acceptance(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut surface = AgentSurface::new(cx);
		surface.apply_review_capture(review_capture(false), cx);
		surface
	});

	surface.read_with(visual, |s, _| {
		let rows = s.factory_rows();
		let reviewer = rows.iter().find(|row| row.native).unwrap();
		assert_eq!(s.reviewer_role(reviewer).as_deref(), Some("implementation readiness"));
		assert_eq!(s.review_phase(reviewer), Some("Reviewing"));
		let cases = s.collaboration_cases(&s.relation_graph(), &rows);
		assert!(
			cases.iter().any(|case| case.state == "Reviewing" && case.from.key == reviewer.key)
		);
	});
	surface.update(visual, |s, cx| {
		s.native_agents.lists.get_mut("agent").unwrap()[0].status = "waitingOnApproval".into();
		let pending = s.factory_rows().into_iter().find(|row| row.native).unwrap();
		assert_eq!(s.review_phase(&pending), Some("Needs approval"));
		s.apply_review_capture(review_capture(true), cx);
		s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
		let rows = s.factory_rows();
		let cases = s.collaboration_cases(&s.relation_graph(), &rows);
		let report = cases.iter().find(|case| case.state == "Report ready").unwrap();
		assert!(report.summary.contains("blocker"));
		assert!(!report.summary.contains("Read the original"));
		assert_eq!(report.source_thread.as_deref(), Some("reviewer-thread"));
		assert_eq!(report.source_turn.as_deref(), Some("review-turn"));
		assert!(!cases.iter().any(|case| case.state == "Accepted"));
		s.toggle_collaboration_exchange(&report.id, None, cx);
		assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		assert_ne!(
			s.native_agents.selected.as_ref().map(|(_, thread)| thread.as_str()),
			Some("reviewer-thread")
		);
	});
}

#[gpui::test]
fn ordinary_followup_and_rebound_threads_do_not_inherit_review_state(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut surface = AgentSurface::new(cx);
		surface.apply_review_capture(review_capture(true), cx);
		surface
	});
	surface.update(visual, |s, _| {
		let rows = s.factory_rows();
		let lead = rows.iter().find(|row| !row.native).unwrap();
		assert!(s.lead_review(lead).is_some());
		s.snapshot.as_mut().unwrap().work_items[0].active_turn_id = Some("new-live-turn".into());
		assert!(s.lead_review(lead).is_none(), "new native turn immediately retires the old phase");
		s.snapshot.as_mut().unwrap().work_items[0].active_turn_id = None;
		let brief = s.work_board.briefs.get_mut(&lead.key).unwrap();
		brief.preview.as_mut().unwrap().turn = "ordinary-followup".into();
		assert!(s.lead_review(lead).is_none());
		let reviewer = rows.iter().find(|row| row.native).unwrap();
		let brief = s.work_board.briefs.get_mut(&reviewer.key).unwrap();
		brief.preview.as_mut().unwrap().turn = "ordinary-native-followup".into();
		assert!(s.reviewer_role(reviewer).is_none());
		let brief = s.work_board.briefs.get_mut(&reviewer.key).unwrap();
		brief.preview.as_mut().unwrap().turn = "review-turn".into();
		let brief = s.work_board.briefs.get_mut(&reviewer.key).unwrap();
		brief.source_thread = Some("different-thread".into());
		assert!(s.reviewer_role(reviewer).is_none());
	});
}

#[gpui::test]
fn collaboration_stays_global_and_expands_without_switching_chat_or_draft(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut surface = AgentSurface::new(cx);

		surface.visual_workspace_fixture(cx);
		surface.visual_workbench_page("workbench", cx);

		surface.workspace.graph_expanded = false;

		surface
	});

	visual.simulate_resize(gpui::size(gpui::px(1_536.), gpui::px(1_024.)));
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

		let before: Vec<_> = cases::exchanges(&s.relation_graph(), &s.factory_rows(), &[], &[])
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
fn native_calls_keep_exact_sources_and_never_join_unrelated_snapshots(cx: &mut TestAppContext) {
	let surface = cx.new(|cx| {
		let mut s = AgentSurface::new(cx);

		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);

		s
	});

	surface.read_with(cx, |s, _| {
		let mut graph = s.relation_graph();
		let rows = s.factory_rows();
		let original = cases::exchanges(&graph, &rows, &[], &[])
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

		let cases = cases::exchanges(&graph, &rows, &[], &[]);

		assert!(cases.iter().find(|case| case.id == "opaque-interaction").unwrap().earlier);
		assert_eq!(cases.iter().find(|case| case.id == original.id).unwrap().state, "Recorded");
		assert_eq!(
			cases.iter().find(|case| case.id == "unlinked-snapshot").unwrap().state,
			"Reported"
		);

		snapshot.kind = Kind::Context;
		snapshot.source = None;
		graph.edges = vec![snapshot];

		assert!(cases::exchanges(&graph, &rows, &[], &[]).is_empty());
	});
}

#[gpui::test]
fn exchange_disclosure_animates_in_both_directions_without_changing_the_draft(
	cx: &mut TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut s = AgentSurface::new(cx);

		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);

		s.work_board.collaboration_expanded = None;
		s.workspace.graph_expanded = false;
		s.workspace.agent_panel_width = 240.;

		s.composer.update(cx, |input, cx| input.set_content("Preserve this draft", cx));

		s
	});

	visual.simulate_resize(gpui::size(gpui::px(1_536.), gpui::px(1_024.)));
	visual.update(crate::ui_frame::draw_frame);
	visual.update(crate::ui_frame::draw_frame);

	thread::sleep(Duration::from_millis(240));

	visual.update(crate::ui_frame::draw_frame);

	let row_id = "collaboration-row-call:ref-sage:request-sage-iris:ref-iris";
	let control_id = "collaboration-call:ref-sage:request-sage-iris:ref-iris";
	let height = |visual: &mut VisualTestContext| {
		f32::from(visual.debug_bounds(row_id).unwrap().size.height)
	};
	let closed = height(visual);
	let header = visual.debug_bounds(control_id).unwrap();

	visual.simulate_click(header.center(), Default::default());

	// Inspect every presented frame. A later sample can miss the middle when
	// native layout takes longer; the contract is visible interpolation.
	let mut opening = Vec::new();

	for _ in 0..2 {
		visual.update(crate::ui_frame::draw_frame);
		opening.push(height(visual));
	}

	thread::sleep(Duration::from_millis(55));

	visual.update(crate::ui_frame::draw_frame);
	opening.push(height(visual));

	thread::sleep(Duration::from_millis(240));

	visual.update(crate::ui_frame::draw_frame);

	let open = height(visual);

	assert!(open > closed + 5., "recorded exchange must open: {closed}, {open}");
	assert!(
		opening.iter().any(|height| *height > closed && *height < open),
		"opening must interpolate: {opening:?}"
	);

	let row = visual.debug_bounds(row_id).unwrap();
	let panel = visual.debug_bounds("workbench-body").unwrap();

	assert!(row.left() >= panel.left() && row.right() <= panel.right());

	let header = visual.debug_bounds(control_id).unwrap();

	visual.simulate_click(header.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);

	let first_closing = height(visual);

	thread::sleep(Duration::from_millis(55));

	visual.update(crate::ui_frame::draw_frame);

	let closing = [first_closing, height(visual)];

	assert!(
		closing.iter().any(|height| *height > closed && *height < open),
		"closing must interpolate: {closing:?}"
	);

	thread::sleep(Duration::from_millis(240));

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
		collaboration: None,
	}
}

#[gpui::test]
fn source_reference_fits_one_line_and_copies_its_exact_value(cx: &mut TestAppContext) {
	const THREAD: &str = "01a15360-31f5-7523-be26-3f961aa42d19";

	let (surface, visual) = cx.add_window_view(|_, cx| {
		let mut s = AgentSurface::new(cx);

		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);

		s.workspace.graph_expanded = false;
		s.workspace.agent_panel_width = 240.;

		let mut record = receipt(70, decodex_protocol::AgentExchangeKind::NotSent);

		record.thread_id = Some(THREAD.into());
		s.work_board.collaboration.source =
			s.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.clone());

		s.work_board.collaboration.entries.insert(70, record);

		s.work_board.collaboration_expanded = Some("unsent:70".into());
		s.work_board.collaboration.source_open = Some("unsent:70".into());

		s.composer.update(cx, |input, cx| input.set_content("Preserve this draft", cx));

		s
	});

	visual.simulate_resize(gpui::size(gpui::px(1_536.), gpui::px(1_024.)));
	visual.update(crate::ui_frame::draw_frame);
	// Hit-test the presented disclosure, independently of asynchronous image loads.
	visual.update(crate::ui_frame::draw_frame);

	thread::sleep(Duration::from_millis(240));

	visual.update(crate::ui_frame::draw_frame);

	let value = visual.debug_bounds("collaboration-reference-unsent:70-Thread").unwrap();
	let panel = visual.debug_bounds("workbench-body").unwrap();

	assert!(f32::from(value.size.height) <= 24.);
	assert!(value.left() >= panel.left() && value.right() <= panel.right());

	let copy = visual.debug_bounds("copy-collaboration-reference-unsent:70-Thread").unwrap();
	let distance = (copy.bottom() - panel.bottom() + gpui::px(24.)).max(gpui::px(0.));

	visual.simulate_event(ScrollEvent {
		position: panel.center(),
		delta: ScrollDelta::Pixels(gpui::point(gpui::px(0.), -distance)),
		..Default::default()
	});
	visual.update(crate::ui_frame::draw_frame);

	thread::sleep(Duration::from_millis(300));

	visual.update(crate::ui_frame::draw_frame);

	let copy = visual.debug_bounds("copy-collaboration-reference-unsent:70-Thread").unwrap();

	assert!(panel.contains(&copy.center()), "the source action must be reachable by scrolling");

	visual.simulate_click(copy.center(), Default::default());
	visual.update(|_, cx| {
		assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some(THREAD));
	});
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Preserve this draft");
		assert_eq!(s.work_board.collaboration.entries[&70].thread_id.as_deref(), Some(THREAD));
	});
}

#[gpui::test]
fn managed_receipts_keep_busy_delivery_and_assessment_separate(cx: &mut TestAppContext) {
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
		let mut busy = receipt(10, decodex_protocol::AgentExchangeKind::NotSent);

		busy.work_id = "agent".into();
		busy.thread_id = Some("ref-agent".into());
		busy.other_work_id = Some("sage".into());

		let mut instruction = receipt(11, decodex_protocol::AgentExchangeKind::Instruction);
		let cases = cases::exchanges(&graph, &rows, &[busy.clone(), instruction.clone()], &[]);

		assert_eq!(
			cases.iter().find(|case| case.state == "Not sent").unwrap().summary,
			"Busy · instruction not sent or queued"
		);
		assert_eq!(
			cases.iter().find(|case| case.id == "managed:sage:11").unwrap().state,
			"Unconfirmed"
		);

		instruction.delivered_turn_id = Some("next-turn".into());

		let cases = cases::exchanges(&graph, &rows, &[instruction.clone()], &[]);

		assert_eq!(cases[0].state, "Continuing");

		let mut result = receipt(12, decodex_protocol::AgentExchangeKind::Result);

		result.instruction_event_id = Some(11);
		result.outcome = Some("completed".into());
		result.delivered_turn_id = Some("manager-review".into());

		let cases = cases::exchanges(&graph, &rows, &[instruction.clone(), result.clone()], &[]);

		assert_eq!(cases.len(), 1);
		assert_eq!(cases[0].id, "managed:sage:11");
		assert_eq!(cases[0].state, "To review");
		assert!(!cases[0].earlier);

		for (assessment, state, earlier) in [
			(AgentExchangeAssessment::FollowUp, "Changes asked", false),
			(AgentExchangeAssessment::Wait, "Waiting", false),
			(AgentExchangeAssessment::UserDecision, "Decision", false),
			(AgentExchangeAssessment::Accepted, "Accepted", true),
		] {
			result.assessment = Some(assessment);

			let cases =
				cases::exchanges(&graph, &rows, &[instruction.clone(), result.clone()], &[]);

			assert_eq!(cases[0].state, state);
			assert_eq!(cases[0].earlier, earlier);
		}

		result.assessment = None;
		result.instruction_event_id = None;

		assert_eq!(
			cases::exchanges(&graph, &rows, &[instruction, result], &[]).len(),
			2,
			"missing linkage must not be guessed from participants or text"
		);
	});
}

#[gpui::test]
fn question_retirement_keeps_later_questions_and_rejects_rebound_sources(cx: &mut TestAppContext) {
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
			question: AgentAsyncQuestionDto {
				arrived_live: false,
				id: decodex_protocol::agent_async_question_id(item, 0),
				title: "Which name?".into(),
				options: vec![],
			},
			pending,
			answer: None,
		};
		let mut old = question("old", false);
		let later = question("later", true);
		let cases = cases::exchanges(&graph, &rows, &[], &[old.clone(), later]);

		assert_eq!(cases.iter().filter(|case| case.state == "Question").count(), 1);
		assert_eq!(cases.iter().filter(|case| case.earlier).count(), 1);
		assert_ne!(cases[0].id, cases[1].id);

		old.thread_id = "another-native-thread".into();

		assert!(cases::exchanges(&graph, &rows, &[], &[old]).is_empty());
	});
}

#[gpui::test]
fn source_receipts_cross_the_local_wire_without_switching_the_active_chat(cx: &mut TestAppContext) {
	let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("query frame")
		};
		let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
			panic!("read-only query")
		};

		assert!(matches!(query.payload, QueryPayload::GetAgentExchanges { before: None }));

		let mut entry = receipt(70, decodex_protocol::AgentExchangeKind::NotSent);

		entry.work_id = "agent".into();
		entry.thread_id = Some("ref-agent".into());
		entry.other_work_id = Some("sage".into());

		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
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
fn native_final_reports_keep_their_own_turn_and_do_not_invent_acceptance(cx: &mut TestAppContext) {
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
		brief.output = Some(Output {
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

#[gpui::test]
fn answered_question_retains_reply_sources_without_claiming_resolution(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);

		let mut graph = s.relation_graph();

		graph.edges.clear();

		let q = AgentExchangeQuestionDto {
			work_id: "sage".into(),
			thread_id: "ref-sage".into(),
			turn_id: "question-turn".into(),
			item_id: "question-item".into(),
			question: AgentAsyncQuestionDto {
				id: "question-id".into(),
				title: "Which name?".into(),
				options: vec![],
				arrived_live: false,
			},
			pending: false,
			answer: Some(AgentQuestionAnswerDto {
				event_id: 71,
				source_event_id: "exact-reply".into(),
				text: "Use Maple".into(),
				truncated: false,
				delivered_turn_id: Some("reply-turn".into()),
				turn_outcome: Some("completed".into()),
			}),
		};
		let cases = cases::exchanges(&graph, &s.factory_rows(), &[], &[q]);
		let case = cases.iter().find(|c| c.id.starts_with("question:")).unwrap();

		assert_eq!(case.state, "Answered");
		assert!(case.steps.iter().any(|(label, text)| *label == "Answer" && text == "Use Maple"));
		assert!(
			case.steps
				.iter()
				.any(|(label, text)| *label == "Reply source" && text.contains("exact-reply"))
		);
		assert_eq!(case.source_turn.as_deref(), Some("question-turn"));
		assert!(case.summary.contains("resolution is not inferred"));
	});
}

#[gpui::test]
fn independent_repair_cycles_never_borrow_the_current_assessment(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.visual_workbench_page("workbench", cx);

		let mut graph = s.relation_graph();

		graph.edges.clear();

		let mut old = receipt(80, decodex_protocol::AgentExchangeKind::PeerInstruction);

		old.collaboration = Some(AgentPeerExchangeDto {
			work_id: "venue".into(),
			request_id: "transport".into(),
			instruction_id: "first".into(),
			current: false,
			state: None,
			outcome: None,
			note: None,
		});

		let mut latest = receipt(81, decodex_protocol::AgentExchangeKind::PeerResult);

		latest.collaboration = Some(AgentPeerExchangeDto {
			work_id: "venue".into(),
			request_id: "transport".into(),
			instruction_id: "repair".into(),
			current: true,
			state: Some("closed".into()),
			outcome: Some("used".into()),
			note: Some("Checked returned source".into()),
		});

		let cases = cases::exchanges(&graph, &s.factory_rows(), &[old, latest], &[]);
		let old = cases.iter().find(|c| c.id == "peer:transport:first").unwrap();
		let latest = cases.iter().find(|c| c.id == "peer:transport:repair").unwrap();

		assert_eq!(old.state, "Earlier");
		assert_eq!(latest.state, "Used");
		assert!(!old.steps.iter().any(|(label, _)| *label == "Assessment"));
		assert!(latest.steps.iter().any(|(label, _)| *label == "Assessment"));
	});
}
