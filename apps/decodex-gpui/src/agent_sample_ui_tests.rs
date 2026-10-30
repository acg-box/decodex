//! Exercise direct entry, selected reference, dismissal and composer isolation.
use gpui::{Context, Entity, MouseButton, Render, TestAppContext, Window};

use super::*;
use crate::shell::agent_surface::wire_test_support::{self, SERVER};
use decodex_protocol::{
	AgentActionDto, CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload,
	QueryResultEnvelope, QueryResultPayload, ServerId, ServerMessage,
};
use futures_util::{SinkExt as _, StreamExt as _};
use tokio_tungstenite::tungstenite::Message;

struct Preview(Entity<AgentSurface>);
impl Render for Preview {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| {
			gpui::div()
				.size_full()
				.relative()
				.child(
					gpui::div()
						.group("history-response-preview")
						.child(markdown::render_with_selection_action(
							"Uses **cursor pagination**.",
							"quick-ask-text",
							s.sample_selection_action(cx),
						))
						.children(s.sample_response_action(
							"Uses **cursor pagination**.",
							"preview",
							cx,
						)),
				)
				.children(s.sample_toolbar_action(cx))
				.child(s.composer.clone())
				.child(s.sample_popover(window, cx))
		})
	}
}

fn preview(cx: &mut TestAppContext) -> (Entity<Preview>, &mut gpui::VisualTestContext) {
	cx.update(crate::composer_input::bind_keys);
	cx.add_window_view(|_, cx| {
		let owner = cx.new(AgentSurface::new);
		cx.observe(&owner, |_, _, cx| cx.notify()).detach();
		owner.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let work = s.selected.clone().unwrap();
			let item =
				s.snapshot.as_mut().unwrap().work_items.iter_mut().find(|w| w.id == work).unwrap();
			item.parent_goal_id = None;
			item.codex_thread_id = Some("parent".into());
			item.active_turn_id = None;
			s.composer.update(cx, |input, cx| input.set_content("Keep my draft.", cx));
		});
		Preview(owner)
	})
}

fn draw(visual: &mut gpui::VisualTestContext) {
	visual.run_until_parked();
	visual.update(|window, cx| {
		window.resize(gpui::size(gpui::px(700.), gpui::px(600.)));
		window.draw(cx).clear(cx);
	});
}

#[gpui::test]
fn direct_question_opens_a_popover_without_moving_the_transcript_or_sending_the_draft(
	cx: &mut TestAppContext,
) {
	let (view, visual) = preview(cx);
	draw(visual);
	let owner = view.read_with(visual, |view, _| view.0.clone());
	let button = visual.debug_bounds("ask-separately").expect("direct toolbar entry");
	visual.simulate_click(button.center(), Default::default());
	draw(visual);
	assert!(visual.debug_bounds("sample-popover").is_some());
	owner.update(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Keep my draft.");
		assert!(!s.sending);
		assert!(s.timeline.navigation.is_none());
	});
	let focus = owner.read_with(visual, |s, cx| s.sample.input.as_ref().unwrap().focus_handle(cx));
	visual.update(|window, cx| window.focus(&focus, cx));
	visual.simulate_keystrokes("escape");
	draw(visual);
	assert!(visual.debug_bounds("sample-popover").is_none());
	owner.update(visual, |s, cx| {
		assert!(s.sample.work.is_none());
		assert!(!s.sending && s.escape_stop.is_none());
		assert_eq!(s.composer.read(cx).content(), "Keep my draft.");
	});
}

#[gpui::test]
fn response_question_quotes_the_clicked_answer_and_never_adds_an_inline_panel(
	cx: &mut TestAppContext,
) {
	let (view, visual) = preview(cx);
	draw(visual);
	let button = visual.debug_bounds("ask-response-preview").expect("answer action");
	visual.simulate_mouse_move(button.center(), MouseButton::Left, Default::default());
	visual.simulate_click(button.center(), Default::default());
	draw(visual);
	let owner = view.read_with(visual, |view, _| view.0.clone());
	owner.update(visual, |s, cx| {
		assert_eq!(s.sample.reference.as_deref(), Some("Uses **cursor pagination**."));
		assert_eq!(s.composer.read(cx).content(), "Keep my draft.");
		assert!(s.timeline.navigation.is_none());
	});
	assert!(visual.debug_bounds("sample-reference").is_some());
	let height = visual.debug_bounds("sample-popover").unwrap().size.height;
	assert!(height < gpui::px(560.), "question fits the window");
}

#[gpui::test]
fn selected_text_opens_a_question_with_only_the_selected_excerpt(cx: &mut TestAppContext) {
	let (view, visual) = preview(cx);
	draw(visual);
	let text = visual.debug_bounds("quick-ask-text-0").expect("selectable answer");
	let start = gpui::point(text.left() + gpui::px(1.), text.top() + gpui::px(8.));
	let end = gpui::point(text.right() - gpui::px(1.), text.top() + gpui::px(8.));
	visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
	visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
	visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
	draw(visual);
	let ask = visual.debug_bounds("ask-selected-text").expect("selection action");
	visual.simulate_mouse_down(ask.center(), MouseButton::Left, Default::default());
	visual.simulate_mouse_up(ask.center(), MouseButton::Left, Default::default());
	draw(visual);
	let owner = view.read_with(visual, |view, _| view.0.clone());
	owner.update(visual, |s, cx| {
		let quote = s.sample.reference.as_deref().unwrap();
		assert!(quote.contains("cursor pagination"));
		assert!(!quote.contains("**"), "selection preserves displayed text");
		assert_eq!(s.composer.read(cx).content(), "Keep my draft.");
	});
}

