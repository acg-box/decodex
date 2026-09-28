//! Expand exact worker tool evidence in place, without leaving the conversation.
use super::*;
use decodex_protocol::{AgentActivityDetailCursor, AgentActivityDetailResult, AgentActivityDto};

#[derive(Default)]
pub(super) struct ActivityDetailState {
	pub value: Option<(String, Option<AgentActivityDetailResult>)>,
	closing: Option<(String, Option<AgentActivityDetailResult>)>,
	pub revision: u64,
	pub task: Option<Task<()>>,
}

impl AgentSurface {
	pub(super) fn clear_activity_detail(&mut self) {
		self.activity_detail.revision += 1;
		self.activity_detail.value = None;
		self.activity_detail.closing = None;
		self.activity_detail.task = None;
	}

	pub(super) fn activity_detail_key(&self, ids: &(String, String, String)) -> Option<String> {
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
		result: AgentActivityDetailResult,
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
		work: &AgentWorkItemDto,
		item: &AgentActivityDto,
		row: gpui::Div,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if ![
			"commandExecution",
			"fileChange",
			"mcpToolCall",
			"dynamicToolCall",
			"webSearch",
			"functionCallOutput",
			"imageView",
		]
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
			.and_then(|(_, result)| result.as_ref())
			.or_else(|| {
				self.activity_detail
					.closing
					.as_ref()
					.filter(|(id, _)| id == &key)
					.and_then(|(_, result)| result.as_ref())
			});
		let body = match result {
			Some(AgentActivityDetailResult::Available { text, offset, next, .. }) => {
				let first_ids = ids.clone();
				let next_ids = ids.clone();
				div()
					.child(super::selectable_text::SelectableText {
						key: format!("detail-text-{key}-{offset}"),
						text: text.clone(),
						highlights: Vec::new(),
						links: Vec::new(),
					})
					.when(*offset > 0, |d| {
						d.child(self.workspace_action(
							"detail-first".into(),
							"Back to start".into(),
							move |s, cx| s.load_activity_detail(first_ids.clone(), None, cx),
							cx,
						))
					})
					.when_some(next.clone(), |d, cursor| {
						d.child(div().debug_selector(|| "detail-next-action".into()).child(
							self.workspace_action(
								"detail-next".into(),
								"Read next portion".into(),
								move |s, cx| {
									s.load_activity_detail(
										next_ids.clone(),
										Some(cursor.clone()),
										cx,
									)
								},
								cx,
							),
						))
					})
			},
			Some(AgentActivityDetailResult::Unavailable) =>
				div().child("Source details are unavailable. Collapse and reopen to retry."),
			None => div().child(crate::ui_loading::loading("Loading details")),
		};
		let metadata_key = format!("tool-reference-{key}");
		let metadata_open = self.expanded_records.contains(&metadata_key);
		let metadata_toggle = self.workspace_action(
			metadata_key.clone(),
			"Technical details".into(),
			move |s, cx| {
				if !s.expanded_records.remove(&metadata_key) {
					s.expanded_records.insert(metadata_key.clone());
				}
				cx.notify();
			},
			cx,
		);

		div()
			.child(
				row.id(SharedString::from(key.clone()))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Inspect {}", item.label))
					.aria_expanded(expanded)
					.cursor_pointer()
					.hover(|d| d.bg(rgba(crate::ui_theme::HOVER_FILL)))
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
				SharedString::from(format!("worker-tool-detail-{key}")),
				expanded,
				div()
					.id(SharedString::from(format!("detail-scroll-{key}")))
					.flex()
					.flex_col()
					.gap(px(8.))
					.max_h(px(280.))
					.overflow_y_scroll()
					.p(px(10.))
					.rounded(px(7.))
					.bg(rgba(0x10101445))
					.font_family("Menlo")
					.text_size(px(11.5))
					.line_height(px(16.))
					.text_color(rgb(ui_theme::TEXT))
					.child(body)
					.child(metadata_toggle)
					.child(disclosure(
						SharedString::from(format!("tool-reference-body-{key}")),
						metadata_open,
						div().mt(px(6.)).text_color(rgb(ui_theme::TEXT_MUTED)).child(
							super::selectable_text::SelectableText {
								key: format!("detail-metadata-{key}"),
								text: format!(
									"{} · {}\nTurn {}\nCall {}",
									item.kind, item.status, item.turn_id, item.item_id
								),
								highlights: Vec::new(),
								links: Vec::new(),
							},
						),
					)),
			))
			.into_any_element()
	}

	fn toggle_activity_detail(&mut self, ids: (String, String, String), cx: &mut Context<Self>) {
		let Some(key) = self.activity_detail_key(&ids) else {
			return;
		};
		self.activity_detail.revision += 1;
		self.activity_detail.task = None;
		if self.activity_detail.value.as_ref().is_some_and(|(selected, _)| selected == &key) {
			self.activity_detail.closing = self.activity_detail.value.take();
			cx.notify();
			return;
		}
		self.load_activity_detail(ids, None, cx);
	}

	fn load_activity_detail(
		&mut self,
		ids: (String, String, String),
		cursor: Option<AgentActivityDetailCursor>,
		cx: &mut Context<Self>,
	) {
		let Some(key) = self.activity_detail_key(&ids) else {
			return;
		};
		self.activity_detail.revision += 1;
		let revision = self.activity_detail.revision;
		self.activity_detail.task = None;
		if let Some(previous) = self.activity_detail.value.take() {
			self.activity_detail.closing = Some(previous);
		}
		self.activity_detail.value = Some((key.clone(), None));
		let Some(profile) = self.profile.clone() else {
			self.activity_detail.value = Some((key, Some(AgentActivityDetailResult::Unavailable)));
			cx.notify();
			return;
		};
		let expected_ids = ids.clone();
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime
				.block_on(AgentClient::new(profile).activity_detail(
					EntityId::new(ids.0).ok()?,
					WireText::new(ids.1).ok()?,
					WireText::new(ids.2).ok()?,
					cursor,
				))
				.ok()
		});
		self.activity_detail.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(AgentActivityDetailResult::Unavailable);
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
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			let snapshot = AgentSnapshotDto {
				runtime_source: Some(EntityId::new("source").unwrap()),
				workspaces: vec![],
				dependencies: vec![],
				pending_events: vec![],
				work_items: vec![AgentWorkItemDto {
					id: "work".into(),
					parent_goal_id: None,
					kind: decodex_protocol::AgentWorkKindDto::Goal,
					title: "Agent".into(),
					codex_thread_id: Some("thread".into()),
					active_turn_id: None,
					dispatch_state: AgentDispatchStateDto::Idle,
					status: AgentWorkStatusDto::Open,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				}],
			};
			let ids = ("work".into(), "turn".into(), "item".into());
			let result = || AgentActivityDetailResult::Available {
				text: "Passed".into(),
				truncated: false,
				offset: 0,
				next: None,
			};
			for change in
				["none", "source", "thread", "reopen", "disconnect", "unavailable", "profile"]
			{
				s.apply_result(Ok(AgentSnapshotResult::Available(snapshot.clone())));
				let key = s.activity_detail_key(&ids).unwrap();
				let revision = s.activity_detail.revision;
				s.activity_detail.value = Some((key.clone(), None));
				match change {
					"source" => {
						let mut replacement = snapshot.clone();
						replacement.runtime_source = Some(EntityId::new("replacement").unwrap());
						s.apply_result(Ok(AgentSnapshotResult::Available(replacement)));
						assert!(s.activity_detail.value.is_none());
					},
					"thread" => {
						let mut replacement = snapshot.clone();
						replacement.work_items[0].codex_thread_id = Some("replacement".into());
						s.apply_result(Ok(AgentSnapshotResult::Available(replacement)));
						assert!(s.activity_detail.value.is_none());
						s.apply_result(Ok(AgentSnapshotResult::Available(snapshot.clone())));
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
						s.apply_result(Ok(AgentSnapshotResult::Unavailable));
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
            s.apply_result(Ok(AgentSnapshotResult::Available(snapshot)));
            let key = s.activity_detail_key(&ids).unwrap();
            s.activity_detail.value = Some((key.clone(),Some(result())));
            s.toggle_activity_detail(ids,cx);
            assert!(s.activity_detail.value.is_none());
            assert!(matches!(&s.activity_detail.closing, Some((id,Some(AgentActivityDetailResult::Available {text,..}))) if id == &key && text == "Passed"));
            s.clear_activity_detail();
            assert!(s.activity_detail.closing.is_none());

		});
	}
}

#[cfg(test)]
#[path = "agent_detail_wire_tests.rs"]
mod wire_tests;
