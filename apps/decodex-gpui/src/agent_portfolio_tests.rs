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

/// Exercise the actual sidebar and browser without starting unrelated runtime reads
/// on the wire fixture that captures the registration command.
struct RegistrationView(Entity<AgentSurface>);
impl Render for RegistrationView {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| {
			if s.workspace.registration.is_some() {
				s.render_catalog(crate::shell::agent_surface::Catalog::Codex, window, cx)
			} else {
				s.workspace_sidebar(window, cx)
			}
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
		assistant: Default::default(),
		agents: Box::new(vec![
			Agent {
				id: "sage".into(),
				name: "Sage".into(),
				native_thread_id: Some("thread-sage".into()),
				assistant: false,
				managed_conversation_ids: Default::default(),
				scheduled_wakes: Default::default(),
			},
			Agent {
				id: "iris".into(),
				name: "Iris".into(),
				native_thread_id: Some("thread-iris".into()),
				assistant: false,
				managed_conversation_ids: Default::default(),
				scheduled_wakes: Default::default(),
			},
		]),
		guardian: Default::default(),
		portfolio: Box::new(portfolio),
		archived_work_ids: Default::default(),
		side_chat_sources: Default::default(),
		agent_slots: Default::default(),
		conversation_titles: Default::default(),
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
fn thread_registration_routes_to_its_own_work_and_preserves_other_drafts(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	surface.update(visual, |s, cx| {
		let mut initial = snapshot();
		initial.work_items.push(agent("ordinary"));
		s.apply_result(Ok(AgentSnapshotResult::Available(initial)));
		assert!(
			s.project_conversation_pages().iter().any(|(id, _, _)| id == "ordinary"),
			"a registered conversation stays in the normal directory before its first visit"
		);
		s.open_page("sage", cx);
		s.composer.update(cx, |input, cx| input.set_content("Draft for Sage", cx));
		s.open_page("ordinary", cx);
		assert!(s.graph_has_composer());
		assert_eq!(s.composer.read(cx).content(), "");
		assert!(matches!(
			s.build_submission("Continue the existing conversation", cx).unwrap(),
			AgentActionDto::Send { root_id, .. } if root_id.as_str() == "ordinary"
		));
		s.open_page("sage", cx);
		assert_eq!(s.composer.read(cx).content(), "Draft for Sage");
	});
}

#[gpui::test]
fn thread_registration_reassignment_click_sends_the_displayed_owner(cx: &mut TestAppContext) {
	let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("command") };
		let ClientMessage::Command(command) = serde_json::from_str(&text).unwrap() else {
			panic!("editing a local conversation must not fetch the external catalog")
		};
		let CommandPayload::Agent { action } = command.payload else { panic!("Agent command") };
		socket.close(None).await.unwrap();
		*action
	});
	let surface = cx.new(AgentSurface::new);
	let (_, visual) = cx.add_window_view(|_, cx| {
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		RegistrationView(surface.clone())
	});
	surface.update(visual, |s, cx| {
		let mut initial = snapshot();
		initial.agents[1].name =
			"The old launch prompt was a long first message rather than a short title.".repeat(10);
		initial
			.conversation_titles
			.insert("thread-iris".into(), "Amber release gate review".into());
		initial.conversation_titles.insert(
			"thread-sage".into(),
			"An intentionally long fallback conversation title ".repeat(12),
		);
		initial.work_items.push(agent("ordinary"));
		initial.agents[0].managed_conversation_ids.push("ordinary".into());
		s.apply_result(Ok(AgentSnapshotResult::Available(initial)));
		s.composer.update(cx, |input, cx| input.set_content("Keep my draft", cx));
		let target = &s.snapshot.as_ref().unwrap().agents[1];
		assert_eq!(s.agent_conversation_label(target), "Amber release gate review");
		assert!(
			s.project_conversation_pages()
				.iter()
				.any(|(id, title, _)| id == &target.id
					&& title == &s.agent_conversation_label(target))
		);
		s.profile = Some(profile);
		s.open_thread_registration(Some("ordinary"), cx);
	});
	visual.update(|window, cx| {
		window.resize(gpui::size(gpui::px(960.), gpui::px(840.)));
		window.draw(cx).clear(cx);
	});
	assert!(visual.debug_bounds("registration-release").is_some());
	assert!(visual.debug_bounds("registration-promote").is_some());
	assert!(visual.debug_bounds("registration-register").is_none());
	assert!(visual.debug_bounds("registration-assign-iris").is_none());
	let choose = visual.debug_bounds("registration-assignment").unwrap();
	visual.simulate_click(choose.center(), Default::default());
	visual.update(crate::ui_frame::draw_frame);
	visual.executor().advance_clock(std::time::Duration::from_millis(250));
	visual.update(crate::ui_frame::draw_frame);
	let target = visual.debug_bounds("registration-assign-iris").unwrap();
	let long = visual.debug_bounds("registration-release").unwrap();
	assert_eq!(long.size.height, target.size.height, "long names remain on one line");
	visual.simulate_click(target.center(), Default::default());
	visual.run_until_parked();
	assert!(matches!(server.join().unwrap(),
		AgentActionDto::RegisterNativeThread { thread_id, relation: decodex_protocol::work::ThreadRegistration::Assign { agent_id, expected_owner } }
			if thread_id.as_str() == "thread-ordinary" && agent_id == "iris" && expected_owner.as_deref() == Some("sage")
	));
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Keep my draft");
		assert!(s.workspace.registration.is_some());
		assert!(!s.uncertain, "An unconfirmed ownership edit must not lock the message composer");
	});
}

