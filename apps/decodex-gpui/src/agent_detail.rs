//! Expand exact worker tool evidence in place, without leaving the conversation.

use gpui::{AnyElement, Div, KeyDownEvent};
use tokio::runtime::Builder;
use ui_theme::{TEXT, TEXT_MUTED};

#[cfg(test)]
use crate::shell::agent_surface::{
	AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentWorkStatusDto,
	ClientProfile, Entity, LoadState, Render, Window, native_timeline,
};
use crate::{
	shell::agent_surface::{
		AgentClient, AgentSurface, AgentWorkItemDto, Context, EntityId, FluentBuilder,
		InteractiveElement, IntoElement, ParentElement, Role, SharedString, SmoothControl,
		StatefulInteractiveElement, Styled, Task, WireText, selectable_text::SelectableText,
		ui_theme,
	},
	ui_loading, ui_motion,
	ui_theme::HOVER_FILL,
};
use decodex_protocol::{
	AgentActivityDetailCursor, AgentActivityDetailResult, AgentActivityDto, AgentTimelineContent,
};

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
		if !self.command_connection_ready() {
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
		row: Div,
		cx: &mut Context<Self>,
	) -> AnyElement {
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
		let body = self.activity_detail_body(&ids, &key, result, cx);
		let metadata_key = format!("tool-reference-{key}");
		let metadata_open = self.timeline.expanded_records.contains(&metadata_key);
		let metadata_toggle = self.workspace_action(
			metadata_key.clone(),
			"Technical details".into(),
			move |s, cx| {
				if !s.timeline.expanded_records.remove(&metadata_key) {
					s.timeline.expanded_records.insert(metadata_key.clone());
				}

				cx.notify();
			},
			cx,
		);

		gpui::div()
			.w_full()
			.min_w_0()
			.debug_selector(|| "tool-detail-row".into())
			.child(
				row.id(SharedString::from(key.clone()))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Inspect {}", item.label))
					.aria_expanded(expanded)
					.cursor_pointer()
					.hover(|d| d.bg(gpui::rgba(HOVER_FILL)))
					.on_click(
						cx.listener(move |s, _, _, cx| s.toggle_activity_detail(click.clone(), cx)),
					)
					.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							s.toggle_activity_detail(ids.clone(), cx);
							cx.stop_propagation();
						}
					}))
					.smooth(),
			)
			.child(ui_motion::disclosure(
				SharedString::from(format!("worker-tool-detail-{key}")),
				expanded,
				gpui::div()
					.id(SharedString::from(format!("detail-scroll-{key}")))
					.w_full()
					.min_w_0()
					.flex()
					.flex_col()
					.gap(gpui::px(8.))
					.max_h(gpui::px(280.))
					.overflow_y_scroll()
					.p(gpui::px(10.))
					.rounded(gpui::px(7.))
					.bg(gpui::rgba(0x10101445))
					.font_family("Menlo")
					.text_size(gpui::px(11.5))
					.line_height(gpui::px(16.))
					.text_color(gpui::rgb(TEXT))
					.child(body)
					.child(metadata_toggle)
					.child(ui_motion::disclosure(
						SharedString::from(format!("tool-reference-body-{key}")),
						metadata_open,
						gpui::div().mt(gpui::px(6.)).text_color(gpui::rgb(TEXT_MUTED)).child(
							SelectableText {
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

	fn activity_detail_body(
		&self,
		ids: &(String, String, String),
		key: &str,
		result: Option<&AgentActivityDetailResult>,
		cx: &mut Context<Self>,
	) -> Div {
		match result {
			Some(AgentActivityDetailResult::Available { text, offset, next, .. }) => {
				let first_ids = ids.clone();
				let next_ids = ids.clone();

				gpui::div()
					.child(SelectableText {
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
						d.child(gpui::div().debug_selector(|| "detail-next-action".into()).child(
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
				gpui::div().child("Source details are unavailable. Collapse and reopen to retry."),
			None => gpui::div().child(ui_loading::loading("Loading details")),
		}
	}

	fn toggle_activity_detail(&mut self, ids: (String, String, String), cx: &mut Context<Self>) {
		let Some(key) = self.activity_detail_key(&ids) else {
			return;
		};

		self.timeline.latest_follow_work = None;

		self.timeline.follow_paused.insert(ids.0.clone());

		self.timeline.navigation = None;

		if let Some(entry) = self
			.timeline
			.native
			.entries
			.iter()
			.find(|entry| {
				matches!(&entry.content,
			AgentTimelineContent::Item{ turn_id, item_id, .. } if turn_id == &ids.1 && item_id == &ids.2)
			})
			.cloned()
		{
			self.anchor_process_toggle(&ids.0, &entry);
		}

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
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

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
	use std::thread;

	use crate::shell::agent_surface::detail::{
		AgentActivityDetailResult, AgentActivityDto, AgentSurface, AgentWorkItemDto, Context,
		EntityId,
	};
	#[cfg(test)]
	use crate::shell::agent_surface::detail::{
		AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentWorkStatusDto,
		LoadState, native_timeline,
	};

	fn prepare_tool_history(s: &mut AgentSurface, cx: &mut Context<AgentSurface>) {
		s.visual_workspace_fixture(cx);

		s.snapshot.as_mut().unwrap().runtime_source = Some(EntityId::new("source").unwrap());
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| w.id == "agent")
			.unwrap()
			.codex_thread_id = Some("thread".into());
		s.timeline.native.binding = Some(native_timeline::Binding {
			work: "agent".into(),
			thread: "thread".into(),
			account: "account".into(),
		});

		s.timeline.native.entries.push(decodex_protocol::AgentTimelineEntry {
			position: 0,
			content: decodex_protocol::AgentTimelineContent::Item {
				collaboration: None,
				turn_id: "turn".into(),
				item_id: "search".into(),
				kind: "webSearch".into(),
				text: String::new(),
				phase: None,
				truncated: false,
				app_ui: false,
				attachments: vec![],
				activity: Some(AgentActivityDto {
					turn_id: "turn".into(),
					item_id: "search".into(),
					kind: "webSearch".into(),
					status: "completed".into(),
					label: "Searching the web for a long query with several terms".into(),
					detail: String::new(),
					plugin_id: None,
					read_only_hint: None,
					native_timestamp_ms: None,
					duration_ms: Some(12_345),
				}),
			},
		});
		cx.notify();
	}

	fn complete_tool_history(s: &mut AgentSurface, cx: &mut Context<AgentSurface>) {
		let mut final_entry = s.timeline.native.entries[0].clone();

		final_entry.position = 1;

		if let decodex_protocol::AgentTimelineContent::Item {
			kind,
			item_id,
			text,
			phase,
			activity,
			..
		} = &mut final_entry.content
		{
			*kind = "agentMessage".into();
			*item_id = "final".into();
			*text = "Done".into();
			*phase = Some("final_answer".into());
			*activity = None;
		}

		s.timeline.native.entries.push(final_entry);
		s.timeline.native.entries.push(decodex_protocol::AgentTimelineEntry {
			position: 2,
			content: decodex_protocol::AgentTimelineContent::TurnBoundary {
				turn_id: "turn".into(),
				completed: true,
				status: Some("completed".into()),
				duration_ms: Some(500),
				usage_summary: None,
				usage: None,
				error: None,
			},
		});
		cx.notify();
	}

	fn activity_detail_snapshot() -> AgentSnapshotDto {
		AgentSnapshotDto {
			context_references: vec![],
			connection_initializing: false,
			runtime_source: Some(EntityId::new("source").expect("valid activity detail source")),
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
		}
	}

	#[gpui::test]
	fn expanded_tool_stays_inside_transcript_and_survives_background_refresh(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(900.), gpui::px(1_000.)));

		let ids = ("agent".to_owned(), "turn".to_owned(), "search".to_owned());

		surface.update(visual, prepare_tool_history);
		visual.update(|w, cx| w.draw(cx).clear(cx));

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| w.draw(cx).clear(cx));

		let row = visual.debug_bounds("tool-detail-row").unwrap();
		let transcript = visual.debug_bounds("workspace-transcript").unwrap();

		assert!(row.right() <= transcript.right(), "tool indent must fit the transcript");

		let standalone_arrow = visual.debug_bounds("tool-chevron-bounds").unwrap();
		let arrow = standalone_arrow;

		assert!(arrow.right() <= row.right(), "arrow {arrow:?} must fit row {row:?}");

		visual.simulate_click(row.center(), Default::default());

		surface.update(visual, |s, cx| {
			assert!(s.timeline.follow_paused.contains("agent"));

			let key = s.activity_detail_key(&ids).unwrap();

			s.activity_detail.value = Some((
				key,
				Some(AgentActivityDetailResult::Available {
					text: "Search details\n".repeat(30),
					truncated: false,
					offset: 0,
					next: None,
				}),
			));

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear(cx));

		thread::sleep(std::time::Duration::from_millis(300));

		visual.update(|w, cx| w.draw(cx).clear(cx));

		let expanded = visual.debug_bounds("tool-detail-row").unwrap();

		assert!(expanded.size.height > row.size.height);

		for refreshing in [true, false, true, false] {
			surface.update(visual, |s, cx| {
				s.state = if refreshing { LoadState::Loading } else { LoadState::Ready };
				s.status_before_refresh = refreshing.then_some(LoadState::Ready);

				cx.notify();
			});

			visual.update(|w, cx| w.draw(cx).clear(cx));

			assert_eq!(visual.debug_bounds("tool-detail-row").unwrap(), expanded);
		}

		// Completed turns use the folded-history path, not the standalone tool row.
		surface.update(visual, complete_tool_history);
		visual.update(|w, cx| w.draw(cx).clear(cx));

		let toggle = visual.debug_bounds("turn-process-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|w, cx| w.draw(cx).clear(cx));

		thread::sleep(std::time::Duration::from_millis(250));

		visual.update(|w, cx| w.draw(cx).clear(cx));

		let arrow = visual.debug_bounds("tool-chevron-bounds").unwrap();

		assert!(
			arrow.right() <= standalone_arrow.right(),
			"grouped arrow {arrow:?} must align with standalone arrow {standalone_arrow:?}"
		);
	}

	#[gpui::test]
	fn activity_details_reject_replaced_sources_and_reopened_requests(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			let snapshot = activity_detail_snapshot();
			let ids = ("work".into(), "turn".into(), "item".into());
			let result = || AgentActivityDetailResult::Available {
				text: "Passed".into(),
				truncated: false,
				offset: 0,
				next: None,
			};

			for change in
				["none", "refresh", "source", "thread", "reopen", "disconnect", "unavailable", "profile"]
			{
				s.apply_result(Ok(AgentSnapshotResult::Available(snapshot.clone())));

				let key = s.activity_detail_key(&ids).unwrap();
				let revision = s.activity_detail.revision;

				s.activity_detail.value = Some((key.clone(), None));

				match change {
					"refresh" => {
						s.state = LoadState::Loading;
						s.status_before_refresh = Some(LoadState::Ready);

						assert_eq!(s.activity_detail_key(&ids).as_ref(), Some(&key));
						assert!(s.command_connection_ready());
					},
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
					matches!(change, "none" | "refresh"),
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
