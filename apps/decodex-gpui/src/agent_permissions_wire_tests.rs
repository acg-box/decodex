//! Permission clicks cross the public socket; lost replies trigger reads, not retries.
use std::thread::JoinHandle;

use decodex_protocol::{AgentPermissionOutcome, AgentWorkItemDto};
use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{
	AppContext as _, TestAppContext,
	prelude::{ParentElement as _, Styled as _},
};
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::{
	permissions::{
		self, AgentActionDto, AgentDispatchStateDto, AgentPermissionState, AgentSnapshotDto,
		AgentSnapshotResult, AgentSurface, AgentWorkStatusDto, ClientProfile, Context, Entity,
		EntityId, IntoElement, Render, Window, WireText,
	},
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	AgentWorkKindDto, CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload,
	QueryResultEnvelope, QueryResultPayload, ServerId, ServerMessage,
};

struct PermissionView {
	surface: Entity<AgentSurface>,
}
impl Render for PermissionView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.w(gpui::px(super::super::composer::composer_menu_width(Some("permissions"))))
			.p(gpui::px(crate::ui_theme::POPOVER_CONTENT_PADDING))
			.child(self.surface.update(cx, |s, cx| s.permission_options(cx)))
	}
}

fn fixture() -> (TempDir, ClientProfile, JoinHandle<Vec<AgentActionDto>>) {
	wire_test_support::fixture(serve)
}

fn available() -> AgentPermissionState {
	AgentPermissionState::Available {
		work_id: EntityId::new("root").unwrap(),
		thread_id: EntityId::new("thread").unwrap(),
		review_token: WireText::new("a".repeat(64)).unwrap(),
		cwd: WireText::new("/native").unwrap(),
		profile_id: Some(WireText::new("readonly").unwrap()),
		mode: None,
		approvals_reviewer: WireText::new("user").unwrap(),
		profiles: vec![
			decodex_protocol::AgentPermissionProfile {
				id: WireText::new("scoped").unwrap(),
				allowed: true,
				can_select: true,
				description: None,
			},
			decodex_protocol::AgentPermissionProfile {
				id: WireText::new(":full-access").unwrap(),
				allowed: false,
				can_select: false,
				description: None,
			},
		],
		can_update: true,
		last_outcome: None,
	}
}

fn work() -> AgentWorkItemDto {
	AgentWorkItemDto {
		id: "root".into(),
		parent_goal_id: None,
		kind: AgentWorkKindDto::Goal,
		title: "Root".into(),
		codex_thread_id: Some("thread".into()),
		active_turn_id: None,
		dispatch_state: AgentDispatchStateDto::Idle,
		status: AgentWorkStatusDto::Open,
		next_check_at_micros: None,
		created_at_micros: 1,
		updated_at_micros: 1,
	}
}

fn snapshot() -> AgentSnapshotDto {
	AgentSnapshotDto {
		agents: Default::default(),
		guardian: Default::default(),
		portfolio: Default::default(),
		archived_work_ids: Default::default(),
		agent_slots: Default::default(),
		context_references: vec![],
		connection_initializing: false,
		runtime_source: Some(EntityId::new("source").unwrap()),
		projects: vec![],
		work_items: vec![work()],
		dependencies: vec![],
		pending_events: vec![],
	}
}

#[gpui::test]
fn permission_click_sends_once_and_retains_unknown_after_lost_reply(cx: &mut TestAppContext) {
	let (_dir, profile, server) = fixture();
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		cx.observe(&surface, |_, _, cx| cx.notify()).detach();

		surface.update(cx, |s, cx| {
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

			s.profile = Some(profile);
			s.refresh_composer_permissions(true, cx);
		});

		PermissionView { surface }
	});

	visual.update(|w, cx| {
		w.resize(gpui::size(permissions::px(900.), permissions::px(700.)));
		w.draw(cx).clear(cx);
	});

	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("permission-profiles-read").is_none());

	assert!(
		visual.debug_bounds("permission-profile-1").is_none(),
		"policy-disabled profile must have no action"
	);

	let button = visual.debug_bounds("permission-profile-0").unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();

	assert_eq!(server.join().unwrap().len(), 1);

	let surface = view.read_with(visual, |v, _| v.surface.clone());

	surface.read_with(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		assert!(s.permission_profiles.task.is_none());
		assert!(!s.permission_profiles.reviewed);
		assert!(s.permission_profiles.feedback.contains("unconfirmed"));
		assert!(matches!(
			s.permission_profiles.state,
			Some(AgentPermissionState::Pending { state: AgentPermissionOutcome::Unknown, .. })
		));
	});
	visual.update(crate::ui_frame::draw_current_frame);

	assert!(visual.debug_bounds("permission-profile-0").is_none());
	assert!(visual.debug_bounds("permission-profiles-read").is_some());

	surface.update(visual, |s, _| {
		s.apply_result(Err(()));
		s.apply_result(Err(()));
	});
	surface.read_with(visual, |s, _| {
		assert!(s.permission_profiles.state.is_none());
		assert!(s.permission_profiles.work.is_none());
	});
}

