//! Rendered current-turn reviewer changes cross the public same-UID socket.
use std::thread::JoinHandle;

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{AppContext as _, TestAppContext};
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::{
	live_settings::{
		self, AgentActionDto, AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult,
		AgentSurface, AgentWorkItemDto, AgentWorkStatusDto, ClientProfile, Context, Edit, Entity,
		EntityId, IntoElement, Render, Reviewer, State, Window, WireText,
	},
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	AgentLiveModelSelection, AgentLiveReviewerOutcome, AgentWorkKindDto, CURRENT_VERSION,
	ClientMessage, CommandPayload, ConversationModel, ConversationReasoningEffort, QueryPayload,
	QueryResultEnvelope, QueryResultPayload, ServerId, ServerMessage,
};

struct ReviewerView {
	surface: Entity<AgentSurface>,
}
impl Render for ReviewerView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.live_reviewer_panel(&work(), cx))
	}
}

fn fixture(model: bool) -> (TempDir, ClientProfile, JoinHandle<Vec<AgentActionDto>>) {
	wire_test_support::fixture(move |listener| serve(listener, model))
}

fn live_state(index: usize, model: bool) -> State {
	State::Available {
		thread_id: EntityId::new("thread").unwrap(),
		turn_id: EntityId::new("turn").unwrap(),
		review_token: WireText::new(if index == 0 { "a" } else { "b" }.repeat(64)).unwrap(),
		can_update: true,
		last_reviewer: (index != 0 && !model).then_some(Reviewer::User),
		last_model: (model && index != 0).then(|| AgentLiveModelSelection {
			model: ConversationModel::new("selected").unwrap(),
			effort: ConversationReasoningEffort::High,
		}),
		model_choices: model.then(|| vec![model_choice()]),
		last_outcome: (index != 0).then_some(AgentLiveReviewerOutcome::Unknown),
	}
}

fn model_choice() -> decodex_protocol::AgentModelDto {
	decodex_protocol::AgentModelDto {
		model: ConversationModel::new("selected").unwrap(),
		name: "Selected".into(),
		efforts: vec![
			decodex_protocol::ConversationReasoningEffort::Low,
			decodex_protocol::ConversationReasoningEffort::High,
		],
		default_effort: Some(ConversationReasoningEffort::Low),
		supports_fast: false,
		service_tiers: vec![],
		default_service_tier: None,
		available_cyber_programs: None,
		specialty: None,
		supports_images: true,
		availability: None,
		upgrade: None,
	}
}

fn work() -> AgentWorkItemDto {
	AgentWorkItemDto {
		id: "root".into(),
		parent_goal_id: None,
		kind: AgentWorkKindDto::Goal,
		title: "Root".into(),
		codex_thread_id: Some("thread".into()),
		active_turn_id: Some("turn".into()),
		dispatch_state: AgentDispatchStateDto::Running,
		status: AgentWorkStatusDto::Open,
		next_check_at_micros: None,
		created_at_micros: 1,
		updated_at_micros: 1,
	}
}

#[gpui::test]
fn reviewed_live_turn_is_invalidated_even_if_the_old_identity_returns(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	for change in ["thread", "turn", "idle", "removed", "source"] {
		let original = AgentSnapshotDto {
			runtime_source: Some(EntityId::new("source").unwrap()),
			workspaces: vec![],
			work_items: vec![work()],
			dependencies: vec![],
			pending_events: vec![],
		};

		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(original.clone())));

			s.live_reviewer.work = Some("root".into());
			s.live_reviewer.state = Some(State::Available {
				thread_id: EntityId::new("thread").unwrap(),
				turn_id: EntityId::new("turn").unwrap(),
				review_token: WireText::new("a".repeat(64)).unwrap(),
				can_update: true,
				last_reviewer: None,
				last_model: None,
				model_choices: None,
				last_outcome: None,
			});

			let before = s.live_reviewer.epoch;
			let mut changed = original.clone();

			match change {
				"thread" => changed.work_items[0].codex_thread_id = Some("other-thread".into()),
				"turn" => changed.work_items[0].active_turn_id = Some("other-turn".into()),
				"idle" => changed.work_items[0].dispatch_state = AgentDispatchStateDto::Idle,
				"removed" => changed.work_items.clear(),
				_ => changed.runtime_source = Some(EntityId::new("other-source").unwrap()),
			}

			s.apply_result(Ok(AgentSnapshotResult::Available(changed)));

			assert_ne!(s.live_reviewer.epoch, before, "{change}");

			s.apply_result(Ok(AgentSnapshotResult::Available(original)));

			assert!(s.live_reviewer.state.is_none(), "old review returned after {change}");
			assert!(s.live_reviewer.work.is_none());
		});
	}
}