#[gpui::test]
fn retained_question_action_rejects_a_changed_source_and_bounds_unicode_excerpts(
	cx: &mut TestAppContext,
) {
	let (view, visual) = preview(cx);
	let owner = view.read_with(visual, |view, _| view.0.clone());
	let action = owner.update(visual, |s, cx| s.sample_selection_action(cx).unwrap());
	visual.update(|window, cx| action("中文".repeat(1000), window, cx));
	owner.update(visual, |s, _| {
		let reference = s.sample.reference.as_deref().unwrap();
		assert!(reference.ends_with("… (excerpt)"));
		assert!(reference.len() < 2050);
		s.reset_sample();
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| Some(&w.id) == s.selected.as_ref())
			.unwrap()
			.codex_thread_id = Some("replacement".into());
	});
	visual.update(|window, cx| action("Old selection".into(), window, cx));
	assert!(owner.read_with(visual, |s, _| s.sample.work.is_none()));
}

#[gpui::test]
fn manual_question_ignores_cached_next_message_suggestions(cx: &mut TestAppContext) {
	let (view, visual) = preview(cx);
	let owner = view.read_with(visual, |view, _| view.0.clone());
	owner.update(visual, |s, cx| {
		s.open_sample(cx);
		s.sample.state = Some(AgentSampleStatus {
			work_id: EntityId::new(s.selected.clone().unwrap()).unwrap(),
			thread_id: Some(WireText::new("parent").unwrap()),
			request_id: Some(WireText::new("suggestion").unwrap()),
			prompt: Some(WireText::new(NEXT_PROMPT).unwrap()),
			phase: AgentSamplePhase::Ready,
			answer: Some(decodex_protocol::AgentSampleAnswer {
				source_turn_id: WireText::new("turn").unwrap(),
				response_id: WireText::new("response").unwrap(),
				text: Some("An unrelated suggested prompt.".into()),
				input_tokens: None,
				cached_input_tokens: None,
				output_tokens: None,
			}),
		});
	});
	draw(visual);
	assert!(visual.debug_bounds("sample-answer").is_none());
	owner.update(visual, |s, cx| {
		s.sample.state.as_mut().unwrap().prompt =
			Some(WireText::new("A separate question").unwrap());
		cx.notify();
	});
	draw(visual);
	assert!(visual.debug_bounds("sample-answer").is_some());
}

#[gpui::test]
fn quoted_question_sends_one_sample_and_reads_back_the_same_answer(cx: &mut TestAppContext) {
	cx.background_executor.allow_parking();
	let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("sample command")
		};
		let ClientMessage::Command(command) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("sample command")
		};
		let CommandPayload::Agent { action } = command.payload else { panic!("agent command") };
		let AgentActionDto::GenerateSample { work_id, thread_id, prompt } = *action else {
			panic!("must not submit the main draft")
		};
		assert_eq!(thread_id.as_str(), "parent");
		assert!(prompt.as_str().contains("<reference>\nUses cursor pagination.\n</reference>"));
		assert!(prompt.as_str().ends_with("Question:\nWhy not offsets?"));
		assert!(!prompt.as_str().contains("Keep my draft."));
		socket.close(None).await.unwrap();
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("readback") };
		let ClientMessage::Query(query) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("must not regenerate")
		};
		assert!(matches!(query.payload, QueryPayload::GetAgentSample { .. }));
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentSample(AgentSampleStatus {
				work_id,
				thread_id: Some(thread_id),
				request_id: Some(WireText::new(command.idempotency_key.as_str()).unwrap()),
				prompt: Some(prompt),
				phase: AgentSamplePhase::Ready,
				answer: Some(decodex_protocol::AgentSampleAnswer {
					source_turn_id: WireText::new("turn").unwrap(),
					response_id: WireText::new("response").unwrap(),
					text: Some("Cursors keep a stable position.".into()),
					input_tokens: None,
					cached_input_tokens: None,
					output_tokens: None,
				}),
			}),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	});
	let (view, visual) = preview(cx);
	let owner = view.read_with(visual, |view, _| view.0.clone());
	let action = owner.update(visual, |s, cx| s.sample_selection_action(cx).unwrap());
	visual.update(|window, cx| action("Uses cursor pagination.".into(), window, cx));
	owner.update(visual, |s, cx| {
		s.profile = Some(profile);
		s.sample
			.input
			.as_ref()
			.unwrap()
			.update(cx, |input, cx| input.set_content("Why not offsets?", cx));
	});
	draw(visual);
	visual.simulate_keystrokes("enter");
	visual.run_until_parked();
	server.join().unwrap();
	// The fixture replies on an OS thread; drain its delivered UI update before rendering.
	for _ in 0..200 {
		visual.run_until_parked();
		if owner.read_with(visual, |s, _| s.sample.state.is_some()) {
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(5));
	}
	draw(visual);
	owner.update(visual, |s, _| {
		assert_eq!(
			s.sample.state.as_ref().map(|state| state.phase),
			Some(AgentSamplePhase::Ready),
			"{}",
			s.sample.feedback
		);
	});
	assert!(visual.debug_bounds("sample-answer").is_some());
	owner.update(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Keep my draft.");
		assert!(!s.sending && !s.uncertain);
	});
}
