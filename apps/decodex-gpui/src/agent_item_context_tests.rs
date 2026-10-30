//! Native item navigation through the rendered conversation and public query.
use super::{AgentSurface, EntityId, ItemContextRequest, ItemContextResult, WireText};
use crate::shell::agent_surface::wire_test_support;
use decodex_protocol::{ClientMessage, QueryPayload, QueryResultPayload};
use futures_util::StreamExt as _;
use gpui::{
	AppContext as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
	StatefulInteractiveElement as _, Styled as _,
};
use tokio_tungstenite::tungstenite::Message;

struct Quiet(gpui::Entity<AgentSurface>);
impl gpui::Render for Quiet {
	fn render(
		&mut self,
		_: &mut gpui::Window,
		_: &mut gpui::Context<Self>,
	) -> impl gpui::IntoElement {
		gpui::div()
	}
}
struct AnchorView(gpui::Entity<AgentSurface>);
impl gpui::Render for AnchorView {
	fn render(
		&mut self,
		_: &mut gpui::Window,
		cx: &mut gpui::Context<Self>,
	) -> impl gpui::IntoElement {
		self.0.update(cx, |s, cx| {
			s.item_context_panel(&("agent".into(), "turn".into(), "item".into()), cx)
		})
	}
}

struct HistoryView(gpui::Entity<AgentSurface>);
impl gpui::Render for HistoryView {
	fn render(
		&mut self,
		_: &mut gpui::Window,
		cx: &mut gpui::Context<Self>,
	) -> impl gpui::IntoElement {
		self.0.update(cx, |s, cx| {
			let work = s.conversation_work().unwrap();
			let scroll = s.timeline.scroll.entry(work.id.clone()).or_default().clone();
			let body =
				gpui::div().min_h(gpui::px(1_200.)).w_full().child(s.history_panel(&work, cx));
			let content = s.anchored_native_content(body.into_any_element());
			let transcript = gpui::div()
				.id("item-history-test-scroll")
				.h(gpui::px(600.))
				.w_full()
				.overflow_y_scroll()
				.track_scroll(&scroll)
				.child(content);
			gpui::div()
				.size_full()
				.flex()
				.flex_col()
				.children(s.item_context_navigation(cx))
				.child(s.anchored_native_viewport(transcript.into_any_element(), scroll))
				.into_any_element()
		})
	}
}

fn message(turn: &str, id: &str) -> decodex_protocol::AgentTimelineContent {
	decodex_protocol::AgentTimelineContent::Item {
		turn_id: turn.into(),
		item_id: id.into(),
		kind: "agentMessage".into(),
		text: format!("Message {id}"),
		truncated: false,
		phase: None,
		app_ui: false,
		activity: None,
		attachments: vec![],
		collaboration: None,
	}
}

fn request(item: &str) -> ItemContextRequest {
	ItemContextRequest {
		work_id: EntityId::new("agent").unwrap(),
		thread_id: EntityId::new("thread").unwrap(),
		turn_id: WireText::new("turn").unwrap(),
		item_id: WireText::new(item).unwrap(),
		cursor: None,
		before: false,
	}
}

fn prepare(s: &mut AgentSurface, cx: &mut gpui::Context<AgentSurface>) {
	s.visual_workspace_fixture(cx);
	s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
	let snapshot = s.snapshot.as_mut().unwrap();
	snapshot.runtime_source = Some(EntityId::new("source").unwrap());
	let work = snapshot.work_items.iter_mut().find(|work| work.id == "agent").unwrap();
	work.codex_thread_id = Some("thread".into());
	s.timeline.scroll.entry("agent".into()).or_default();
	s.timeline.native.binding = Some(crate::shell::agent_surface::native_timeline::Binding {
		work: "agent".into(),
		thread: "thread".into(),
		account: "account".into(),
	});
	// The same item ID in another turn must not satisfy an exact-item location.
	s.timeline.native.entries = vec![decodex_protocol::AgentTimelineEntry {
		position: 100,
		content: message("recent-turn", "item"),
	}];
}

fn answer(request: ItemContextRequest, ids: &[&str], next: Option<&str>) -> ItemContextResult {
	ItemContextResult::Available {
		items: ids.iter().map(|id| message(request.turn_id.as_str(), id)).collect(),
		request,
		previous_cursor: Some(WireText::new("earlier").unwrap()),
		next_cursor: next.map(|v| WireText::new(v).unwrap()),
	}
}

