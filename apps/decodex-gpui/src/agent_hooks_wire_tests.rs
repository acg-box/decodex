//! Hook clicks cross the public socket; lost replies trigger reads, not retries.
use std::thread::JoinHandle;

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::TestAppContext;
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::{hooks::*, wire_test_support, wire_test_support::SERVER};
use decodex_protocol::{
	AgentHookEditReceipt, AgentWorkKindDto, CURRENT_VERSION, ClientMessage, CommandPayload,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId, ServerMessage,
};

struct HookView {
	surface: Entity<AgentSurface>,
}
impl Render for HookView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.hook_settings_panel(&work(), cx))
	}
}

fn fixture(restore: bool) -> (TempDir, ClientProfile, JoinHandle<Vec<AgentActionDto>>) {
	wire_test_support::fixture(move |listener| serve(listener, restore))
}

fn available() -> State {
	State::Available {
		work_id: EntityId::new("root").unwrap(),
		thread_id: EntityId::new("thread").unwrap(),
		review_token: WireText::new("a".repeat(64)).unwrap(),
		config_file: WireText::new("/native/config.toml").unwrap(),
		hooks: vec![decodex_protocol::AgentHookDto {
			key: WireText::new("fixture-hook").unwrap(),
			trust_status: "untrusted".into(),
			enabled: true,
			managed: false,
			current_hash: WireText::new("hash").unwrap(),
			saved_enabled: Some(true),
			saved_hash: None,
			details: "UserPromptSubmit · echo fixture · /native/hooks.json".into(),
		}],
		notices: vec![],
		can_update: true,
		last_edit: None,
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
		runtime_source: Some(EntityId::new("source").unwrap()),
		workspaces: vec![],
		work_items: vec![work()],
		dependencies: vec![],
		pending_events: vec![],
	}
}

#[gpui::test]
fn hook_click_sends_once_and_retains_unknown_after_lost_reply(cx: &mut TestAppContext) {
	for restore in [false, true] {
		let (_dir, profile, server) = fixture(restore);
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);

			cx.observe(&surface, |_, _, cx| cx.notify()).detach();

			surface.update(cx, |s, cx| {
				s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
				s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

				s.profile = Some(profile);
			});

			HookView { surface }
		});

		visual.update(|w, cx| {
			w.resize(gpui::size(px(900.), px(700.)));
			w.draw(cx).clear();
		});

		let button = visual.debug_bounds("hook-settings-read").unwrap();

		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		let button = visual.debug_bounds("hook-setting-0-0").unwrap();

		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();

		assert_eq!(server.join().unwrap().len(), 1);

		let surface = view.read_with(visual, |v, _| v.surface.clone());

		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
			assert!(s.hook_settings.task.is_none());
			assert!(!s.hook_settings.reviewed);
			assert!(s.hook_settings.feedback.contains("could not be confirmed"));
			assert!(matches!(
				s.hook_settings.state,
				Some(State::Available { can_update: false, .. })
			));
		});
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		assert!(visual.debug_bounds("hook-setting-0-0").is_none());

		surface.update(visual, |s, _| s.apply_result(Ok(AgentSnapshotResult::Unavailable)));
		surface.read_with(visual, |s, _| {
			assert!(s.hook_settings.state.is_none());
			assert!(s.hook_settings.work.is_none());
		});
	}
}

#[gpui::test]
fn hook_review_is_invalidated_on_task_or_source_transition(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	for change in ["thread", "running", "removed", "source"] {
		surface.update(cx, |s, _| {
			let original = snapshot();

			s.apply_result(Ok(AgentSnapshotResult::Available(original.clone())));

			s.hook_settings.work = Some("root".into());
			s.hook_settings.state = Some(available());

			let epoch = s.hook_settings.epoch;
			let mut next = original.clone();

			match change {
				"thread" => next.work_items[0].codex_thread_id = Some("other".into()),
				"running" => next.work_items[0].dispatch_state = AgentDispatchStateDto::Running,
				"removed" => next.work_items.clear(),
				_ => next.runtime_source = Some(EntityId::new("other-source").unwrap()),
			}

			s.apply_result(Ok(AgentSnapshotResult::Available(next)));

			assert_ne!(s.hook_settings.epoch, epoch, "{change}");

			s.apply_result(Ok(AgentSnapshotResult::Available(original)));

			assert!(s.hook_settings.state.is_none());
		});
	}
}

#[gpui::test]
fn child_navigation_and_disconnect_cannot_edit_parent_hooks(cx: &mut TestAppContext) {
	let (_root, profile, server) = wire_test_support::fixture(|_| async {});

	server.join().unwrap();

	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		s.hook_settings.work = Some("root".into());
		s.hook_settings.state = Some(available());
		s.hook_settings.reviewed = true;
		s.profile = Some(profile);

		s.open_native_agent("root", "child", cx);

		assert_eq!(s.native_agents.selected, Some(("root".into(), "child".into())));
		assert!(!s.hook_settings.reviewed);
		assert!(s.hook_settings.state.is_none());

		s.update_hook_settings(
			"root".into(),
			Some((WireText::new("fixture-hook").unwrap(), Change::Trust)),
			cx,
		);

		assert!(s.hook_settings.task.is_none());

		s.open_page("root", cx);
		s.apply_result(Ok(AgentSnapshotResult::Unavailable));
		s.update_hook_settings("root".into(), None, cx);

		assert!(s.hook_settings.task.is_none());
	});
}

