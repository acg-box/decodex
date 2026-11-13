//! Suggestion sampling must not submit or replace the ordinary conversation draft.

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{
	AppContext as _, Entity, Focusable as _, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, TestAppContext, Window,
};
use tokio::runtime::Builder;
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::{
	sample::{
		AgentSamplePhase, AgentSampleStatus, AgentSurface, Context, EntityId, NEXT_PROMPT,
		WireText, request, watch,
	},
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	AgentActionDto, AgentSampleAnswer, AgentTimelineContent, AgentTimelineEntry, CURRENT_VERSION,
	ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId,
	ServerMessage,
};

struct SuggestionPanel(Entity<AgentSurface>, usize);
impl Render for SuggestionPanel {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let body = self.0.update(cx, |s, cx| {
			gpui::div().child(s.render_composer_capsule(false, window, cx)).child(s.model.clone())
		});
		body.on_action(cx.listener(|view, _: &crate::shell::FocusNext, window, cx| {
			view.1 += 1;
			window.focus_next(cx);
		}))
		.on_action(
			cx.listener(|_, _: &crate::shell::FocusPrevious, window, cx| window.focus_prev(cx)),
		)
	}
}

fn suggestion_fixture(s: &mut AgentSurface, cx: &mut Context<AgentSurface>) {
	s.visual_workspace_fixture(cx);
	let work = s.selected.clone().unwrap();
	let item = s.snapshot.as_mut().unwrap().work_items.iter_mut().find(|w| w.id == work).unwrap();
	item.parent_goal_id = None;
	item.codex_thread_id = Some("parent".into());
	item.active_turn_id = None;
	s.composer_manager = Some(work.clone());
	s.composer.update(cx, |input, cx| input.set_content("", cx));
	s.open_prompt_suggestion(cx);
	s.sample.state = Some(AgentSampleStatus {
		work_id: EntityId::new(work).unwrap(),
		thread_id: Some(WireText::new("parent").unwrap()),
		request_id: Some(WireText::new("suggestion").unwrap()),
		prompt: Some(WireText::new(NEXT_PROMPT).unwrap()),
		phase: AgentSamplePhase::Ready,
		answer: Some(AgentSampleAnswer {
			source_turn_id: WireText::new("finished-turn").unwrap(),
			response_id: WireText::new("response").unwrap(),
			text: Some("Compare the two approaches.".into()),
			input_tokens: None,
			cached_input_tokens: None,
			output_tokens: None,
		}),
	});
}

fn automatic_fixture(s: &mut AgentSurface, cx: &mut Context<AgentSurface>) {
	suggestion_fixture(s, cx);
	s.reset_sample();
	s.timeline.native.binding = Some(crate::shell::agent_surface::native_timeline::Binding {
		work: s.selected.clone().unwrap(),
		thread: "parent".into(),
		account: "account".into(),
	});
	s.timeline.native.entries.clear();
	s.observe_prompt_completion(cx);
}

fn completed_answer(turn: &str, status: &str) -> Vec<AgentTimelineEntry> {
	vec![
		AgentTimelineEntry {
			position: 0,
			content: AgentTimelineContent::Item {
				turn_id: turn.into(),
				item_id: format!("answer-{turn}"),
				kind: "agentMessage".into(),
				text: "The answer is ready.".into(),
				phase: Some("final_answer".into()),

				truncated: false,
				activity: None,
				attachments: vec![],
				collaboration: None,
			},
		},
		AgentTimelineEntry {
			position: 1,
			content: AgentTimelineContent::TurnBoundary {
				started_at_micros: None,
				turn_id: turn.into(),
				completed: true,
				status: Some(status.into()),
				duration_ms: None,
				usage_summary: None,
				usage: None,
				timing: None,
				error: None,
			},
		},
	]
}