#[gpui::test]
fn located_loaded_item_reveals_its_fold_without_a_native_read(cx: &mut gpui::TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, cx| {
			prepare(s, cx);
			let mut content = message("turn", "reasoning");
			if let decodex_protocol::AgentTimelineContent::Item { kind, .. } = &mut content {
				*kind = "reasoning".into();
			}
			s.timeline.native.entries = vec![
				decodex_protocol::AgentTimelineEntry { position: 1, content },
				decodex_protocol::AgentTimelineEntry {
					position: 2,
					content: message("turn", "final"),
				},
				decodex_protocol::AgentTimelineEntry {
					position: 3,
					content: decodex_protocol::AgentTimelineContent::TurnBoundary {
						turn_id: "turn".into(),
						completed: true,
						status: Some("completed".into()),
						duration_ms: None,
						usage: None,
						timing: None,
						usage_summary: None,
						error: None,
					},
				},
			];
		});
		HistoryView(surface)
	});
	visual.update(crate::ui_frame::draw_frame);
	view.update(visual, |v, cx| {
		v.0.update(cx, |s, cx| {
			assert!(s.profile.is_none());
			s.locate_item_context(request("reasoning"), cx);
			assert!(s.nearby_history.task.is_none());
			assert!(!s.item_context_window_active());
			assert_eq!(s.timeline.native.entries.len(), 3);
		})
	});
	visual.update(crate::ui_frame::draw_frame);
	assert!(visual.debug_bounds("conversation-located-item").is_some());
	assert!(visual.debug_bounds("conversation-item-history").is_none());
	view.read_with(visual, |v, cx| {
		v.0.read_with(cx, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		})
	});
}

#[gpui::test]
fn unloaded_item_opens_in_the_main_transcript_and_returns_to_the_saved_position(
	cx: &mut gpui::TestAppContext,
) {
	let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("query");
		};
		let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
			panic!("query");
		};
		let QueryPayload::GetItemContext { request } = query.payload else {
			panic!("context");
		};
		assert_eq!(request.thread_id.as_str(), "thread");
		assert_eq!(request.turn_id.as_str(), "turn");
		assert_eq!(request.item_id.as_str(), "item");
		wire_test_support::answer_query(
			&mut socket,
			query.query_id,
			QueryResultPayload::ItemContext(answer(
				request,
				&["before", "item", "after"],
				Some("later"),
			)),
		)
		.await;
	});
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, cx| {
			prepare(s, cx);
			s.profile = Some(profile);
		});
		HistoryView(surface)
	});
	visual.update(crate::ui_frame::draw_frame);
	view.update(visual, |v, cx| {
		v.0.update(cx, |s, cx| {
			s.timeline.scroll["agent"].set_offset(gpui::point(gpui::px(0.), gpui::px(-120.)));
			s.locate_item_context(request("item"), cx);
		})
	});
	visual.run_until_parked();
	server.join().unwrap();
	visual.update(crate::ui_frame::draw_frame);
	assert!(visual.debug_bounds("conversation-item-history").is_some());
	assert!(visual.debug_bounds("conversation-located-item").is_some());
	assert!(visual.debug_bounds("item-context-item").is_none());
	view.update(visual, |v, cx| v.0.update(cx, |s, cx| s.refresh_open_native_history(cx)));
	view.read_with(visual, |v, cx| {
		v.0.read_with(cx, |s, _| {
			assert_eq!(s.timeline.native.entries[0].content, message("recent-turn", "item"));
			assert!(!s.history_viewport_fill_ready());
		})
	});
	let close = visual.debug_bounds("item-history-return").unwrap();
	visual.simulate_click(close.center(), Default::default());
	view.read_with(visual, |v, cx| {
		v.0.read_with(cx, |s, cx| {
			assert!(!s.item_context_window_active());
			assert_eq!(s.timeline.scroll["agent"].offset().y, gpui::px(-120.));
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		})
	});
}

#[gpui::test]
fn item_history_pages_use_native_cursors_and_keep_the_visible_page_on_failure(
	cx: &mut gpui::TestAppContext,
) {
	let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
		for page in 0..3 {
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("query");
			};
			let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
				panic!("query");
			};
			let QueryPayload::GetItemContext { request } = query.payload else {
				panic!("context");
			};
			assert!(!request.before);
			assert_eq!(
				request.cursor.as_ref().map(WireText::as_str),
				[None, Some("later"), Some("later-2")][page]
			);
			let result = match page {
				0 => answer(request, &["item", "after"], Some("later")),
				1 => answer(request, &["next-1", "next-2"], Some("later-2")),
				_ => ItemContextResult::Unavailable,
			};
			wire_test_support::answer_query(
				&mut socket,
				query.query_id,
				QueryResultPayload::ItemContext(result),
			)
			.await;
		}
	});
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, cx| {
			prepare(s, cx);
			s.profile = Some(profile);
			s.locate_item_context(request("item"), cx);
		});
		HistoryView(surface)
	});
	visual.run_until_parked();
	for _ in 0..2 {
		visual.update(crate::ui_frame::draw_frame);
		let next = visual.debug_bounds("item-context-page-false").unwrap();
		visual.simulate_click(next.center(), Default::default());
		visual.run_until_parked();
	}
	server.join().unwrap();
	view.read_with(visual, |v, cx| {
		v.0.read_with(cx, |s, _| {
			assert!(
				matches!(&s.nearby_history.result, Some(ItemContextResult::Available { items, .. })
			if items == &vec![message("turn", "next-1"), message("turn", "next-2")])
			);
			assert!(s.feedback.message.contains("unchanged"));
			assert_eq!(s.timeline.native.entries.len(), 1);
		})
	});
}

