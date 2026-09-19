//! Expand exact worker tool evidence in place, without leaving the conversation.
use super::*;
use decodex_protocol::{ChiefActivityDetailResult, ChiefActivityDto};

impl ChiefSurface {
	pub(super) fn detail_row(
		&self,
		work: &ChiefWorkItemDto,
		item: &ChiefActivityDto,
		row: gpui::Div,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if work.kind == decodex_protocol::ChiefWorkKindDto::Manager
			|| !["commandExecution", "fileChange", "mcpToolCall", "dynamicToolCall", "webSearch"]
				.contains(&item.kind.as_str())
		{
			return row.into_any_element();
		}
		let ids = (work.id.clone(), item.turn_id.clone(), item.item_id.clone());
		let key = serde_json::json!(ids).to_string();
		let expanded = self.activity_detail.as_ref().is_some_and(|(id, _)| id == &key);
		let click = ids.clone();
		let result = self
			.activity_detail
			.as_ref()
			.filter(|(id, _)| id == &key)
			.and_then(|(_, result)| result.as_ref());
		let body = match result {
			Some(ChiefActivityDetailResult::Available { text, truncated }) =>
				div().child(text.clone()).when(*truncated, |d| d.child(muted("Output shortened"))),
			Some(ChiefActivityDetailResult::Unavailable) =>
				div().child("Source details are unavailable. Collapse and reopen to retry."),
			None => div().child("Loading details…"),
		};
		div()
			.child(
				row.id(SharedString::from(key.clone()))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Inspect {}", item.label))
					.aria_expanded(expanded)
					.cursor_pointer()
					.hover(|d| d.bg(rgba(0xffffff08)))
					.on_click(
						cx.listener(move |s, _, _, cx| s.toggle_activity_detail(click.clone(), cx)),
					)
					.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							s.toggle_activity_detail(ids.clone(), cx);
							cx.stop_propagation();
						}
					}))
					.smooth(),
			)
			.child(disclosure(
				"worker-tool-detail",
				expanded,
				div()
					.id(SharedString::from(format!("detail-scroll-{key}")))
					.max_h(px(280.))
					.overflow_y_scroll()
					.p(px(10.))
					.rounded(px(7.))
					.bg(rgba(0x10101445))
					.font_family("Menlo")
					.text_size(px(10.5))
					.line_height(px(16.))
					.text_color(rgb(ui_theme::TEXT))
					.child(body),
			))
			.into_any_element()
	}

	fn toggle_activity_detail(&mut self, ids: (String, String, String), cx: &mut Context<Self>) {
		let key = serde_json::json!(ids).to_string();
		self.activity_detail_task = None;
		if self.activity_detail.as_ref().is_some_and(|(selected, _)| selected == &key) {
			self.activity_detail = None;
			cx.notify();
			return;
		}
		self.activity_detail = Some((key.clone(), None));
		let Some(profile) = self.profile.clone() else {
			self.activity_detail = Some((key, Some(ChiefActivityDetailResult::Unavailable)));
			cx.notify();
			return;
		};
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime
				.block_on(ChiefClient::new(profile).activity_detail(
					EntityId::new(ids.0).ok()?,
					WireText::new(ids.1).ok()?,
					WireText::new(ids.2).ok()?,
				))
				.ok()
		});
		self.activity_detail_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(ChiefActivityDetailResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.activity_detail.as_ref().is_some_and(|(current, _)| current == &key) {
					s.activity_detail = Some((key, Some(result)));
					s.activity_detail_task = None;
					cx.notify();
				}
			});
		}));
		cx.notify();
	}
}
