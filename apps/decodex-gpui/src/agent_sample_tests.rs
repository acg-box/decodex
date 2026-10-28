//! Separate answer controls must not submit or replace the ordinary conversation draft.

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{
	AppContext as _, Focusable as _, InteractiveElement as _, ParentElement as _, Render,
	TestAppContext, Window,
};
use tokio::runtime::Builder;
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::{
	sample::{
		AgentSamplePhase, AgentSampleStatus, AgentSurface, Context, Entity, EntityId, IntoElement,
		NEXT_PROMPT, WireText, request, request::Operation, watch,
	},
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	AgentActionDto, AgentSampleAnswer, AgentTimelineContent, AgentTimelineEntry, CURRENT_VERSION,
	ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId,
	ServerMessage,
};

struct SamplePanel(Entity<AgentSurface>);
impl Render for SamplePanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| s.sample_panel(&s.selected.clone().unwrap(), cx))
	}
}

struct SuggestionPanel(Entity<AgentSurface>, usize);
impl Render for SuggestionPanel {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let body = self.0.update(cx, |s, cx| {
			gpui::div()
				.child(s.sample_panel(&s.selected.clone().unwrap(), cx))
				.child(s.render_composer_capsule(false, window, cx))
				.child(s.model.clone())
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
	s.suggest_next_prompt(cx);
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
				app_ui: false,
				truncated: false,
				activity: None,
				attachments: vec![],
				collaboration: None,
			},
		},
		AgentTimelineEntry {
			position: 1,
			content: AgentTimelineContent::TurnBoundary {
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
			assert!(s.sample.suggestion);
			assert!(s.composer.read(cx).content().is_empty());
			assert!(!s.sending);
		});
		SuggestionPanel(surface, 0)
	});
	visual.run_until_parked();
	server.join().unwrap();
	let surface = view.read_with(visual, |view, _| view.0.clone());
	surface.update(visual, |s, cx| {
		s.reset_sample();
		s.observe_prompt_completion(cx);
		assert!(s.sample.work.is_none(), "closing cannot regenerate the same turn");
		assert!(s.composer.read(cx).content().is_empty());
	});
}

#[gpui::test]
fn automatic_suggestion_skips_old_history_drafts_failed_turns_and_replaced_sources(
	cx: &mut TestAppContext,
) {
	cx.update(|cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			automatic_fixture(s, cx);
			s.timeline.native.binding.as_mut().unwrap().account = "other-account".into();
			s.timeline.native.entries = completed_answer("old", "completed");
			s.observe_prompt_completion(cx);
			assert!(s.sample.work.is_none(), "opening saved history is not a new answer");
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
			s.composer.update(cx, |input, cx| input.set_content("Already typing", cx));
			s.timeline.native.entries = completed_answer("new", "completed");
			s.observe_prompt_completion(cx);
			assert!(s.sample.work.is_none());
			s.composer.update(cx, |input, cx| input.set_content("", cx));
			s.observe_prompt_completion(cx);
			assert!(s.sample.work.is_none(), "clearing a draft does not replay a skipped trigger");
			s.timeline.native.entries = completed_answer("failed", "failed");
			s.observe_prompt_completion(cx);
			assert!(s.sample.work.is_none());
			s.snapshot.as_mut().unwrap().runtime_source =
				Some(EntityId::new("replacement").unwrap());
			s.timeline.native.entries = completed_answer("other-source", "completed");
			s.observe_prompt_completion(cx);
			assert!(s.sample.work.is_none());
		});
	});
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
	visual.update(|window, cx| {
		window.resize(gpui::size(gpui::px(650.), gpui::px(700.)));
		window.draw(cx).clear(cx);
	});
	surface.update(visual, |s, cx| {
		assert!(s.sample.input.is_none());
		assert!(s.composer.read(cx).content().is_empty());
		assert!(!s.sending);
	});
	assert!(visual.debug_bounds("sample-use-draft").is_none());
	visual.update(|window, cx| window.focus(&surface.read(cx).composer.focus_handle(cx), cx));
	visual.simulate_keystrokes("tab");
	surface.update(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Compare the two approaches.");
		assert!(s.sample.work.is_none());
		assert!(!s.sending);
	});
	assert_eq!(view.read_with(visual, |view, _| view.1), 0, "accept without moving focus");
	visual.update(|window, cx| window.focus(&surface.read(cx).composer.focus_handle(cx), cx));
	visual.simulate_keystrokes("cmd-z");
	surface.update(visual, |s, cx| assert!(s.composer.read(cx).content().is_empty()));
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