fn native_thread_candidate(title: String) -> decodex_protocol::NativeThreadCandidate {
	decodex_protocol::NativeThreadCandidate {
		updated_at_micros: 0,
		thread_id: title.clone(),
		title,
		preview: Some("Original discussion".into()),
	}
}

#[gpui::test]
fn thread_registration_lists_native_pages_and_promotes_the_selected_thread(
	cx: &mut TestAppContext,
) {
	use decodex_protocol::{NativeThreadPage, work::ThreadRegistration};
	let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
		for (before, thread, next) in
			[(None, "native-a", Some("page-b")), (Some("page-b"), "native-b", None)]
		{
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("page query")
			};
			let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
				panic!("query")
			};
			assert!(
				matches!(query.payload, QueryPayload::ListNativeThreads { cursor, search: None } if cursor.as_ref().map(|s|s.as_str()) == before)
			);
			let threads = if before.is_none() {
				(0..24).map(|index| format!("thread-{index}")).chain([thread.to_owned()]).collect()
			} else {
				vec![thread.to_owned()]
			};
			wire_test_support::answer_query(
				&mut socket,
				query.query_id,
				QueryResultPayload::NativeThreads(Some(NativeThreadPage {
					threads: threads.into_iter().map(native_thread_candidate).collect(),
					next_cursor: next.map(str::to_owned),
				})),
			)
			.await;
		}
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("command") };
		let ClientMessage::Command(command) = serde_json::from_str(&text).unwrap() else {
			panic!("command")
		};
		let CommandPayload::Agent { action } = command.payload else { panic!("Agent command") };
		*action
	});
	let surface = cx.new(AgentSurface::new);
	let (_, visual) = cx.add_window_view(|_, cx| {
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		RegistrationView(surface.clone())
	});
	surface.update(visual, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		s.profile = Some(profile);
		s.workspace.browsing = true;
		cx.notify();
	});
	visual.simulate_resize(gpui::size(gpui::px(960.), gpui::px(450.)));
	visual.update(crate::ui_frame::draw_frame);
	for selector in ["codex-conversations", "choose-thread-native-b", "registration-promote"] {
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);
		if selector == "choose-thread-native-b" {
			let viewport = visual.debug_bounds("registration-candidates").unwrap();
			assert!(
				visual.debug_bounds(selector).is_none(),
				"second page requested before scrolling in {viewport:?}, last first-page row {:?}",
				visual.debug_bounds("choose-thread-native-a")
			);
			let scroll_to_end = gpui::ScrollEvent {
				position: viewport.center(),
				delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-2000.))),
				..Default::default()
			};
			visual.simulate_event(scroll_to_end.clone());
			visual.update(crate::ui_frame::draw_frame);
			visual.run_until_parked();
			visual.update(crate::ui_frame::draw_frame);
			visual.simulate_event(scroll_to_end);
			visual.update(crate::ui_frame::draw_frame);
			assert!(viewport.contains(&visual.debug_bounds(selector).expect("next page").center()));
		}
		if selector == "registration-promote" {
			std::thread::sleep(std::time::Duration::from_millis(220));
			for width in [960., 360.] {
				visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(840.)));
				visual.update(crate::ui_frame::draw_frame);
				let title = visual.debug_bounds("registration-title").unwrap();
				let preview = visual.debug_bounds("registration-preview").unwrap();
				let add = visual.debug_bounds("registration-register").unwrap();
				let promote = visual.debug_bounds("registration-promote").unwrap();
				let assign = visual.debug_bounds("registration-assignment").unwrap();
				assert!(visual.debug_bounds("registration-assign-iris").is_none());
				assert!(title.bottom() <= preview.top());
				for control in [add, promote, assign] {
					assert!(preview.bottom() <= control.top());
					assert!(
						control.right() <= gpui::px(width) && control.left() >= gpui::px(0.),
						"action fits at {width}: {control:?}"
					);
				}
				assert_eq!(visual.debug_bounds("registration-back").is_some(), width < 850.);
				assert!(visual.debug_bounds("registration-release").is_none());
			}
			let copy = visual.debug_bounds("registration-copy-id").unwrap();
			visual.simulate_click(copy.center(), Default::default());
			visual.update(|_, cx| {
				assert_eq!(
					cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
					Some("native-b")
				)
			});
		}
		let target = visual.debug_bounds(selector).expect(selector);
		visual.simulate_click(target.center(), Default::default());
	}
	visual.run_until_parked();
	assert!(matches!(server.join().unwrap(),
		AgentActionDto::RegisterNativeThread { thread_id, relation:ThreadRegistration::Promote { expected_owner: None }}
		if thread_id.as_str() == "native-b"
	));
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
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, _| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		s.profile = Some(profile);
	});
	let (_settings, visual) = cx.add_window_view(|_, cx| {
		let mut settings = crate::settings_surface::SettingsSurface::new(
			crate::desktop_settings::DesktopSettingsController::production(),
			cx,
		);
		settings.category = crate::settings_surface::SettingsCategory::Agents;
		settings.bind_agent(&surface, cx);
		settings
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

	surface.read_with(visual, |s, _| {
		assert!(
			!s.snapshot.as_ref().unwrap().guardian.settings.review_details,
			"A lost reply is not an application receipt"
		);
	});
}

#[gpui::test]
fn guardian_history_has_no_settings_control(cx: &mut TestAppContext) {
	let (_, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		});
		PortfolioView { surface, guardian: true }
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("global-guardian-history").is_some());
	assert!(visual.debug_bounds("guardian-history-empty").is_some());
	assert!(visual.debug_bounds("guardian-review-details").is_none());
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