#[gpui::test]
fn automatic_suggestion_sends_once_after_a_new_answer_and_never_sends_the_draft(
	cx: &mut TestAppContext,
) {
	cx.background_executor.allow_parking();
	let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("command") };
		let ClientMessage::Command(command) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("one sample")
		};
		let CommandPayload::Agent { action } = command.payload else { panic!("agent sample") };
		let AgentActionDto::GenerateSample { work_id, thread_id, prompt } = *action else {
			panic!("no send action")
		};
		assert_eq!(thread_id.as_str(), "parent");
		assert_eq!(prompt.as_str(), NEXT_PROMPT);
		socket.close(None).await.unwrap();
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("readback") };
		let ClientMessage::Query(query) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("no inference replay")
		};
		assert!(matches!(query.payload, QueryPayload::GetAgentSample { .. }));
		let status = AgentSampleStatus {
			work_id,
			thread_id: Some(thread_id),
			request_id: Some(WireText::new(command.idempotency_key.as_str()).unwrap()),
			prompt: Some(prompt),
			phase: AgentSamplePhase::Obsolete,
			answer: None,
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentSample(status),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	});
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			automatic_fixture(s, cx);
			s.profile = Some(profile);
			s.timeline.native.entries = completed_answer("new-turn", "completed");
			s.observe_prompt_completion(cx);
			s.observe_prompt_completion(cx);
			assert_eq!(s.composer.read(cx).content(), "");
			assert!(!s.sending);
		});
		SuggestionPanel(surface, 0)
	});
	visual.run_until_parked();
	server.join().unwrap();
	let surface = view.read_with(visual, |view, _| view.0.clone());
	surface.update(visual, |s, cx| {
		s.reset_sample();
		s.composer.update(cx, |input, cx| input.set_content("Already typing", cx));
		s.observe_prompt_completion(cx);
		assert!(s.sample.work.is_none(), "typing does not request another suggestion");
		assert_eq!(s.composer.read(cx).content(), "Already typing");
	});
}

#[gpui::test]
fn automatic_suggestion_recovers_saved_history_and_waits_for_the_active_turn(
	cx: &mut TestAppContext,
) {
	cx.update(|cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			automatic_fixture(s, cx);
			s.timeline.native.binding.as_mut().unwrap().account = "other-account".into();
			s.timeline.native.entries = completed_answer("old", "completed");
			s.observe_prompt_completion(cx);
			assert!(s.sample.work.is_some(), "opening saved history restores its suggestion");
			s.reset_sample();
			let selected = s.selected.clone().unwrap();
			let item = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == selected)
				.unwrap();
			item.active_turn_id = Some("new".into());
			s.observe_prompt_completion(cx);
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == selected)
				.unwrap()
				.active_turn_id = None;
			s.observe_prompt_completion(cx);
			assert!(
				s.sample.work.is_none(),
				"wait for the completed active turn, not an old history page"
			);
			s.timeline.native.entries = completed_answer("failed", "failed");
			s.observe_prompt_completion(cx);
			assert!(s.sample.work.is_none(), "failed turns do not produce suggestions");
			s.snapshot.as_mut().unwrap().runtime_source =
				Some(EntityId::new("replacement").unwrap());
			s.timeline.native.entries = completed_answer("other-source", "completed");
			s.observe_prompt_completion(cx);
			assert!(s.sample.work.is_some(), "a new source can generate its own suggestion");
		});
	});
}

#[gpui::test]
fn long_suggestion_wraps_and_an_existing_chat_never_returns_to_a_quote(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, suggestion_fixture);
		SuggestionPanel(surface, 0)
	});
	let surface = view.read_with(visual, |v, _| v.0.clone());
	visual.simulate_resize(gpui::size(gpui::px(360.), gpui::px(800.)));
	visual.update(crate::ui_frame::draw_current_frame);
	let short = visual.debug_bounds("composer-editor-area").unwrap().size.height;
	let long = "请把刚才讨论的两个方案逐项比较，说明各自的维护成本、适用条件和风险，再给出下一步可以执行的建议。".repeat(3);
	surface.update(visual, |s, cx| {
		s.sample.state.as_mut().unwrap().answer.as_mut().unwrap().text = Some(long.clone());
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("composer-editor-area").unwrap().size.height > short);
	surface.update(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).placeholder(), long);
		s.reset_sample();
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	surface.read_with(visual, |s, cx| assert_eq!(s.composer.read(cx).placeholder(), "Message"));
}