#[gpui::test]
fn permission_read_keeps_the_current_choice_and_menu_geometry(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
			s.permission_profiles.work = Some("root".into());
			let mut state = available();
			if let AgentPermissionState::Available { mode, .. } = &mut state {
				*mode = Some(decodex_protocol::PermissionMode::ApproveForMe);
			}
			s.permission_profiles.state = Some(state);
			s.permission_profiles.reviewed = true;
		});
		PermissionView { surface }
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let before = visual.debug_bounds("permission-choice-1").unwrap();
	let row = visual.debug_bounds("permission-choice-0").unwrap();
	let description = visual.debug_bounds("permission-description-0").unwrap();
	assert_eq!(row.size.height, gpui::px(38.));
	assert!(row.left() <= description.left() && description.right() <= row.right());
	assert!(row.top() < description.top() && description.bottom() < row.bottom());
	assert_eq!(description.size.height, gpui::px(14.), "the short description fits one line");
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	surface.update(visual, |s, cx| {
		s.begin_permission_read("root", false);
		assert_eq!(s.permission_label(), "Approve for me");
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert_eq!(visual.debug_bounds("permission-choice-1"), Some(before));
	assert!(visual.debug_bounds("permission-profiles-read").is_none());
}

#[gpui::test]
fn dormant_conversation_keeps_choices_without_a_connect_action(cx: &mut TestAppContext) {
	let (_view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
			s.permission_profiles.work = Some("root".into());
			s.permission_profiles.state = Some(AgentPermissionState::NotConnected);
			assert_eq!(s.permission_label(), "Permissions");
		});
		PermissionView { surface }
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("permission-profiles-read").is_none());
	assert!(visual.debug_bounds("permission-connect").is_none());
	for id in
		["permission-choice-0", "permission-choice-1", "permission-choice-2", "permission-choice-3"]
	{
		assert!(visual.debug_bounds(id).is_some());
	}
}

#[gpui::test]
fn dormant_permission_choice_is_local_and_carried_by_the_next_message(cx: &mut TestAppContext) {
	use decodex_protocol::PermissionMode;
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, cx| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
			s.permission_profiles.work = Some("root".into());
			s.permission_profiles.state = Some(AgentPermissionState::NotConnected);
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
		});
		PermissionView { surface }
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let choice = visual.debug_bounds("permission-choice-1").unwrap().center();
	visual.simulate_click(choice, Default::default());
	visual.run_until_parked();
	let surface = view.read_with(visual, |view, _| view.surface.clone());
	surface.read_with(visual, |s, cx| {
		assert!(matches!(s.permission_profiles.state, Some(AgentPermissionState::NotConnected)));
		assert!(
			s.permission_profiles.task.is_none(),
			"choosing a preset must not connect or send a request"
		);
		assert_eq!(s.permission_label(), "Approve for me");
		assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		let action = s.configured_send(
			EntityId::new("root").unwrap(),
			decodex_protocol::HistoryText::new("Continue").unwrap(),
			vec![],
		);
		let AgentActionDto::SendConfigured { execution, .. } = action else {
			panic!("configured send")
		};
		assert_eq!(execution.permission_mode, Some(PermissionMode::ApproveForMe));
		assert!(
			execution.model.is_none()
				&& execution.reasoning_effort.is_none()
				&& execution.selected_service_tier().is_none()
		);
	});
	assert!(
		surface.read_with(visual, |s, _| s
			.draft_profiles
			.execution
			.choice("other-agent")
			.is_empty())
	);
}