#[gpui::test]
fn separate_answer_preserves_the_composer_and_hides_a_replaced_source(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s.selected.clone().unwrap();
			let item =
				s.snapshot.as_mut().unwrap().work_items.iter_mut().find(|w| w.id == work).unwrap();

			item.parent_goal_id = None;
			item.codex_thread_id = Some("parent".into());

			s.composer.update(cx, |input, cx| {
				input.set_content("Keep this main conversation draft.", cx)
			});
			s.open_sample(cx);
			s.sample
				.input
				.as_ref()
				.unwrap()
				.update(cx, |input, cx| input.set_content("Another approach?", cx));

			s.sample.state = Some(AgentSampleStatus {
				work_id: EntityId::new(work).unwrap(),
				thread_id: Some(WireText::new("parent").unwrap()),
				request_id: Some(WireText::new("request").unwrap()),
				prompt: Some(WireText::new("Another approach?").unwrap()),
				phase: AgentSamplePhase::Ready,
				answer: Some(AgentSampleAnswer {
					source_turn_id: WireText::new("turn").unwrap(),
					response_id: WireText::new("response").unwrap(),
					text: Some("A separate answer.".into()),
					input_tokens: Some(12),
					cached_input_tokens: Some(4),
					output_tokens: Some(3),
				}),
			});
		});

		SamplePanel(surface)
	});

	visual.update(|window, cx| {
		window.resize(gpui::size(gpui::px(650.), gpui::px(700.)));
		window.draw(cx).clear(cx);
	});

	assert!(visual.debug_bounds("sample-panel").is_some());
	assert!(visual.debug_bounds("sample-close").is_some());

	let surface = view.read_with(visual, |view, _| view.0.clone());

	surface.update(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Keep this main conversation draft.");
		assert_eq!(s.sample.input.as_ref().unwrap().read(cx).content(), "Another approach?");

		let mut next = s.snapshot.clone().unwrap();

		next.runtime_source = Some(EntityId::new("replacement").unwrap());

		s.invalidate_sample(&next);

		assert!(s.sample.state.is_none());
		assert!(s.sample.work.is_none());
		assert_eq!(s.composer.read(cx).content(), "Keep this main conversation draft.");
	});

	visual.update(crate::ui_frame::draw_current_frame);

	assert!(visual.debug_bounds("sample-panel").is_none());
}