#[gpui::test]
fn reviewer_click_sends_exact_turn_once_and_reads_unknown_receipt(cx: &mut TestAppContext) {
	exercise_live_settings(cx, false);
}

#[gpui::test]
fn model_click_sends_exact_turn_once_and_reads_unknown_receipt(cx: &mut TestAppContext) {
	exercise_live_settings(cx, true);
}

fn exercise_live_settings(cx: &mut TestAppContext, model: bool) {
	let (_dir, profile, server) = fixture(model);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		cx.observe(&surface, |_, _, cx| cx.notify()).detach();

		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				runtime_source: Some(EntityId::new("source").unwrap()),
				workspaces: vec![],
				work_items: vec![work()],
				dependencies: vec![],
				pending_events: vec![],
			})));

			s.profile = Some(profile);
		});

		ReviewerView { surface }
	});

	visual.update(|w, cx| {
		w.resize(gpui::size(live_settings::px(900.), live_settings::px(600.)));
		w.draw(cx).clear();
	});

	let button = visual.debug_bounds("live-reviewer-read").unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});

	if model {
		for selector in ["live-model-open", "live-model-choice-0", "live-model-effort-1"] {
			let button = visual.debug_bounds(selector).unwrap();

			visual.simulate_click(button.center(), Default::default());
			visual.run_until_parked();
			visual.update(|w, cx| {
				w.draw(cx).clear();
			});
		}
	}

	let button =
		visual.debug_bounds(if model { "live-model-apply" } else { "live-reviewer-user" }).unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();

	assert_eq!(server.join().unwrap().len(), 1);

	let surface = view.read_with(visual, |v, _| v.surface.clone());

	surface.read_with(visual, |s, _| {
		assert!(s.live_reviewer.task.is_none());
		assert!(s.live_reviewer.feedback.contains("could not be confirmed"));

		if model {
			assert!(matches!(
				&s.live_reviewer.state,
				Some(State::Available { last_model: Some(selection), .. })
					if selection.model.as_str() == "selected" && selection.effort.as_str() == "high"
			));
		}

		assert!(matches!(
			s.live_reviewer.state,
			Some(State::Available {
				last_outcome: Some(decodex_protocol::AgentLiveReviewerOutcome::Unknown),
				..
			})
		));
	});
	surface.update(visual, |s, cx| {
		assert!(!s.live_reviewer.reviewed, "receipt refresh is not a new user review");

		s.update_live_settings(
			"root".into(),
			"turn".into(),
			Some(if model {
				Edit::Model(AgentLiveModelSelection {
					model: ConversationModel::new("selected").unwrap(),
					effort: ConversationReasoningEffort::High,
				})
			} else {
				Edit::Reviewer(Reviewer::User)
			}),
			cx,
		);

		assert!(s.live_reviewer.task.is_none(), "a repeated click cannot publish again");
	});
	visual.update(|window, cx| {
		window.draw(cx).clear();
	});

	assert!(visual.debug_bounds("live-reviewer-user").is_none());
	assert!(visual.debug_bounds("live-model-apply").is_none());
	assert!(visual.debug_bounds("live-reviewer-read").is_some());

	surface.update(visual, |s, _| s.apply_result(Err(())));
	surface.read_with(visual, |s, _| {
		assert!(s.live_reviewer.state.is_none());
		assert!(s.live_reviewer.work.is_none());
	});
}

#[gpui::test]
fn child_navigation_and_disconnect_cannot_edit_the_parent_reviewer(cx: &mut TestAppContext) {
	let (_root, profile, server) = wire_test_support::fixture(|_| async {});

	server.join().unwrap();

	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				runtime_source: Some(EntityId::new("source").unwrap()),
				workspaces: vec![],
				work_items: vec![work()],
				dependencies: vec![],
				pending_events: vec![],
			})))
		});

		ReviewerView { surface }
	});
	let surface = view.read_with(visual, |v, _| v.surface.clone());

	surface.update(visual, |s, cx| {
		s.live_reviewer.reviewed = true;
		s.live_reviewer.work = Some("root".into());
		s.profile = Some(profile);

		s.open_native_agent("root", "child", cx);

		assert_eq!(s.native_agents.selected, Some(("root".into(), "child".into())));
		assert!(!s.live_reviewer.reviewed);
		assert!(s.live_reviewer.work.is_none());
	});

	visual.update(|window, cx| {
		window.resize(gpui::size(live_settings::px(900.), live_settings::px(600.)));
		window.draw(cx).clear();
	});

	assert!(visual.debug_bounds("live-reviewer-read").is_none());

	surface.update(visual, |s, cx| {
		s.open_page("root", cx);
		s.apply_result(Ok(AgentSnapshotResult::Unavailable));
		cx.notify();
	});
	visual.update(|window, cx| {
		window.draw(cx).clear();
	});

	assert!(visual.debug_bounds("live-reviewer-read").is_none());
}