#[gpui::test]
fn permission_shields_distinguish_modes_without_using_other_agents_state(cx: &mut TestAppContext) {
	use crate::shell::workspace_symbols::Symbol;
	use decodex_protocol::PermissionMode;
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		s.new_work_conversation(cx);
		for mode in PermissionMode::ALL {
			s.choose_permission_mode(mode, cx);
			assert!(matches!(
				(mode, s.permission_symbol()),
				(PermissionMode::AskForApproval, Symbol::Shield)
					| (PermissionMode::ApproveForMe, Symbol::ShieldCheck)
					| (PermissionMode::FullAccess, Symbol::ShieldSlash)
					| (PermissionMode::ReadOnly, Symbol::ShieldLock)
			));
		}
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		s.open_page("root", cx);
		s.permission_profiles.work = Some("other-agent".into());
		s.permission_profiles.state = Some(available());
		assert!(matches!(s.permission_symbol(), Symbol::Shield));
	});
}

#[gpui::test]
fn independent_agent_permissions_follow_the_composer_and_new_draft(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		let mut snapshot = snapshot();
		let mut lead = work();
		lead.id = "lead".into();
		lead.codex_thread_id = Some("lead-thread".into());
		snapshot.work_items.push(lead);
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot)));
		s.composer_manager = Some("lead".into());
		s.selected = Some("lead".into());
		assert_eq!(s.permission_owner().as_deref(), Some("lead"));
		s.permission_profiles.work = Some("root".into());
		s.permission_profiles.state = Some(available());
		assert_eq!(s.permission_label(), "Permissions", "another Agent's facts are not current");
		s.permission_mode = decodex_protocol::PermissionMode::FullAccess;
		s.new_work_conversation(cx);
		assert!(s.permission_owner().is_none());
		assert_eq!(s.creation_permission_mode(), decodex_protocol::PermissionMode::AskForApproval);
		s.choose_permission_mode(decodex_protocol::PermissionMode::ApproveForMe, cx);
		assert_eq!(s.permission_label(), "Approve for me");
		s.open_page("lead", cx);
		assert_eq!(s.permission_owner().as_deref(), Some("lead"));
		s.new_work_conversation(cx);
		assert_eq!(s.permission_label(), "Approve for me", "reopening retains the draft choice");
	});
}

#[gpui::test]
fn permission_review_is_invalidated_on_task_or_source_transition(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	for change in ["thread", "turn", "running", "removed", "source"] {
		surface.update(cx, |s, _| {
			let original = snapshot();

			s.apply_result(Ok(AgentSnapshotResult::Available(original.clone())));

			s.permission_profiles.work = Some("root".into());
			s.permission_profiles.state = Some(available());

			let epoch = s.permission_profiles.epoch;
			let mut next = original.clone();

			match change {
				"thread" => next.work_items[0].codex_thread_id = Some("other".into()),
				"turn" => next.work_items[0].active_turn_id = Some("other".into()),
				"running" => next.work_items[0].dispatch_state = AgentDispatchStateDto::Running,
				"removed" => next.work_items.clear(),
				_ => next.runtime_source = Some(EntityId::new("other-source").unwrap()),
			}

			s.apply_result(Ok(AgentSnapshotResult::Available(next)));

			assert_ne!(s.permission_profiles.epoch, epoch, "{change}");

			s.apply_result(Ok(AgentSnapshotResult::Available(original)));

			assert!(s.permission_profiles.state.is_none());
		});
	}
}

#[gpui::test]
fn running_permissions_offer_both_named_and_builtin_profiles(cx: &mut TestAppContext) {
	let (_view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, _| {
			let mut snapshot = snapshot();

			snapshot.work_items[0].dispatch_state = AgentDispatchStateDto::Running;
			snapshot.work_items[0].active_turn_id = Some("active".into());

			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot)));

			s.permission_profiles.work = Some("root".into());

			let mut state = available();

			if let AgentPermissionState::Available { profiles, .. } = &mut state {
				profiles[0].can_select = true;
				profiles[1].id = WireText::new(":workspace").expect("builtin");
				profiles[1].allowed = true;
				profiles[1].can_select = true;
			}

			s.permission_profiles.state = Some(state);
			s.permission_profiles.reviewed = true;
		});

		PermissionView { surface }
	});

	visual.update(|window, cx| {
		window.resize(gpui::size(permissions::px(900.), permissions::px(700.)));
		window.draw(cx).clear(cx);
	});

	assert!(visual.debug_bounds("permission-profile-0").is_some());
	assert!(visual.debug_bounds("permission-choice-0").is_some());
	assert!(visual.debug_bounds("permission-choice-1").is_some());
}

