//! Nearby native item history in the existing evidence detail area.
use gpui::AnyElement;
use tokio::runtime::Builder;

use crate::shell::agent_surface::{
	AgentClient, AgentSurface, Context, EntityId, IntoElement, ParentElement, Styled, Task,
	WireText, markdown,
};
use decodex_protocol::{AgentTimelineContent, ItemContextRequest, ItemContextResult};

#[derive(Default)]
pub(super) struct Panel {
	request: Option<ItemContextRequest>,
	result: Option<ItemContextResult>,
	task: Option<Task<()>>,
	epoch: u64,
}

impl AgentSurface {
	pub(super) fn reset_item_context(&mut self) {
		self.nearby_history =
			Panel { epoch: self.nearby_history.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn item_context_panel(
		&self,
		ids: &(String, String, String),
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Some(thread) = self
			.conversation_work()
			.filter(|work| work.id == ids.0)
			.and_then(|work| work.codex_thread_id)
		else {
			return gpui::div().into_any_element();
		};
		let request = ItemContextRequest {
			work_id: EntityId::new(&ids.0).expect("work"),
			thread_id: EntityId::new(thread).expect("thread"),
			turn_id: WireText::new(&ids.1).expect("native turn"),
			item_id: WireText::new(&ids.2).expect("native item"),
			cursor: None,
			before: false,
		};
		let open = self.nearby_history.request.as_ref().is_some_and(|current| {
			current.work_id == request.work_id
				&& current.thread_id == request.thread_id
				&& current.turn_id == request.turn_id
				&& current.item_id == request.item_id
		});
		let initial = request.clone();
		let action = self.workspace_action(
			format!("item-context-{}", ids.2),
			if open { "Close nearby history" } else { "Nearby history" }.into(),
			move |s, cx| {
				if open {
					s.reset_item_context();
					cx.notify();
				} else {
					s.load_item_context(initial.clone(), cx);
				}
			},
			cx,
		);
		let mut panel = gpui::div().flex().flex_col().gap_2().child(action);
		if !open {
			return panel.into_any_element();
		}
		match &self.nearby_history.result {
			Some(ItemContextResult::Available { items, previous_cursor, next_cursor, .. }) => {
				panel = panel.child(
					"Nearby items in this turn · earlier and later pages keep the exact source",
				);
				for (index, item) in items.iter().enumerate() {
					if let AgentTimelineContent::Item {
						item_id,
						kind,
						text,
						truncated,
						activity,
						..
					} = item
					{
						let label = match kind.as_str() {
							"userMessage" => "You",
							"agentMessage" => "Assistant",
							_ => activity
								.as_ref()
								.map_or(kind.as_str(), |activity| activity.label.as_str()),
						};
						panel = panel.child(
							gpui::div()
								.flex()
								.flex_col()
								.gap_1()
								.child(format!(
									"{label}{}",
									if item_id == request.item_id.as_str() {
										" · Selected item"
									} else {
										""
									}
								))
								.child(markdown::render(
									text,
									&format!("item-context-text-{index}"),
								))
								.children(truncated.then(|| {
									crate::shell::agent_surface::muted("Retained text shortened.")
								})),
						);
					}
				}
				for (before, cursor, label) in
					[(true, previous_cursor, "Earlier items"), (false, next_cursor, "Later items")]
				{
					if let Some(cursor) = cursor {
						let mut next = request.clone();
						next.before = before;
						next.cursor = Some(cursor.clone());
						panel = panel.child(self.workspace_action(
							format!("item-context-page-{before}"),
							label.into(),
							move |s, cx| s.load_item_context(next.clone(), cx),
							cx,
						));
					}
				}
			},
			Some(ItemContextResult::Unsupported) =>
				panel = panel.child("This native server does not support item anchors."),
			Some(ItemContextResult::Unavailable) =>
				panel = panel.child("Nearby history unavailable. Reopen to retry."),
			None => panel = panel.child(crate::ui_loading::loading("Loading nearby history")),
		}
		panel.into_any_element()
	}

	fn load_item_context(&mut self, request: ItemContextRequest, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let source = self.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.clone());
		if source.is_none() || self.selected.as_deref() != Some(request.work_id.as_str()) {
			return;
		}
		self.reset_item_context();
		let epoch = self.nearby_history.epoch;
		self.nearby_history.request = Some(request.clone());
		let expected = request.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(AgentClient::new(profile).item_context(request)).ok()
		});
		self.nearby_history.task = Some(cx.spawn(async move |s, cx| {
			let result = future.await.unwrap_or(ItemContextResult::Unavailable);
			let _ = s.update(cx, |s, cx| {
				if s.selected.as_deref() != Some(expected.work_id.as_str())
					|| s.nearby_history.epoch != epoch
					|| s.nearby_history.request.as_ref() != Some(&expected)
					|| s.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.as_ref())
						!= source.as_ref()
				{
					return;
				}
				s.nearby_history.task = None;
				s.nearby_history.result = Some(result);
				cx.notify();
			});
		}));
		cx.notify();
	}
}

#[cfg(test)]
mod tests {
	use super::{AgentSurface, EntityId, ItemContextRequest, ItemContextResult, WireText};
	use crate::shell::agent_surface::wire_test_support;
	use decodex_protocol::{ClientMessage, QueryPayload, QueryResultPayload};
	use futures_util::StreamExt as _;
	use gpui::AppContext as _;
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

	#[gpui::test]
	fn nearby_history_action_reads_the_selected_native_child(cx: &mut gpui::TestAppContext) {
		let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("query")
			};
			let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
				panic!("query")
			};
			let QueryPayload::GetItemContext { request } = query.payload else { panic!("context") };
			assert_eq!(request.thread_id.as_str(), "native-child");
			wire_test_support::answer_query(
				&mut socket,
				query.query_id,
				QueryResultPayload::ItemContext(ItemContextResult::Available {
					request,
					items: vec![],
					previous_cursor: None,
					next_cursor: None,
				}),
			)
			.await;
		});
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.profile = Some(profile);
				s.snapshot.as_mut().unwrap().runtime_source =
					Some(EntityId::new("source").unwrap());
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
					QueryResultPayload::ItemContext(ItemContextResult::Available {
						request,
						items: vec![],
						previous_cursor: None,
						next_cursor: None,
					}),
				)
				.await;
			});
			let (view, visual) = cx.add_window_view(|_, cx| Quiet(cx.new(AgentSurface::new)));
			let surface = view.read_with(visual, |view, _| view.0.clone());
			surface.update(visual, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.profile = Some(profile);
				s.snapshot.as_mut().unwrap().runtime_source =
					Some(EntityId::new("source").unwrap());
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
					s.snapshot.as_mut().unwrap().runtime_source =
						Some(EntityId::new("other").unwrap());
				}
			});
			visual.run_until_parked();
			server.join().unwrap();
			surface
				.read_with(visual, |s, _| assert_eq!(s.nearby_history.result.is_some(), !changed));
		}
	}
}