#[gpui::test]
fn tab_accepts_prompt_suggestion_as_an_undoable_unsent_draft(cx: &mut TestAppContext) {
	cx.update(crate::shell::bind_keys);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, suggestion_fixture);
		SuggestionPanel(surface, 0)
	});
	let surface = view.read_with(visual, |view, _| view.0.clone());
	for width in [180., 280., 650.] {
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(width), gpui::px(700.)));
			window.draw(cx).clear(cx);
		});
		let text = visual.debug_bounds("composer-input-text").expect("full-width composer");
		let editor = visual.debug_bounds("composer-editor-area").unwrap();
		assert!((text.size.width - editor.size.width).abs() < gpui::px(1.));
		let keycap =
			visual.debug_bounds("composer-suggestion-keycap").expect("separate keyboard cue");
		assert_eq!(keycap.size.height, gpui::px(18.));
		assert!(keycap.left() >= editor.left() && keycap.right() <= editor.right());
		assert!(keycap.top() >= editor.top() && keycap.bottom() <= editor.bottom());
		assert!(
			surface
				.read_with(visual, |s, cx| s.composer.read(cx).placeholder_hint() == Some("Tab"))
		);
	}
	surface.update(visual, |s, cx| {
		assert!(s.composer.read(cx).content().is_empty());
		assert_eq!(s.composer.read(cx).placeholder(), "Compare the two approaches.");
		assert!(!s.sending);
	});
	assert!(visual.debug_bounds("sample-panel").is_none());
	visual.update(|window, cx| window.focus(&surface.read(cx).composer.focus_handle(cx), cx));
	visual.simulate_keystrokes("tab");
	surface.update(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Compare the two approaches.");
		assert_eq!(s.sample.work, s.selected, "keep the cached suggestion after acceptance");
		assert!(!s.sending);
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(surface.read_with(visual, |s, cx| s.composer.read(cx).placeholder_hint().is_none()));
	assert!(visual.debug_bounds("composer-suggestion-keycap").is_none());
	assert_eq!(view.read_with(visual, |view, _| view.1), 0, "accept without moving focus");
	visual.update(|window, cx| window.focus(&surface.read(cx).composer.focus_handle(cx), cx));
	visual.simulate_keystrokes("cmd-z");
	surface.update(visual, |s, cx| assert!(s.composer.read(cx).content().is_empty()));
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("composer-suggestion-accept").is_some());
	assert!(surface.read_with(visual, |s, cx| s.composer.read(cx).placeholder()
		== "Compare the two approaches."));
}

#[gpui::test]
fn clicking_tab_and_clearing_the_draft_restores_the_same_suggestion(cx: &mut TestAppContext) {
	cx.update(crate::shell::bind_keys);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, suggestion_fixture);
		SuggestionPanel(surface, 0)
	});
	let surface = view.read_with(visual, |view, _| view.0.clone());
	visual.simulate_resize(gpui::size(gpui::px(650.), gpui::px(700.)));
	visual.update(crate::ui_frame::draw_current_frame);
	let accept = visual.debug_bounds("composer-suggestion-accept").unwrap();
	visual.simulate_click(accept.center(), Default::default());
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Compare the two approaches.");
		assert!(!s.sending);
	});
	visual.simulate_keystrokes("cmd-a backspace");
	visual.update(crate::ui_frame::draw_current_frame);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "");
		assert_eq!(s.composer.read(cx).placeholder(), "Compare the two approaches.");
		assert_eq!(
			s.sample.state.as_ref().unwrap().request_id.as_ref().unwrap().as_str(),
			"suggestion"
		);
		assert!(!s.sending);
	});
	assert!(visual.debug_bounds("composer-suggestion-accept").is_some());
	assert!(visual.debug_bounds("composer-suggestion-refresh").is_some());
}

