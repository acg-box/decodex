//! Clickable Work associations and the single global review control.

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{AppContext as _, Entity, Render, TestAppContext, Window};
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::{
	portfolio::{
		AgentActionDto, AgentSurface, Context, EntityId, IntoElement, ResultSource, Work, WorkState,
	},
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	AgentDispatchStateDto, AgentRequestResult, AgentRequestText, AgentSnapshotDto,
	AgentSnapshotResult, AgentWorkItemDto, AgentWorkKindDto, AgentWorkStatusDto, CURRENT_VERSION,
	ClientMessage, CommandPayload, ProjectDto, QueryPayload, QueryResultEnvelope,
	QueryResultPayload, ServerId, ServerMessage, WorkDetailResult,
	work::{
		Agent, CollaborationInstruction, CollaborationRequest, CollaborationState, Material,
		WorkPortfolio, WorkRequirements, WorkResult,
	},
};

struct PortfolioView {
	surface: Entity<AgentSurface>,
	guardian: bool,
}
impl Render for PortfolioView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| {
			if self.guardian { s.render_global_guardian(cx) } else { s.render_work_portfolio(cx) }
		})
	}
}

fn agent(id: &str) -> AgentWorkItemDto {
	AgentWorkItemDto {
		id: id.into(),
		parent_goal_id: None,
		kind: AgentWorkKindDto::Goal,
		title: id.into(),
		codex_thread_id: Some(format!("thread-{id}")),
		active_turn_id: None,
		status: AgentWorkStatusDto::Open,
		dispatch_state: AgentDispatchStateDto::Idle,
		next_check_at_micros: None,
		created_at_micros: 1,
		updated_at_micros: 1,
	}
}

fn snapshot() -> AgentSnapshotDto {
	let works = [
		("venue", "market", "sage"),
		("insurance", "market", "sage"),
		("route", "trip", "sage"),
		("hotel", "trip", "iris"),
	]
	.into_iter()
	.map(|(id, project, owner)| Work {
		id: id.into(),
		project_id: Some(project.into()),
		owner_agent_id: owner.into(),
		title: id.into(),
		requirements: WorkRequirements {
			id: "r1".into(),
			goal: format!("Research {id}"),
			criteria: "Return a sourced plan".into(),
			source: "User decision".into(),
		},
		state: WorkState::Open,
		reason: None,
		resume_condition: None,
		user_hold: false,
		assessment: None,
	})
	.collect();
	let portfolio = WorkPortfolio {
		works,
		materials: vec![Material {
			id: "evidence".into(),
			work_id: "venue".into(),
			title: "Venue details".into(),
			reference: "https://example.com/venue".into(),
			sha256: None,
		}],
		results: vec![WorkResult {
			id: "venue-result".into(),
			work_id: "venue".into(),
			requirements_id: "r1".into(),
			instruction_id: Some("route-instruction".into()),
			native_contribution_id: None,
			agent_id: "iris".into(),
			summary: "Accessible transport plan".into(),
			source: ResultSource::Artifact {
				path: "/tmp/public-plan.txt".into(),
				sha256: "a".repeat(64),
			},
			limits: "No booking".into(),
		}],
		collaborations: vec![CollaborationRequest {
			id: "transport".into(),
			work_id: "venue".into(),
			recipient_agent_id: "iris".into(),
			instruction: CollaborationInstruction {
				id: "route-instruction".into(),
				requirements_id: "r1".into(),
				text: "Compare public routes".into(),
				material_ids: vec!["evidence".into()],
				event_id: Some(1),
				delivered_turn_id: Some("receiving-turn".into()),
			},
			state: CollaborationState::Returned,
			required: true,
			outcome: None,
			note: None,
		}],
		..Default::default()
	};

	AgentSnapshotDto {
		agents: Box::new(vec![
			Agent {
				id: "sage".into(),
				name: "Sage".into(),
				native_thread_id: Some("thread-sage".into()),
				assistant: false,
			},
			Agent {
				id: "iris".into(),
				name: "Iris".into(),
				native_thread_id: Some("thread-iris".into()),
				assistant: false,
			},
		]),
		guardian: Default::default(),
		portfolio: Box::new(portfolio),
		archived_work_ids: Default::default(),
		agent_slots: Default::default(),
		context_references: vec![],
		connection_initializing: false,
		runtime_source: Some(EntityId::new("source").unwrap()),
		projects: [("market", "Community Market"), ("trip", "Family Trip")]
			.into_iter()
			.map(|(id, name)| ProjectDto {
				id: id.into(),
				name: name.into(),
				directory: format!("/tmp/{id}"),
				agent_ids: vec![if id == "market" { "sage".into() } else { "iris".into() }],
				native_project_id: Some(format!("native-{id}")),
				lead_agent_id: Some(if id == "market" { "sage".into() } else { "iris".into() }),
			})
			.collect(),
		work_items: vec![agent("sage"), agent("iris")],
		dependencies: vec![],
		pending_events: vec![],
	}
}

#[gpui::test]
fn independent_agent_message_never_falls_back_to_another_agent(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

	surface.update(visual, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		s.open_page("iris", cx);

		assert_eq!(s.root_id().as_deref(), Some("sage"));
		assert_eq!(s.composer_manager.as_deref(), Some("iris"));
		assert!(matches!(
			s.build_submission("Keep this message in Family Trip", cx).unwrap(),
			AgentActionDto::Send { root_id, .. } if root_id.as_str() == "iris"
		));
	});
}