fn running_snapshot() -> AgentSnapshotDto {
	AgentSnapshotDto {
		runtime_source: Some(EntityId::new("source").unwrap()),
		workspaces: vec![],
		work_items: vec![work()],
		dependencies: vec![],
		pending_events: vec![],
	}
}

#[gpui::test]
fn ordinary_refresh_keeps_live_settings_read_and_publication_receipt(cx: &mut TestAppContext) {
	for model in [false, true] {
		let (_dir, profile, server) = fixture(model);
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(running_snapshot())));

			s.profile = Some(profile);
		});

		let edit = if model {
			Edit::Model(AgentLiveModelSelection {
				model: ConversationModel::new("selected").unwrap(),
				effort: ConversationReasoningEffort::High,
			})
		} else {
			Edit::Reviewer(Reviewer::User)
		};

		for selection in [None, Some(edit)] {
			let saving = selection.is_some();

			surface.update(cx, |s, cx| {
				s.update_live_settings("root".into(), "turn".into(), selection, cx);

				assert!(s.live_reviewer.task.is_some());

				// Advance the snapshot generation before this operation can complete.
				s.generation += 1;

				s.apply_result(Ok(AgentSnapshotResult::Available(running_snapshot())));
			});

			cx.run_until_parked();
			surface.read_with(cx, |s, _| {
				assert!(
					s.live_reviewer.task.is_none(),
					"refresh must not strand a completed operation"
				);
				assert!(matches!(s.live_reviewer.state, Some(State::Available { .. })));
				assert_eq!(s.live_reviewer.reviewed, !saving);

				if saving {
					assert!(s.live_reviewer.feedback.contains("could not be confirmed"));
					assert!(matches!(
						s.live_reviewer.state,
						Some(State::Available {
							last_outcome: Some(decodex_protocol::AgentLiveReviewerOutcome::Unknown),
							..
						})
					));
				}
			});
		}

		assert_eq!(server.join().unwrap().len(), 1, "an uncertain publication must not be retried");
	}
}

#[gpui::test]
fn disconnect_invalidates_live_review_before_same_turn_reconnect(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(running_snapshot())));

		s.live_reviewer.work = Some("root".into());
		s.live_reviewer.state = Some(live_state(0, true));
		s.live_reviewer.reviewed = true;

		let epoch = s.live_reviewer.epoch;

		s.mark_stale(cx);

		assert_ne!(s.live_reviewer.epoch, epoch);

		s.apply_result(Ok(AgentSnapshotResult::Available(running_snapshot())));

		assert!(s.live_reviewer.state.is_none());
		assert!(!s.live_reviewer.reviewed);
	});
}

async fn serve(listener: UnixListener, model: bool) -> Vec<AgentActionDto> {
	let mut actions = Vec::new();

	for index in 0..3 {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("text request")
		};
		let request: ClientMessage = serde_json::from_str(&text).unwrap();

		if index == 1 {
			let ClientMessage::Command(command) = request else { panic!("setting command") };
			let CommandPayload::Agent { action } = command.payload else { panic!("Agent command") };
			let (work_id, turn_id, review_token) = match &*action {
				AgentActionDto::SetLiveReviewer { work_id, turn_id, review_token, reviewer }
					if !model =>
				{
					assert_eq!(*reviewer, Reviewer::User);

					(work_id, turn_id, review_token)
				},
				AgentActionDto::SetLiveModel {
					work_id,
					turn_id,
					review_token,
					model: selected,
					effort,
				} if model => {
					assert_eq!(selected.as_str(), "selected");
					assert_eq!(effort.as_str(), "high");

					(work_id, turn_id, review_token)
				},
				_ => panic!("wrong live settings action"),
			};

			assert_eq!(work_id.as_str(), "root");
			assert_eq!(turn_id.as_str(), "turn");
			assert_eq!(review_token.as_str(), "a".repeat(64));

			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();

			continue;
		}

		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let QueryPayload::GetAgentLiveReviewer { work_id, include_models: true } = query.payload
		else {
			panic!("account query")
		};

		assert_eq!(work_id.as_str(), "root");

		let state = live_state(index, model);
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentLiveReviewer(state),
		});

		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}

	actions
}
