//! Expand exact worker tool evidence in place, without leaving the conversation.
use super::*;
use decodex_protocol::{ChiefActivityDetailResult, ChiefActivityDto};

#[derive(Default)]
pub(super) struct ActivityDetailState {
	pub value: Option<(String, Option<ChiefActivityDetailResult>)>,
	pub revision: u64,
	pub task: Option<Task<()>>,
}

impl ChiefSurface {
	pub(super) fn clear_activity_detail(&mut self) {
		self.activity_detail.revision += 1;
		self.activity_detail.value = None;
		self.activity_detail.task = None;
	}

	fn activity_detail_key(&self, ids: &(String, String, String)) -> Option<String> {
		if self.state != LoadState::Ready
			&& !(self.state == LoadState::Loading && self.status_before_refresh.is_none())
		{
			return None;
		}
		let snapshot = self.snapshot.as_ref()?;
		let source = snapshot.runtime_source.as_ref()?;
		let work = snapshot.work_items.iter().find(|work| work.id == ids.0)?;
		let thread = work.codex_thread_id.as_ref()?;
		Some(serde_json::json!([ids, thread, source]).to_string())
	}

	fn accept_activity_detail(
		&mut self,
		ids: &(String, String, String),
		key: String,
		revision: u64,
		result: ChiefActivityDetailResult,
	) -> bool {
		if self.activity_detail.revision != revision
			|| self.activity_detail_key(ids).as_ref() != Some(&key)
			|| !self.activity_detail.value.as_ref().is_some_and(|(current, _)| current == &key)
		{
			return false;
		}
		self.activity_detail.value = Some((key, Some(result)));
		self.activity_detail.task = None;
		true
	}

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
		let Some(key) = self.activity_detail_key(&ids) else {
			return row.into_any_element();
		};
		let expanded = self.activity_detail.value.as_ref().is_some_and(|(id, _)| id == &key);
		let click = ids.clone();
		let result = self
			.activity_detail
			.value
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
		let Some(key) = self.activity_detail_key(&ids) else {
			return;
		};
		self.activity_detail.revision += 1;
		let revision = self.activity_detail.revision;
		self.activity_detail.task = None;
		if self.activity_detail.value.as_ref().is_some_and(|(selected, _)| selected == &key) {
			self.activity_detail.value = None;
			cx.notify();
			return;
		}
		self.activity_detail.value = Some((key.clone(), None));
		let Some(profile) = self.profile.clone() else {
			self.activity_detail.value = Some((key, Some(ChiefActivityDetailResult::Unavailable)));
			cx.notify();
			return;
		};
		let expected_ids = ids.clone();
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
		self.activity_detail.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(ChiefActivityDetailResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.accept_activity_detail(&expected_ids, key, revision, result) {
					cx.notify();
				}
			});
		}));
		cx.notify();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[gpui::test]
	fn activity_details_reject_replaced_sources_and_reopened_requests(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			let snapshot = ChiefSnapshotDto {
				runtime_source: Some(EntityId::new("source").unwrap()),
				workspaces: vec![],
				dependencies: vec![],
				pending_events: vec![],
				work_items: vec![ChiefWorkItemDto {
					id: "work".into(),
					parent_goal_id: None,
					kind: decodex_protocol::ChiefWorkKindDto::Goal,
					title: "Chief".into(),
					codex_thread_id: Some("thread".into()),
					active_turn_id: None,
					dispatch_state: ChiefDispatchStateDto::Idle,
					status: ChiefWorkStatusDto::Open,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				}],
			};
			let ids = ("work".into(), "turn".into(), "item".into());
			let result =
				|| ChiefActivityDetailResult::Available { text: "Passed".into(), truncated: false };
			for change in
				["none", "source", "thread", "reopen", "disconnect", "unavailable", "profile"]
			{
				s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot.clone())));
				let key = s.activity_detail_key(&ids).unwrap();
				let revision = s.activity_detail.revision;
				s.activity_detail.value = Some((key.clone(), None));
				match change {
					"source" => {
						let mut replacement = snapshot.clone();
						replacement.runtime_source = Some(EntityId::new("replacement").unwrap());
						s.apply_result(Ok(ChiefSnapshotResult::Available(replacement)));
						assert!(s.activity_detail.value.is_none());
					},
					"thread" => {
						let mut replacement = snapshot.clone();
						replacement.work_items[0].codex_thread_id = Some("replacement".into());
						s.apply_result(Ok(ChiefSnapshotResult::Available(replacement)));
						assert!(s.activity_detail.value.is_none());
						s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot.clone())));
					},
					"reopen" => {
						s.clear_activity_detail();
						s.activity_detail.value = Some((key.clone(), None));
					},
					"disconnect" => {
						s.mark_stale(cx);
						assert!(s.activity_detail.value.is_none());
					},
					"unavailable" => {
						s.apply_result(Ok(ChiefSnapshotResult::Unavailable));
						assert!(s.activity_detail.value.is_none());
					},
					"profile" => {
						s.bind_profile(None, cx);
						assert!(s.activity_detail.value.is_none());
					},
					_ => {},
				}
				assert_eq!(
					s.accept_activity_detail(&ids, key, revision, result()),
					change == "none",
					"{change}"
				);
				if change == "reopen" {
					let key = s.activity_detail_key(&ids).unwrap();
					assert!(s.accept_activity_detail(
						&ids,
						key,
						s.activity_detail.revision,
						result()
					));
				}
			}
		});
	}
}