#[gpui::test]
fn independent_agent_keeps_its_composer_and_can_answer_its_approval(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

	surface.update(visual, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		s.open_page("iris", cx);

		s.workspace.graph_visible = false;

		assert_eq!(s.root_id().as_deref(), Some("sage"));
		assert!(s.graph_has_composer(), "each independent Agent owns a conversation");

		s.snapshot.as_mut().unwrap().pending_events =
			vec![decodex_protocol::AgentPendingEventDto {
				id: 904,
				source_event_id: "iris-approval".into(),
				work_item_id: "iris".into(),
				event_kind: "permission_pending".into(),
				created_at_micros: 1,
				delivery_claimed: false,
			}];

		let request = AgentRequestResult::Available {
			event_id: 904,
			work_id: "iris".into(),
			method: "item/commandExecution/requestApproval".into(),
			request_json: AgentRequestText::new(
				serde_json::json!({
					"command":"printf approved > approval.txt",
					"cwd":"/tmp/trip",
					"availableDecisions":["accept","cancel"]
				})
				.to_string(),
			)
			.unwrap(),
		};

		s.prepare_question_inputs(&request, cx);

		s.request = Some(request);

		cx.notify();
	});

	visual.update(|window, cx| {
		window.resize(gpui::size(gpui::px(1_200.), gpui::px(1_000.)));
		window.draw(cx).clear(cx);
	});

	assert!(visual.debug_bounds("composer-editor-area").is_some());
	assert!(visual.debug_bounds("discuss-with-agent").is_none());

	let allow = visual.debug_bounds("request-accept").expect("the Agent's approval control");

	visual.simulate_click(allow.center(), Default::default());
	visual.run_until_parked();
	surface.read_with(visual, |s, _| {
		assert_eq!(s.selected.as_deref(), Some("iris"));
		assert_eq!(s.feedback.message, "No service profile is configured.");
	});
}

#[gpui::test]
fn two_projects_four_works_open_scoped_collaboration_material_and_result(cx: &mut TestAppContext) {
	let (_directory, profile, server) = wire_test_support::fixture(work_detail);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		cx.observe(&surface, |_, _, cx| cx.notify()).detach();

		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

			s.profile = Some(profile);
		});

		PortfolioView { surface, guardian: false }
	});

	visual.update(|w, cx| {
		w.resize(gpui::size(gpui::px(1_200.), gpui::px(1_500.)));
		w.draw(cx).clear(cx);
	});

	for id in ["work-venue", "work-insurance", "work-route", "work-hotel"] {
		assert!(visual.debug_bounds(id).is_some());
	}

	let bounds = visual.debug_bounds("work-venue").unwrap();

	visual.simulate_click(bounds.center(), Default::default());
	visual.run_until_parked();
	server.join().unwrap();
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_current_frame);

	assert!(visual.debug_bounds("material-venue-evidence").is_some());

	let bounds = visual.debug_bounds("collaboration-transport").unwrap();

	visual.simulate_click(bounds.center(), Default::default());
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_current_frame);

	let surface = view.read_with(visual, |v, _| v.surface.clone());

	surface.read_with(visual, |s, _| {
		assert_eq!(s.workspace.contribution_detail.as_deref(), Some("transport"));

		let snapshot = s.snapshot.as_ref().unwrap();

		assert_eq!(
			snapshot.portfolio.works.iter().filter(|w| w.owner_agent_id == "sage").count(),
			3
		);
		assert_eq!(snapshot.agents.len(), 2, "Works must not create extra Agents");
	});

	let bounds = visual.debug_bounds("open-result-venue-result").unwrap();

	visual.simulate_click(bounds.center(), Default::default());
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_current_frame);

	assert!(visual.debug_bounds("result-file-venue-result").is_some());
}

#[gpui::test]
fn review_details_click_does_not_change_execution_settings(cx: &mut TestAppContext) {
	let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let message = socket.next().await.unwrap().unwrap();
		let Message::Text(text) = message else { panic!("text command") };
		let ClientMessage::Command(command) = serde_json::from_str(&text).unwrap() else {
			panic!("global command")
		};
		let CommandPayload::Agent { action } = command.payload else { panic!("Agent command") };

		socket.close(None).await.unwrap();

		*action
	});
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		cx.observe(&surface, |_, _, cx| cx.notify()).detach();

		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

			s.profile = Some(profile);
		});

		PortfolioView { surface, guardian: true }
	});

	visual.update(|w, cx| {
		w.resize(gpui::size(gpui::px(900.), gpui::px(900.)));
		w.draw(cx).clear(cx);
	});

	let button = visual.debug_bounds("guardian-review-details").unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();

	assert!(
		matches!(server.join().unwrap(),AgentActionDto::SetReviewDetails{settings_id,expected_revision:1,review_details:true} if settings_id.as_str()=="guardian-global")
	);

	let surface = view.read_with(visual, |v, _| v.surface.clone());

	surface.read_with(visual, |s, _| {
		assert!(
			!s.snapshot.as_ref().unwrap().guardian.settings.review_details,
			"A lost reply is not an application receipt"
		);
	});
}

async fn work_detail(listener: UnixListener) {
	let mut socket = wire_test_support::accept(&listener).await;
	let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("query") };
	let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
		panic!("Work detail query")
	};

	assert!(
		matches!(&query.payload, QueryPayload::GetWorkDetail{work_id,digest:None,offset:0} if work_id.as_str()=="venue")
	);

	let mut portfolio = *snapshot().portfolio;

	portfolio.works.retain(|w| w.id == "venue");

	let text = serde_json::to_string(&portfolio).unwrap();
	let result = WorkDetailResult::Page {
		digest: "a".repeat(64),
		offset: 0,
		total_bytes: text.len(),
		text,
		next_offset: None,
	};

	socket
		.send(Message::Text(
			serde_json::to_string(&ServerMessage::QueryResult(QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).unwrap(),
				query_id: query.query_id,
				payload: QueryResultPayload::WorkDetail(result),
			}))
			.unwrap()
			.into(),
		))
		.await
		.unwrap();
}