#[gpui::test]
fn running_hook_setting_controls_follow_current_service_eligibility(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, _| {
			let mut next = snapshot();

			next.work_items[0].dispatch_state = AgentDispatchStateDto::Running;
			next.work_items[0].active_turn_id = Some("active".into());

			s.apply_result(Ok(AgentSnapshotResult::Available(next)));

			s.hook_settings.work = Some("root".into());
			s.hook_settings.state = Some(available());
			s.hook_settings.reviewed = true;
		});

		HookView { surface }
	});

	visual.update(|w, cx| {
		w.resize(gpui::size(px(900.), px(700.)));
		w.draw(cx).clear();
	});

	assert!(visual.debug_bounds("hook-setting-0-0").is_some());

	let surface = view.read_with(visual, |v, _| v.surface.clone());

	surface.update(visual, |s, cx| {
		if let Some(State::Available { can_update, .. }) = &mut s.hook_settings.state {
			*can_update = false;
		}

		cx.notify();
	});

	visual.update(|w, cx| {
		w.draw(cx).clear();
	});

	assert!(visual.debug_bounds("hook-setting-0-0").is_none());
}

#[gpui::test]
fn managed_and_unknown_hooks_cannot_offer_consent_actions(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, _| {
		s.hook_settings.work = Some("root".into());

		for (managed, status) in [(true, "trusted"), (false, "managed"), (false, "future-policy")] {
			let mut state = available();

			if let State::Available { hooks, .. } = &mut state {
				hooks[0].managed = managed;
				hooks[0].trust_status = status.into();
			}

			s.hook_settings.state = Some(state);

			for change in [Change::Trust, Change::Enabled(false)] {
				assert!(
					s.hook_setting_action(
						&EntityId::new("root").unwrap(),
						"thread",
						WireText::new("fixture-hook").unwrap(),
						change
					)
					.is_none()
				);
			}
		}
	});
}

#[gpui::test]
fn ordinary_refresh_keeps_hook_read_and_write_readback(cx: &mut TestAppContext) {
	let (_dir, profile, server) = fixture(false);
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, _| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		s.profile = Some(profile);
	});

	for selection in [None, Some((WireText::new("fixture-hook").unwrap(), Change::Trust))] {
		let saving = selection.is_some();

		surface.update(cx, |s, cx| {
			s.update_hook_settings("root".into(), selection, cx);

			assert!(s.hook_settings.task.is_some());
			// Advance the snapshot generation before the operation can complete.
			s.generation += 1;

			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		});

		cx.run_until_parked();
		surface.read_with(cx, |s, _| {
			assert!(
				s.hook_settings.task.is_none(),
				"refresh must not strand a completed operation"
			);
			assert!(matches!(s.hook_settings.state, Some(State::Available { .. })));
			assert_eq!(s.hook_settings.reviewed, !saving);

			if saving {
				assert!(s.hook_settings.feedback.contains("could not be confirmed"));
			}
		});
	}

	assert_eq!(server.join().unwrap().len(), 1, "an uncertain write must not be retried");
}

#[gpui::test]
fn disconnect_invalidates_hook_review_before_same_source_reconnect(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		s.hook_settings.work = Some("root".into());
		s.hook_settings.state = Some(available());
		s.hook_settings.reviewed = true;

		let epoch = s.hook_settings.epoch;

		s.mark_stale(cx);

		assert_ne!(s.hook_settings.epoch, epoch);

		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		assert!(s.hook_settings.state.is_none());
		assert!(!s.hook_settings.reviewed);
	});
}

async fn serve(listener: UnixListener, restore: bool) -> Vec<AgentActionDto> {
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
			let AgentActionDto::SetHookSetting {
				work_id,
				thread_id,
				review_token,
				hook_key,
				change,
			} = &*action
			else {
				panic!("hook command")
			};

			assert_eq!(work_id.as_str(), "root");
			assert_eq!(thread_id.as_str(), "thread");
			assert_eq!(review_token.as_str(), "a".repeat(64));
			assert_eq!(hook_key.as_str(), "fixture-hook");
			assert_eq!(*change, if restore { Change::Enabled(false) } else { Change::Trust });

			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();

			continue;
		}

		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let QueryPayload::GetAgentHookSettings { work_id } = query.payload else {
			panic!("account query")
		};

		assert_eq!(work_id.as_str(), "root");

		let mut state = available();

		if let State::Available { hooks, can_update, last_edit, .. } = &mut state {
			if restore {
				hooks[0].trust_status = "trusted".into();
				hooks[0].saved_hash = Some("hash".into());
			}
			if index == 2 {
				*can_update = false;
				*last_edit = Some(Box::new(AgentHookEditReceipt {
					outcome: "unknown".into(),
					hook: WireText::new("fixture-hook").unwrap(),
					work_id: EntityId::new("other-task").unwrap(),
					account_id: EntityId::new("account").unwrap(),
				}));
			}
		}

		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentHookSettings(state),
		});

		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}

	actions
}