#[gpui::test]
async fn refresh_requests_one_different_suggestion_without_sending_a_chat_message(
	cx: &mut TestAppContext,
) {
	cx.background_executor.allow_parking();
	let (reply, ready) = tokio::sync::oneshot::channel();
	let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("generation")
		};
		let ClientMessage::Command(command) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("generation command")
		};
		let CommandPayload::Agent { action } = command.payload else { panic!("agent command") };
		let AgentActionDto::GenerateSample { work_id, thread_id, prompt } = *action else {
			panic!("must not send a chat message")
		};
		assert_ne!(prompt.as_str(), NEXT_PROMPT);
		assert!(prompt.as_str().contains("Compare the two approaches."));
		assert!(prompt.as_str().contains("different suggestion"));
		assert!(prompt.as_str().len() <= decodex_protocol::MAX_AGENT_SAMPLE_PROMPT_BYTES);
		socket.close(None).await.unwrap();
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("status") };
		let ClientMessage::Query(query) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("no duplicate generation")
		};
		assert!(matches!(query.payload, QueryPayload::GetAgentSample { .. }));
		ready.await.unwrap();
		let state = AgentSampleStatus {
			work_id,
			thread_id: Some(thread_id),
			request_id: Some(WireText::new(command.idempotency_key.as_str()).unwrap()),
			prompt: Some(prompt),
			phase: AgentSamplePhase::Ready,
			answer: Some(AgentSampleAnswer {
				source_turn_id: WireText::new("finished-turn").unwrap(),
				response_id: WireText::new("refreshed-response").unwrap(),
				text: Some("Check the trade-offs next.".into()),
				input_tokens: None,
				cached_input_tokens: None,
				output_tokens: None,
			}),
		};
		wire_test_support::answer_query(
			&mut socket,
			query.query_id,
			QueryResultPayload::AgentSample(state),
		)
		.await;
	});
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			suggestion_fixture(s, cx);
			s.profile = Some(profile);
		});
		SuggestionPanel(surface, 0)
	});
	let surface = view.read_with(visual, |view, _| view.0.clone());
	visual.simulate_resize(gpui::size(gpui::px(650.), gpui::px(700.)));
	visual.update(crate::ui_frame::draw_current_frame);
	let refresh = visual.debug_bounds("composer-suggestion-refresh").unwrap();
	visual.simulate_click(refresh.center(), Default::default());
	surface.update(visual, |s, cx| {
		assert!(s.prompt_suggestion_busy());
		assert_eq!(s.prompt_suggestion(cx), Some("Compare the two approaches."));
		let epoch = s.sample.epoch;
		s.refresh_prompt_suggestion(cx);
		assert_eq!(s.sample.epoch, epoch, "ignore repeated refresh while busy");
		s.accept_prompt_suggestion(cx);
		assert_eq!(s.composer.read(cx).content(), "");
	});
	reply.send(()).unwrap();
	visual
		.condition(&surface, |s, _| s.suggested_prompt() == Some("Check the trade-offs next."))
		.await;
	server.join().unwrap();
	surface.update(visual, |s, cx| {
		assert_eq!(s.prompt_suggestion(cx), Some("Check the trade-offs next."));
		assert_eq!(s.composer.read(cx).content(), "");
		assert!(!s.sending);
		s.reset_sample();
	});
}

#[gpui::test]
fn tab_preserves_normal_focus_navigation_without_a_current_empty_draft_suggestion(
	cx: &mut TestAppContext,
) {
	cx.update(crate::shell::bind_keys);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, suggestion_fixture);
		SuggestionPanel(surface, 0)
	});
	let surface = view.read_with(visual, |view, _| view.0.clone());
	visual.update(|window, cx| {
		window.resize(gpui::size(gpui::px(650.), gpui::px(700.)));
		window.draw(cx).clear(cx);
		window.focus(&surface.read(cx).composer.focus_handle(cx), cx);
	});
	visual.simulate_keystrokes("shift-tab");
	assert!(surface.read_with(visual, |s, cx| s.composer.read(cx).content().is_empty()));
	visual.update(|window, cx| window.focus(&surface.read(cx).model.focus_handle(cx), cx));
	visual.simulate_keystrokes("tab");
	assert!(surface.read_with(visual, |s, cx| s.composer.read(cx).content().is_empty()));
	surface.update(visual, |s, cx| {
		s.composer.update(cx, |input, cx| input.set_content("Keep typing", cx))
	});
	visual.update(|window, cx| window.focus(&surface.read(cx).composer.focus_handle(cx), cx));
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(surface.read_with(visual, |s, cx| s.composer.read(cx).placeholder_hint().is_none()));
	visual.simulate_keystrokes("tab");
	assert_eq!(
		surface.read_with(visual, |s, cx| s.composer.read(cx).content().to_owned()),
		"Keep typing"
	);
	surface.update(visual, |s, cx| {
		s.composer.update(cx, |input, cx| input.set_content("", cx));
		s.sample.state.as_mut().unwrap().phase = AgentSamplePhase::Obsolete;
	});
	visual.update(|window, cx| window.focus(&surface.read(cx).composer.focus_handle(cx), cx));
	visual.simulate_keystrokes("tab");
	assert_eq!(view.read_with(visual, |view, _| view.1), 3);
	assert!(surface.read_with(visual, |s, cx| s.composer.read(cx).content().is_empty()));
}