#[gpui::test]
fn enter_in_a_separate_question_does_not_reach_the_main_send_action(cx: &mut TestAppContext) {
	cx.update(crate::composer_input::bind_keys);

	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s.selected.clone().unwrap();
			let item =
				s.snapshot.as_mut().unwrap().work_items.iter_mut().find(|w| w.id == work).unwrap();

			item.parent_goal_id = None;
			item.codex_thread_id = Some("parent".into());

			s.composer.update(cx, |input, cx| input.set_content("Do not submit this draft.", cx));
			s.open_sample(cx);
		});

		SamplePanel(surface)
	});
	let surface = view.read_with(visual, |view, _| view.0.clone());

	visual.update(|window, cx| {
		window.resize(gpui::size(gpui::px(650.), gpui::px(700.)));

		let input = surface.read(cx).sample.input.as_ref().unwrap().clone();

		window.focus(&input.focus_handle(cx), cx);
		window.draw(cx).clear(cx);
	});

	assert!(visual.debug_bounds("sample-question").is_some());

	let feedback = surface.read_with(visual, |s, _| s.feedback.message.clone());

	visual.simulate_keystrokes("cmd-enter");
	visual.simulate_keystrokes("enter");
	surface.update(visual, |s, cx| {
		assert_eq!(
			s.feedback.message, feedback,
			"Enter in the separate question must not reach main send validation"
		);
		assert!(!s.sending);
		assert!(!s.uncertain);
		assert_eq!(s.composer.read(cx).content(), "Do not submit this draft.");
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
			Operation::Generate(WireText::new("Another approach?").unwrap()),
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
fn cancellation_controls_wait_for_native_completion_and_preserve_the_draft(
	cx: &mut TestAppContext,
) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s.selected.clone().unwrap();

			s.sample.work = Some(work.clone());
			s.sample.thread = Some("parent".into());

			s.composer.update(cx, |input, cx| input.set_content("Keep this draft.", cx));

			s.sample.state = Some(AgentSampleStatus {
				work_id: EntityId::new(work).unwrap(),
				thread_id: Some(WireText::new("parent").unwrap()),
				request_id: Some(WireText::new("generation").unwrap()),
				prompt: Some(WireText::new("Question").unwrap()),
				phase: AgentSamplePhase::Pending,
				answer: None,
			});
		});

		SamplePanel(surface)
	});
	let surface = view.read_with(visual, |view, _| view.0.clone());

	for phase in
		[AgentSamplePhase::Pending, AgentSamplePhase::Cancelling, AgentSamplePhase::Cancelled]
	{
		surface.update(visual, |s, _| s.sample.state.as_mut().unwrap().phase = phase);

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(650.), gpui::px(700.)));
			window.draw(cx).clear(cx);
		});

		assert_eq!(
			visual.debug_bounds("sample-cancel").is_some(),
			phase == AgentSamplePhase::Pending
		);
		assert!(visual.debug_bounds("sample-close").is_some());
		assert_eq!(
			surface.read_with(visual, |s, cx| s.composer.read(cx).content().to_owned()),
			"Keep this draft."
		);
	}

	surface.update(visual, |s, _| s.reset_sample());

	assert_eq!(
		surface.read_with(visual, |s, cx| s.composer.read(cx).content().to_owned()),
		"Keep this draft."
	);
}

#[test]
fn a_lost_cancellation_acknowledgement_reads_back_the_exact_sample_without_replay() {
	let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("command frame")
		};
		let ClientMessage::Command(command) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("cancel command")
		};
		let CommandPayload::Agent { action } = command.payload else { panic!("agent command") };
		let AgentActionDto::CancelSample { work_id, request_id } = *action else {
			panic!("cancellation only")
		};

		assert_eq!(request_id.as_str(), "original-generation");

		socket.close(None).await.unwrap();

		for phase in [AgentSamplePhase::Cancelling, AgentSamplePhase::Cancelled] {
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("query frame")
			};
			let ClientMessage::Query(query) = serde_json::from_str::<ClientMessage>(&text).unwrap()
			else {
				panic!("never repeat a sample command")
			};

			assert!(matches!(query.payload, QueryPayload::GetAgentSample { .. }));

			let state = AgentSampleStatus {
				work_id: work_id.clone(),
				thread_id: Some(WireText::new("parent").unwrap()),
				request_id: Some(request_id.clone()),
				prompt: Some(WireText::new("Question").unwrap()),
				phase,
				answer: None,
			};
			let result = ServerMessage::QueryResult(QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).unwrap(),
				query_id: query.query_id,
				payload: QueryResultPayload::AgentSample(state),
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
			Operation::Cancel(WireText::new("original-generation").unwrap()),
			closure,
			updates,
		)
		.await;

		assert_eq!(
			results.borrow().as_ref().unwrap().0.as_ref().unwrap().phase,
			AgentSamplePhase::Cancelled
		);
	});

	server.join().unwrap();
}
