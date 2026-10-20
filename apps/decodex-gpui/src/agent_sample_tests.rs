//! Separate answer controls must not submit or replace the ordinary conversation draft.
use gpui::{Focusable as _, Render, TestAppContext, Window};

use super::*;
use decodex_protocol::AgentSampleAnswer;

struct SamplePanel(Entity<AgentSurface>);
impl Render for SamplePanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| s.sample_panel(&s.selected.clone().unwrap(), cx))
	}
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
	visual.update(|window, cx| {
		window.draw(cx).clear(cx);
	});
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
	use crate::shell::agent_surface::wire_test_support;
	use decodex_protocol::{
		CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope,
		QueryResultPayload, ServerId, ServerMessage,
	};
	use futures_util::{SinkExt as _, StreamExt as _};
	use tokio_tungstenite::tungstenite::Message;
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
		let decodex_protocol::AgentActionDto::GenerateSample { work_id, thread_id, prompt } =
			*action
		else {
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
				server_id: ServerId::new(wire_test_support::SERVER).unwrap(),
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
	let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
	runtime.block_on(async {
		let (_closed, closure) = watch::channel(false);
		let (updates, results) = watch::channel(None);
		request::run(
			profile,
			EntityId::new("root").unwrap(),
			WireText::new("parent").unwrap(),
			Some(WireText::new("Another approach?").unwrap()),
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