#[gpui::test]
fn known_message_actions_stay_out_of_the_transcript(cx: &mut gpui::TestAppContext) {
	struct Transcript(gpui::Entity<AgentSurface>);
	impl gpui::Render for Transcript {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			cx: &mut gpui::Context<Self>,
		) -> impl gpui::IntoElement {
			self.0.update(cx, |s, cx| {
				let work = s.conversation_work().unwrap();
				s.native_timeline_panel(&work, cx).unwrap()
			})
		}
	}
	let (_, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| work.id == "agent")
				.unwrap();
			work.codex_thread_id = Some("thread".into());
			s.timeline.native.binding =
				Some(crate::shell::agent_surface::native_timeline::Binding {
					work: "agent".into(),
					thread: "thread".into(),
					account: "account".into(),
				});
			s.timeline.native.entries = vec![decodex_protocol::AgentTimelineEntry {
				position: 1,
				content: decodex_protocol::AgentTimelineContent::Item {
					turn_id: "turn".into(),
					item_id: "item".into(),
					kind: "userMessage".into(),
					text: "A retained message".into(),
					truncated: false,
					phase: None,
					app_ui: false,
					activity: None,
					attachments: vec![],
					collaboration: None,
				},
			}];
		});
		Transcript(surface)
	});
	visual.update(crate::ui_frame::draw_frame);
	assert!(
		visual.debug_bounds("item-context-item").is_none(),
		"Ordinary messages must not show a redundant nearby-history action"
	);
}

#[gpui::test]
fn nearby_history_action_reads_the_selected_native_child(cx: &mut gpui::TestAppContext) {
	let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("query") };
		let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
			panic!("query")
		};
		let QueryPayload::GetItemContext { request } = query.payload else { panic!("context") };
		assert_eq!(request.thread_id.as_str(), "native-child");
		wire_test_support::answer_query(
			&mut socket,
			query.query_id,
			QueryResultPayload::ItemContext(answer(request, &["item"], None)),
		)
		.await;
	});
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.profile = Some(profile);
			s.snapshot.as_mut().unwrap().runtime_source = Some(EntityId::new("source").unwrap());
			s.native_agents.selected = Some(("agent".into(), "native-child".into()));
		});
		AnchorView(surface)
	});
	visual.update(crate::ui_frame::draw_frame);
	let button = visual.debug_bounds("item-context-item").expect("nearby history action");
	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	server.join().unwrap();
	view.read_with(visual, |v, cx| {
		v.0.read_with(cx, |s, _| assert!(s.nearby_history.result.is_some()))
	});
}

#[gpui::test]
fn nearby_history_query_keeps_identity_and_discards_a_changed_source(
	cx: &mut gpui::TestAppContext,
) {
	for changed in [false, true] {
		let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("query")
			};
			let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
				panic!("query")
			};
			let QueryPayload::GetItemContext { request } = query.payload else {
				panic!("item context")
			};
			assert_eq!(request.turn_id.as_str(), "turn");
			assert_eq!(request.item_id.as_str(), "item");
			wire_test_support::answer_query(
				&mut socket,
				query.query_id,
				QueryResultPayload::ItemContext(answer(request, &["item"], None)),
			)
			.await;
		});
		let (view, visual) = cx.add_window_view(|_, cx| Quiet(cx.new(AgentSurface::new)));
		let surface = view.read_with(visual, |view, _| view.0.clone());
		surface.update(visual, |s, cx| {
			prepare(s, cx);
			s.profile = Some(profile);
			s.snapshot.as_mut().unwrap().runtime_source = Some(EntityId::new("source").unwrap());
			s.load_item_context(
				ItemContextRequest {
					work_id: EntityId::new("agent").unwrap(),
					thread_id: EntityId::new("thread").unwrap(),
					turn_id: WireText::new("turn").unwrap(),
					item_id: WireText::new("item").unwrap(),
					cursor: None,
					before: false,
				},
				cx,
			);
			if changed {
				s.snapshot.as_mut().unwrap().runtime_source = Some(EntityId::new("other").unwrap());
			}
		});
		visual.run_until_parked();
		server.join().unwrap();
		surface.read_with(visual, |s, _| assert_eq!(s.nearby_history.result.is_some(), !changed));
	}
}