#[gpui::test]
fn child_navigation_and_disconnect_cannot_edit_parent_permissions(cx: &mut TestAppContext) {
	let (_root, profile, server) = wire_test_support::fixture(|_| async {});

	server.join().unwrap();

	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		s.permission_profiles.work = Some("root".into());
		s.permission_profiles.state = Some(available());
		s.permission_profiles.reviewed = true;
		s.profile = Some(profile);

		s.open_native_agent("root", "child", cx);

		assert_eq!(s.native_agents.selected, Some(("root".into(), "child".into())));
		assert!(!s.permission_profiles.reviewed);
		assert!(s.permission_profiles.state.is_none());

		s.update_permission_profiles(
			"root".into(),
			Some(WireText::new("scoped").unwrap()),
			None,
			cx,
		);

		assert!(s.permission_profiles.task.is_none());

		s.open_page("root", cx);
		s.apply_result(Ok(AgentSnapshotResult::Unavailable));
		s.update_permission_profiles("root".into(), None, None, cx);

		assert!(s.permission_profiles.task.is_none());
	});
}

#[gpui::test]
fn ordinary_refresh_keeps_permission_read_and_unknown_selection(cx: &mut TestAppContext) {
	let (_dir, profile, server) = fixture();
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, _| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		s.profile = Some(profile);
	});

	for selection in [None, Some(WireText::new("scoped").unwrap())] {
		let saving = selection.is_some();

		surface.update(cx, |s, cx| {
			if saving {
				s.draft_profiles.execution.change("root".into(), |choice| {
					choice.permission_mode = Some(decodex_protocol::PermissionMode::ReadOnly);
					choice.reasoning_effort =
						Some(decodex_protocol::ConversationReasoningEffort::High);
				});
			}
			s.update_permission_profiles("root".into(), selection, None, cx);

			assert!(s.permission_profiles.task.is_some());
			if saving {
				let next = s.draft_profiles.execution.choice("root");
				assert!(next.permission_mode.is_none(), "a live choice supersedes the dormant one");
				assert!(next.reasoning_effort.is_some(), "other next-message choices are retained");
			}

			// Advance the snapshot generation before this operation can complete.
			s.generation += 1;

			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		});

		cx.run_until_parked();
		surface.read_with(cx, |s, _| {
			assert!(
				s.permission_profiles.task.is_none(),
				"refresh must not strand a completed operation"
			);
			assert_eq!(s.permission_profiles.reviewed, !saving);

			if saving {
				assert!(s.permission_profiles.feedback.contains("unconfirmed"));
				assert!(matches!(
					s.permission_profiles.state,
					Some(AgentPermissionState::Pending {
						state: AgentPermissionOutcome::Unknown,
						..
					})
				));
			} else {
				assert!(matches!(
					s.permission_profiles.state,
					Some(AgentPermissionState::Available { .. })
				));
			}
		});
	}

	assert_eq!(server.join().unwrap().len(), 1, "an uncertain selection must not be retried");
}

#[gpui::test]
fn disconnect_invalidates_permission_review_before_same_source_reconnect(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		s.permission_profiles.work = Some("root".into());
		s.permission_profiles.state = Some(available());
		s.permission_profiles.reviewed = true;

		let epoch = s.permission_profiles.epoch;

		s.mark_stale(cx);

		assert_ne!(s.permission_profiles.epoch, epoch);

		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		assert!(s.permission_profiles.state.is_none());
		assert!(!s.permission_profiles.reviewed);
	});
}

async fn serve(listener: UnixListener) -> Vec<AgentActionDto> {
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
			let AgentActionDto::SelectPermissions {
				work_id,
				thread_id,
				review_token,
				profile_id,
				mode: None,
			} = &*action
			else {
				panic!("account setting")
			};

			assert_eq!(work_id.as_str(), "root");
			assert_eq!(thread_id.as_str(), "thread");
			assert_eq!(review_token.as_str(), "a".repeat(64));
			assert_eq!(profile_id.as_str(), "scoped");

			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();

			continue;
		}

		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let QueryPayload::GetAgentPermissionProfiles { work_id } = query.payload else {
			panic!("account query")
		};

		assert_eq!(work_id.as_str(), "root");

		let state = if index == 0 {
			available()
		} else {
			AgentPermissionState::Pending {
				profile_id: WireText::new("scoped").unwrap(),
				state: AgentPermissionOutcome::Unknown,
			}
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentPermissionProfiles(state),
		});

		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}

	actions
}
