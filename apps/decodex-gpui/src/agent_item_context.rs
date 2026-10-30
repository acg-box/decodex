//! Locate native items in the conversation, reading an anchor only when needed.
use gpui::{AnyElement, Div, InteractiveElement as _, Pixels, Point};
use tokio::runtime::Builder;

use crate::shell::agent_surface::{
	AgentClient, AgentSurface, AgentWorkItemDto, Context, EntityId, IntoElement, ParentElement,
	Styled, Task, WireText,
};
use decodex_protocol::{AgentTimelineContent, ItemContextRequest, ItemContextResult};

#[derive(Default)]
pub(super) struct Panel {
	request: Option<ItemContextRequest>,
	result: Option<ItemContextResult>,
	source: Option<EntityId>,
	return_offset: Option<Point<Pixels>>,
	task: Option<Task<()>>,
	epoch: u64,
}

impl AgentSurface {
	pub(super) fn reset_item_context(&mut self) {
		if self.item_context_matches_source()
			&& let Some(offset) = self.nearby_history.return_offset
			&& let Some(scroll) = self.selected.as_ref().and_then(|id| self.timeline.scroll.get(id))
		{
			scroll.set_offset(offset);
			self.clear_item_history_viewport();
		}
		self.nearby_history =
			Panel { epoch: self.nearby_history.epoch.wrapping_add(1), ..Default::default() };
	}