#[gpui::test]
fn prompt_suggestion_preserves_later_edits_and_rejects_stale_or_unrelated_results(
	cx: &mut TestAppContext,
) {
	cx.update(|cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			suggestion_fixture(s, cx);
			s.composer.update(cx, |input, cx| input.set_content("My newer draft", cx));
			s.accept_prompt_suggestion(cx);
			assert_eq!(s.composer.read(cx).content(), "My newer draft");
			s.composer.update(cx, |input, cx| input.set_content("", cx));
			s.sample.state.as_mut().unwrap().prompt =
				Some(WireText::new("A separate question").unwrap());
			s.accept_prompt_suggestion(cx);
			assert!(s.composer.read(cx).content().is_empty());
			s.sample.state.as_mut().unwrap().prompt = Some(WireText::new(NEXT_PROMPT).unwrap());
			let mut next = s.snapshot.clone().unwrap();
			next.work_items
				.iter_mut()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.unwrap()
				.codex_thread_id = Some("replacement".into());
			s.invalidate_sample(&next);
			s.snapshot = Some(next);
			s.accept_prompt_suggestion(cx);
			assert!(s.composer.read(cx).content().is_empty());
			assert!(s.sample.work.is_none());
		});
	});
}

#[test]
fn a_lost_sample_acknowledgement_is_read_back_without_another_model_command() {
	let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("command frame")
		};
		let ClientMessage::Command(command) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("one explicit command")
		};
		let CommandPayload::Agent { action } = command.payload else { panic!("sample command") };
		let AgentActionDto::GenerateSample { work_id, thread_id, prompt } = *action else {
			panic!("sample action")
		};

		assert_eq!(prompt.as_str(), "Another approach?");

		socket.close(None).await.unwrap();

		for phase in
			[AgentSamplePhase::Pending, AgentSamplePhase::Ready, AgentSamplePhase::Obsolete]
		{
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("query frame")
			};
			let ClientMessage::Query(query) = serde_json::from_str::<ClientMessage>(&text).unwrap()
			else {
				panic!("generation must not replay")
			};

			assert!(matches!(query.payload, QueryPayload::GetAgentSample { .. }));

			let status = AgentSampleStatus {
				work_id: work_id.clone(),
				thread_id: Some(thread_id.clone()),
				request_id: Some(WireText::new(command.idempotency_key.as_str()).unwrap()),
				prompt: Some(prompt.clone()),
				phase,
				answer: (phase == AgentSamplePhase::Ready).then(|| AgentSampleAnswer {
					source_turn_id: WireText::new("turn").unwrap(),
					response_id: WireText::new("response").unwrap(),
					text: Some("A separate answer.".into()),
					input_tokens: None,
					cached_input_tokens: None,
					output_tokens: None,
				}),
			};
			let result = ServerMessage::QueryResult(QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).unwrap(),
				query_id: query.query_id,
				payload: QueryResultPayload::AgentSample(status),
			});

			socket
				.send(Message::Text(serde_json::to_string(&result).unwrap().into()))
				.await
				.unwrap();
			socket.close(None).await.unwrap();
		}
	});
	let runtime = Builder::new_current_thread().enable_all().build().unwrap();

	runtime.block_on(async {
		let (_closed, closure) = watch::channel(false);
		let (updates, results) = watch::channel(None);

		request::run(
			profile,
			EntityId::new("root").unwrap(),
			WireText::new("parent").unwrap(),
			WireText::new("Another approach?").unwrap(),
			closure,
			updates,
		)
		.await;

		let result = results.borrow();
		let status = result.as_ref().unwrap().0.as_ref().unwrap();

		assert_eq!(
			status.phase,
			AgentSamplePhase::Obsolete,
			"read back source changes after displaying the answer"
		);
		assert!(status.answer.is_none());
	});

	server.join().unwrap();
}

#[gpui::test]
fn unfinished_or_unavailable_suggestions_leave_the_input_bar_quiet(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, suggestion_fixture);
		SuggestionPanel(surface, 0)
	});
	let surface = view.read_with(visual, |view, _| view.0.clone());

	for phase in [
		AgentSamplePhase::Pending,
		AgentSamplePhase::Cancelling,
		AgentSamplePhase::Cancelled,
		AgentSamplePhase::Failed,
		AgentSamplePhase::Obsolete,
	] {
		surface.update(visual, |s, _| s.sample.state.as_mut().unwrap().phase = phase);
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("sample-panel").is_none());
		assert!(
			surface.read_with(visual, |s, cx| s.composer.read(cx).placeholder_hint().is_none())
		);
		assert!(surface.read_with(visual, |s, cx| s.composer.read(cx).content().is_empty()));
	}
}