	/// This action belongs to an existing evidence/reference detail, not each message.
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
		self.workspace_action(
			format!("item-context-{}", ids.2),
			"Show in conversation".into(),
			move |s, cx| s.locate_item_context(request.clone(), cx),
			cx,
		)
	}

	fn item_context_matches_source(&self) -> bool {
		self.nearby_history.source.is_some()
			&& self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref())
				== self.nearby_history.source.as_ref()
			&& self.nearby_history.request.as_ref().is_some_and(|request| {
				self.conversation_work().is_some_and(|work| {
					work.id == request.work_id.as_str()
						&& work.codex_thread_id.as_deref() == Some(request.thread_id.as_str())
				})
			})
	}

	pub(super) fn item_context_window_active(&self) -> bool {
		self.item_context_matches_source()
			&& matches!(&self.nearby_history.result,
				Some(ItemContextResult::Available { items, .. }) if !items.is_empty())
	}

	pub(super) fn item_history_location_active(&self) -> bool {
		self.item_context_matches_source()
			&& !matches!(
				self.nearby_history.result,
				Some(ItemContextResult::Unavailable | ItemContextResult::Unsupported)
			)
	}

	pub(super) fn is_located_item(&self, content: &AgentTimelineContent) -> bool {
		self.item_context_matches_source()
			&& matches!(content, AgentTimelineContent::Item { turn_id, item_id, .. }
				if self.nearby_history.request.as_ref().is_some_and(|r|
					r.turn_id.as_str() == turn_id && r.item_id.as_str() == item_id))
	}

	pub(super) fn located_item_row(
		&self,
		content: &AgentTimelineContent,
		body: AnyElement,
	) -> AnyElement {
		if !self.is_located_item(content) {
			return body;
		}
		gpui::div()
			.debug_selector(|| "conversation-located-item".into())
			.w_full()
			.min_w_0()
			.border_l_2()
			.pl_2()
			.border_color(crate::ui_theme::rgb(crate::ui_theme::BLUE))
			.child(body)
			.into_any_element()
	}

	pub(super) fn item_context_history(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Option<Div> {
		if !self.item_context_window_active() {
			return None;
		}
		let ItemContextResult::Available { items, previous_cursor, next_cursor, .. } =
			self.nearby_history.result.as_ref()?
		else {
			return None;
		};
		let mut body = gpui::div()
			.debug_selector(|| "conversation-item-history".into())
			.w_full()
			.min_w_0()
			.flex()
			.flex_col()
			.gap_4();
		body = body.children(self.item_history_page_action(true, previous_cursor.as_ref(), cx));
		for content in items {
			let AgentTimelineContent::Item { turn_id, item_id, .. } = content else {
				continue;
			};
			let identity =
				serde_json::json!([work.id, work.codex_thread_id, turn_id, item_id]).to_string();
			let row = self.native_item_content(work, content, &identity, false, cx);
			let row = self.located_item_row(content, row);
			body = body.child(self.item_history_scroll_row(work, turn_id, item_id, row, cx));
		}
		body = body.children(self.item_history_page_action(false, next_cursor.as_ref(), cx));
		if self.nearby_history.task.is_some() {
			body = body.child(crate::ui_loading::loading("Loading conversation history"));
		}
		Some(body)
	}

	pub(super) fn item_context_navigation(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
		self.item_context_window_active().then(|| {
			gpui::div()
				.flex_none()
				.px(gpui::px(16.))
				.pb(gpui::px(8.))
				.flex()
				.items_center()
				.justify_between()
				.child(crate::shell::agent_surface::muted("Conversation history · This turn"))
				.child(self.workspace_action(
					"item-history-return".into(),
					"Return to conversation".into(),
					|s, cx| {
						s.reset_item_context();
						cx.notify();
					},
					cx,
				))
				.into_any_element()
		})
	}

	fn item_history_page_action(
		&self,
		before: bool,
		cursor: Option<&WireText>,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let mut request = self.nearby_history.request.clone()?;
		request.cursor = Some(cursor?.clone());
		request.before = before;
		Some(self.workspace_action(
			format!("item-context-page-{before}"),
			if before { "Earlier messages" } else { "Later messages" }.into(),
			move |s, cx| s.load_item_context(request.clone(), cx),
			cx,
		))
	}

	fn locate_item_context(&mut self, request: ItemContextRequest, cx: &mut Context<Self>) {
		let in_window = self.item_context_window_active()
			&& matches!(&self.nearby_history.result,
				Some(ItemContextResult::Available { items, .. }) if items.iter().any(|item|
					matches!(item, AgentTimelineContent::Item { turn_id, item_id, .. }
						if turn_id == request.turn_id.as_str() && item_id == request.item_id.as_str())));
		if in_window {
			self.nearby_history.task = None;
			self.nearby_history.epoch = self.nearby_history.epoch.wrapping_add(1);
			self.nearby_history.request = Some(request.clone());
			self.focus_item_history(request.turn_id.as_str(), request.item_id.as_str());
		} else {
			self.reset_item_context();
			self.nearby_history.source =
				self.snapshot.as_ref().and_then(|s| s.runtime_source.clone());
			self.nearby_history.request = Some(request.clone());
			if !self.item_context_matches_source() {
				return;
			}
			if !self.locate_loaded_history_item(&request) {
				self.feedback = crate::ui_feedback::Feedback::neutral("Locating message…");
				self.load_item_context(request, cx);
			}
		}
		self.pause_item_history_navigation();
		self.workspace.details_visible = false;
		cx.notify();
	}

	fn load_item_context(&mut self, request: ItemContextRequest, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let source = self.snapshot.as_ref().and_then(|snapshot| snapshot.runtime_source.clone());
		if source.is_none()
			|| self.selected.as_deref() != Some(request.work_id.as_str())
			|| self.nearby_history.task.is_some()
		{
			return;
		}
		if request.cursor.is_none() {
			self.reset_item_context();
			self.nearby_history.return_offset =
				self.timeline.scroll.get(request.work_id.as_str()).map(gpui::ScrollHandle::offset);
		}
		self.nearby_history.epoch = self.nearby_history.epoch.wrapping_add(1);
		let epoch = self.nearby_history.epoch;
		self.nearby_history.source = source.clone();
		self.nearby_history.request = Some(request.clone());
		let expected = request.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(AgentClient::new(profile).item_context(request)).ok()
		});
		self.nearby_history.task = Some(cx.spawn(async move |s, cx| {
			let result = future.await.unwrap_or(ItemContextResult::Unavailable);
			let _ = s.update(cx, |s, cx| {
				if s.nearby_history.epoch != epoch
					|| s.nearby_history.request.as_ref() != Some(&expected)
					|| !s.item_context_matches_source()
				{
					return;
				}
				s.nearby_history.task = None;
				s.accept_item_history(result, &expected);
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn accept_item_history(&mut self, result: ItemContextResult, request: &ItemContextRequest) {
		if let ItemContextResult::Available { items, .. } = &result
			&& let Some(AgentTimelineContent::Item { turn_id, item_id, .. }) =
				if request.cursor.is_none() {
					items.iter().find(|item| {
						matches!(item,
						AgentTimelineContent::Item { turn_id, item_id, .. }
						if turn_id == request.turn_id.as_str() && item_id == request.item_id.as_str())
					})
				} else if request.before {
					items.last()
				} else {
					items.first()
				} {
			let (turn, item) = (turn_id.clone(), item_id.clone());
			self.nearby_history.result = Some(result);
			self.focus_item_history(&turn, &item);
			self.feedback = crate::ui_feedback::Feedback::neutral("History loaded.");
			return;
		}
		self.feedback = crate::ui_feedback::Feedback::neutral(match result {
			ItemContextResult::Unsupported => "This Codex version cannot locate unloaded messages.",
			_ => "Could not load that history. The current conversation is unchanged.",
		});
		if !self.item_context_window_active() {
			self.nearby_history.result = Some(ItemContextResult::Unavailable);
		}
	}
}

#[cfg(test)]
#[path = "agent_item_context_tests.rs"]
mod tests;
