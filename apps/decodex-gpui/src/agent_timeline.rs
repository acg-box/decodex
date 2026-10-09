//! Native timeline page state. Native IDs remain distinct from local inbox row IDs.
#[path = "agent_timeline_groups.rs"] mod groups;
#[path = "agent_timeline_inputs.rs"] mod inputs;
#[path = "agent_timeline_media.rs"] mod media;
#[path = "agent_timeline_receipts.rs"] mod receipts;
#[path = "agent_timeline_render.rs"] mod render;
#[path = "agent_timeline_scroll.rs"] mod scroll;

use std::{collections::BTreeSet, mem, time::Duration};

use gpui::{AnyElement, Div, StatefulInteractiveElement as _};
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotResult;
#[cfg(test)] use crate::shell::agent_surface::wire_test_support;
use crate::{
	shell::agent_surface::{
		self, AgentClient, AgentHistoryResult, AgentSurface, AgentWorkItemDto, ClientProfile,
		Context, EntityId, FluentBuilder, InteractiveElement, IntoElement, ParentElement,
		SharedString, Styled, StyledImage, Task, WireText, auth_recovery_entry, markdown,
		text_reveal::StreamingText,
	},
	ui_loading, ui_motion,
};
use decodex_protocol::{
	AgentTimelineContent, AgentTimelineEntry, AgentTimelinePage, WeatherForecast,
};
use inputs::InputReceipts;
use media::Preview;
use scroll::{ROW_GAP, Viewport};

impl AgentSurface {
	pub(super) fn restore_prompt_presentation(
		&mut self,
		work: &str,
		thread: &str,
		history: AgentHistoryResult,
		timeline: decodex_protocol::AgentTimelineResult,
		cx: &mut Context<Self>,
	) -> Result<(), &'static str> {
		if self.selected.as_deref() != Some(work)
			|| !self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot
					.work_items
					.iter()
					.any(|item| item.id == work && item.codex_thread_id.as_deref() == Some(thread))
			}) {
			return Err("History source changed");
		}
		if !matches!(&history, AgentHistoryResult::Available { questions_recovering: false, .. }) {
			return Err("Question history recovery is incomplete");
		}

		let decodex_protocol::AgentTimelineResult::Available { work_id, account_id, page } =
			timeline
		else {
			return Err("Native history is unavailable");
		};

		if work_id.as_str() != work || page.thread_id != thread {
			return Err("Native history source changed");
		}

		let mut restored =
			Timeline { epoch: self.timeline.native.epoch.wrapping_add(1), ..Default::default() };

		if !restored.replace(
			Binding {
				work: work.into(),
				thread: thread.into(),
				account: account_id.as_str().into(),
			},
			page,
		) {
			return Err("Native history page could not be applied");
		}

		restored.requested = Some((work.into(), thread.into()));

		self.cancel_native_scroll_anchor();

		self.timeline.native = restored;
		self.history_task = None;
		self.older_task = None;
		self.timeline.loading_older = false;
		self.timeline.older_retry_after = None;
		self.timeline.older_scroll_anchor = None;

		self.timeline.older_history.remove(work);

		self.output_stream = Default::default();
		self.history_requested_for = Some(work.into());
		self.timeline.read_at = Some(std::time::Instant::now());

		self.observe_question_notices(&history);
		self.prepare_async_question_inputs(work, &history, cx);
		self.timeline.cache.insert(work.into(), history.clone());

		self.history = Some((work.into(), history));
		self.timeline.navigation = None;

		self.timeline.follow_paused.remove(work);
		self.timeline.scroll.entry(work.into()).or_default().scroll_to_bottom();
		cx.notify();

		Ok(())
	}

	pub(super) fn history_viewport_fill_ready(&self) -> bool {
		if self.profile.is_none() {
			return false;
		}
		if let Some(binding) =
			self.timeline.native.binding.as_ref().filter(|_| !self.timeline.native.show_saved)
		{
			return self.selected.as_ref() == Some(&binding.work)
				&& self.history_viewport_underfilled(&binding.work)
				&& self.timeline.native.older_cursor.is_some()
				&& self.timeline.native.notice.is_none()
				&& self.timeline.native.task.is_none()
				&& !self.native_pagination_settling()
				&& self.timeline.native.can_retry(std::time::Instant::now());
		}
		self.history_prefetch_needed()
			&& self.timeline.older_retry_after.is_none_or(|at| at <= std::time::Instant::now())
	}

	pub(super) fn prefetch_native_history(&mut self, cx: &mut Context<Self>) -> bool {
		let Some(binding) = self.timeline.native.binding.clone().filter(|binding| {
			!self.timeline.native.show_saved && self.selected.as_ref() == Some(&binding.work)
		}) else {
			return false;
		};

		let underfilled = self.history_viewport_underfilled(&binding.work)
			&& self.timeline.native.notice.is_none();
		let near_top = self.timeline.native.prefetch_requested
			&& self.timeline.follow_paused.contains(&binding.work)
			&& self.timeline.scroll.get(&binding.work).is_some_and(|scroll| {
				let height = f32::from(scroll.bounds().size.height);
				height > 0. && -f32::from(scroll.offset().y) <= (height * 0.6).clamp(240., 600.)
			});
		if (underfilled || near_top)
			&& self.timeline.native.older_cursor.is_some()
			&& self.timeline.native.task.is_none()
			&& !self.native_pagination_settling()
			&& self.timeline.native.can_retry(std::time::Instant::now())
		{
			self.load_native_timeline_page(&binding.work, &binding.thread, true, underfilled, cx);
		}

		true
	}

	pub(super) fn refresh_open_native_history(&mut self, cx: &mut Context<Self>) {
		if self.connection_initializing() {
			return;
		}
		let Some((work, thread)) =
			self.conversation_work().and_then(|work| Some((work.id, work.codex_thread_id?)))
		else {
			self.timeline.native.reset();

			return;
		};

		if self.timeline.native.requested.as_ref() != Some(&(work.clone(), thread.clone())) {
			self.timeline.native.reset();
			self.timeline.native.viewport.request_latest();
		}

		let selected = self.conversation_work();
		let turn = selected.as_ref().and_then(|work| work.active_turn_id.as_deref());

		self.timeline.native.retry_after_turn_change(turn);

		if self.timeline.native.can_retry(std::time::Instant::now()) {
			self.load_native_timeline(&work, &thread, false, cx);
		}
	}

	pub(super) fn native_history_active(&self, work: &AgentWorkItemDto) -> bool {
		!self.timeline.native.show_saved
			&& self.timeline.native.binding.as_ref().is_some_and(|binding| {
				binding.work == work.id && Some(&binding.thread) == work.codex_thread_id.as_ref()
			})
	}

	pub(super) fn native_history_loading(&self, work: &AgentWorkItemDto) -> bool {
		if self.timeline.native.show_saved
			|| self.native_history_active(work)
			|| self.timeline.native.failures > 0
			|| work.codex_thread_id.is_none()
		{
			return false;
		}

		match self.timeline.native.requested.as_ref() {
			None => self.profile.is_some(),
			Some((id, thread)) =>
				self.timeline.native.task.is_some()
					&& id == &work.id
					&& Some(thread) == work.codex_thread_id.as_ref(),
		}
	}

	pub(super) fn native_history_controls(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let owner = work.id.clone();
		let thread = work.codex_thread_id.clone();
		let mut controls = gpui::div().flex().flex_col().gap(gpui::px(ROW_GAP)).child(
			gpui::div().debug_selector(|| "native-latest-action".into()).child(
				self.workspace_action(
					"native-timeline-refresh".into(),
					"Refresh conversation".into(),
					move |s, cx| {
						if let Some(thread) = &thread {
							s.read_latest_native_history(&owner, thread, cx);
						}
					},
					cx,
				),
			),
		);

		if self.native_agents.selected.is_none()
			&& self.timeline.native.binding.as_ref().is_some_and(|b| {
				b.work == work.id && Some(&b.thread) == work.codex_thread_id.as_ref()
			}) {
			controls = controls.child(
				gpui::div().debug_selector(|| "native-history-source-toggle".into()).child(
					self.workspace_action(
						"native-history-source".into(),
						if self.timeline.native.show_saved {
							"Show conversation"
						} else {
							"Show saved local records"
						}
						.into(),
						|s, cx| {
							s.cancel_native_scroll_anchor();

							s.timeline.native.show_saved = !s.timeline.native.show_saved;
							s.timeline.navigation = None;

							cx.notify();
						},
						cx,
					),
				),
			);
		}
		controls.into_any_element()
	}

	pub(super) fn native_timeline_panel(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut panel = gpui::div().flex().flex_col().gap(gpui::px(ROW_GAP));

		// Reserve the first-load state before the request starts, but retain
		// existing history during background refreshes and fallback retries.
		if self.native_history_loading(work) && !self.connection_initializing() {
			panel = panel.child(ui_loading::conversation("Loading conversation"));
		}
		if self.timeline.native.requested.as_ref().is_some_and(|(id, thread)| {
			id == &work.id && Some(thread) == work.codex_thread_id.as_ref()
		}) && let Some(message) = self.timeline.native.notice
		{
			panel = panel.child(
				gpui::div()
					.debug_selector(|| "native-history-notice".into())
					.child(agent_surface::muted(message)),
			);
		}
		if self
			.timeline
			.native
			.binding
			.as_ref()
			.is_some_and(|b| b.work == work.id && Some(&b.thread) == work.codex_thread_id.as_ref())
		{
			if self.timeline.native.show_saved {
				return panel
					.child(agent_surface::muted(
						"Saved local records can include earlier thread bindings and delivery receipts.",
					))
					.into_any_element();
			}

			let binding = self.timeline.native.binding.as_ref().expect("matching binding").clone();

			if self.timeline.native.browsing_window {
				panel = panel.child(agent_surface::muted(
					"Showing an earlier history window. Open Details to refresh the conversation.",
				));
			}
			if self.timeline.native.older_cursor.is_some() {
				panel = panel.child(
					self.workspace_action(
						"native-timeline-older".into(),
						if self.timeline.native.task.is_some() {
							"Loading earlier history…"
						} else {
							"Load earlier history · up to 15 records"
						}
						.into(),
						move |s, cx| {
							s.load_native_timeline(&binding.work, &binding.thread, true, cx)
						},
						cx,
					),
				);
			}
			if self.timeline.native.opening_session.is_some() {
				panel = panel.child(agent_surface::muted(
					"Voice conversation continued from an earlier page.",
				));
			}

			for item in &self.timeline.native.summary {
				panel = panel.child(self.native_summary_row(work, item, cx));
			}

			panel = self.append_native_history_rows(panel, work, cx);
			panel = panel.children(self.send_previews(&work.id));

			if let Some(messages) = self.streamed_output(work) {
				for message in messages.iter().filter(|message| {
                    work.active_turn_id.as_deref() == Some(&message.turn_id)
                        && !self.timeline.native.entries.iter().any(|entry| matches!(&entry.content,
                            AgentTimelineContent::Item{ turn_id, item_id, .. } if turn_id == &message.turn_id && item_id == &message.item_id))
                }) {
                    panel = panel.child(StreamingText {
                        text: markdown::response_text(&message.text),
                        key: format!("native-draft-{}-{}-{}", work.id, message.turn_id, message.item_id),
                    });
                }
			}
		}

		panel.into_any_element()
	}

	fn process_history_body(
		&self,
		work: &AgentWorkItemDto,
		group: &groups::Group,
		entry: &AgentTimelineEntry,
		first: bool,
		cx: &mut Context<Self>,
	) -> Div {
		let owner = cx.entity();
		let indices = group.indices.clone();
		let source_work = work.clone();
		let header = first.then(|| self.turn_process_header(work, group, entry, cx));
		gpui::div().w_full().debug_selector(|| "turn-process-block".into()).children(header).child(
			ui_motion::disclosure_lazy(
				SharedString::from(format!(
					"turn-process-body-{}-{}-{}",
					work.id,
					group.turn,
					serde_json::json!(key(entry))
				)),
				group.expanded,
				move |cx| {
					owner.update(cx, |s, cx| {
						render::process_indent(
							gpui::div().w_full().flex().flex_col().gap(gpui::px(8.)).children(
								indices
									.iter()
									.filter_map(|index| s.timeline.native.entries.get(*index))
									.map(|entry| {
										s.native_timeline_content(
											&source_work,
											entry,
											&format!(
												"process-{}-{}",
												source_work.id,
												serde_json::json!(key(entry))
											),
											cx,
										)
									}),
							),
						)
					})
				},
			),
		)
	}

	fn append_native_history_rows(
		&self,
		mut panel: Div,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Div {
		let groups =
			groups::groups(&self.timeline.native.entries, &self.timeline.native.expanded_turns);
		let mut collapsed = BTreeSet::new();
		let mut headers = std::collections::BTreeMap::new();

		for group in &groups {
			headers.insert(group.indices[0], group);
			collapsed.extend(group.indices.iter().skip(1).copied());
		}

		self.prepare_process_folds(work, &collapsed);

		let voice_groups = super::voice::history::groups(&self.timeline.native.entries);
		let voice_headers: std::collections::BTreeMap<_, _> =
			voice_groups.iter().map(|g| (g.indices[0], g)).collect();
		let voice_hidden: BTreeSet<_> =
			voice_groups.iter().flat_map(|g| g.indices.iter().skip(1).copied()).collect();
		let empty_reasoning = groups::empty_completed_reasoning(&self.timeline.native.entries);
		let replied_turns: BTreeSet<_> = self
			.timeline
			.native
			.entries
			.iter()
			.filter_map(|entry| match &entry.content {
				AgentTimelineContent::Item { turn_id, kind, .. } if kind == "agentMessage" =>
					Some(turn_id),
				_ => None,
			})
			.collect();
		let mut hidden = Vec::new();

		for (index, entry) in self.timeline.native.entries.iter().enumerate() {
			if let Some(group) = voice_headers.get(&index) {
				let entry = &self.timeline.native.entries
					[group.anchor_index(&self.timeline.native.entries)];
				if group.hidden() {
					continue;
				}
				if !hidden.is_empty() {
					panel = panel.child(self.native_history_spacer(work, mem::take(&mut hidden)));
				}
				let title = if group.failed {
					"Voice conversation · Failed"
				} else if group.ended {
					"Voice conversation · Ended"
				} else {
					"Voice conversation"
				};
				let body = if group.text.trim().is_empty() {
					gpui::div()
						.id(SharedString::from(format!(
							"voice-status-{}",
							serde_json::json!([work.id, key(entry)])
						)))
						.role(gpui::Role::Status)
						.child(agent_surface::muted(group.empty_status()))
						.into_any_element()
				} else {
					super::voice::history::VoiceBlock {
						key: serde_json::json!([
							work.id,
							work.codex_thread_id,
							group.identity(&self.timeline.native.entries)
						])
						.to_string(),
						title: title.into(),
						expanded: !group.ended,
						text: group.text.clone(),
					}
					.into_any_element()
				};
				let body = self.anchored_native_history_entry(work, entry, body);
				panel = panel.child(self.native_scroll_row(work, entry, body, cx));
				continue;
			}
			if voice_hidden.contains(&index) {
				continue;
			}
			if matches!(&entry.content,
				AgentTimelineContent::TurnBoundary { completed: true, turn_id, status, error: None, .. }
				if replied_turns.contains(turn_id)
					&& !matches!(status.as_deref(), Some("interrupted" | "failed")))
			{
				continue;
			}
			if empty_reasoning.contains(&index) {
				continue;
			}

			if let Some(group) = headers.get(&index) {
				if !hidden.is_empty() {
					panel = panel.child(self.native_history_spacer(work, mem::take(&mut hidden)));
				}

				let body =
					self.process_history_body(work, group, entry, index == group.first_index, cx);

				panel =
					panel.child(self.native_scroll_row(work, entry, body.into_any_element(), cx));

				continue;
			}

			if collapsed.contains(&index) {
				continue;
			}

			if let Some(height) = self.native_offscreen_height(work, entry) {
				hidden.push((entry, height));

				continue;
			}

			if !hidden.is_empty() {
				panel = panel.child(self.native_history_spacer(work, mem::take(&mut hidden)));
			}

			panel = panel.child(self.native_timeline_row(work, entry, cx));
		}

		if !hidden.is_empty() {
			panel = panel.child(self.native_history_spacer(work, hidden));
		}

		panel
	}

	fn read_latest_native_history(&mut self, work: &str, thread: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work) || self.timeline.native.task.is_some() {
			return;
		}

		self.cancel_native_scroll_anchor();

		self.timeline.native.epoch = self.timeline.native.epoch.wrapping_add(1);

		self.timeline.native.recovered();

		self.timeline.native.show_saved = false;

		self.timeline.native.viewport.request_latest();
		self.timeline.follow_paused.remove(work);

		self.timeline.navigation = None;

		self.set_voice_follow(true);
		self.load_native_timeline(work, thread, false, cx);
	}

	fn refresh_native_summary(&mut self, binding: Binding, items: Vec<AgentTimelineContent>) {
		let jump = self.timeline.native.viewport.take_latest_request();
		let work = binding.work.clone();

		self.cancel_native_scroll_anchor();
		self.timeline.native.accept_summary(binding, items);

		if jump {
			self.timeline.scroll.entry(work).or_default().scroll_to_bottom();
		}
	}

	fn refresh_native_history(&mut self, binding: Binding, page: AgentTimelinePage) -> bool {
		let jump = self.timeline.native.viewport.take_latest_request();
		let work = binding.work.clone();

		// Do not fold a running process out from under a reader browsing history.
		if self.timeline.follow_paused.contains(&work) && !self.timeline.native.entries.is_empty() {
			for entry in &page.entries {
				if let AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } =
					&entry.content
				{
					let already_finished = self.timeline.native.entries.iter().any(|old| {
						matches!(&old.content,
						AgentTimelineContent::TurnBoundary {turn_id: old_turn, completed: true, ..} if old_turn == turn_id)
					});

					if !already_finished {
						self.timeline.native.expanded_turns.insert(turn_id.clone());
					}
				}
			}
		}

		let accepted = if jump {
			self.timeline.native.replace(binding, page)
		} else {
			self.timeline.native.refresh(binding, page)
		};

		if accepted && jump {
			self.timeline.scroll.entry(work).or_default().scroll_to_bottom();
		}

		accepted
	}

	fn load_native_timeline(
		&mut self,
		work: &str,
		thread: &str,
		older: bool,
		cx: &mut Context<Self>,
	) {
		self.load_native_timeline_page(work, thread, older, false, cx);
	}

	fn load_native_timeline_page(
		&mut self,
		work: &str,
		thread: &str,
		older: bool,
		fill_viewport: bool,
		cx: &mut Context<Self>,
	) {
		if self.selected.as_deref() != Some(work) || self.timeline.native.task.is_some() {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			return;
		};
		let cursor = if older { self.timeline.native.older_cursor.clone() } else { None };

		if older && cursor.is_none() {
			return;
		}

		let (Ok(work_id), Ok(thread_id)) = (EntityId::new(work), EntityId::new(thread)) else {
			return;
		};

		if older {
			self.timeline.native.prefetch_requested = false;
			if !fill_viewport {
				self.timeline.follow_paused.insert(work.into());
				self.timeline.navigation = None;
				self.set_voice_follow(false);
			}
		}

		let (work, thread) = (work.to_owned(), thread.to_owned());
		let epoch = self.timeline.native.epoch;

		self.timeline.native.requested = Some((work.clone(), thread.clone()));
		self.timeline.native.requested_turn =
			self.conversation_work().and_then(|work| work.active_turn_id);

		let sent_cursor = cursor.clone();
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime
				.block_on(AgentClient::new(profile).timeline(
					work_id,
					thread_id,
					cursor.map(WireText::new).transpose().ok()?,
				))
				.ok()
		});

		self.timeline.native.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.timeline.native.epoch != epoch {
					return;
				}

				if s.selected.as_deref() != Some(&work) || !s.conversation_matches(&work, &thread) {
					return;
				}
				s.timeline.native.task = None;

				if let Some(decodex_protocol::AgentTimelineResult::Summary {
					work_id,
					account_id,
					thread_id,
					items,
				}) = &result
					&& sent_cursor.is_none()
					&& work_id.as_str() == work
					&& thread_id == &thread
				{
					s.refresh_native_summary(
						Binding {
							work: work.clone(),
							thread: thread.clone(),
							account: account_id.as_str().into(),
						},
						items.clone(),
					);
					cx.notify();

					return;
				}
				if let Some(decodex_protocol::AgentTimelineResult::Available {
					account_id,
					page,
					..
				}) = result
				{
					let binding = Binding { work, thread, account: account_id.as_str().into() };
					let accepted = match sent_cursor {
						Some(cursor) => {
							let accepted = s.prepend_native_history(&binding, &cursor, page);
							if accepted
								&& fill_viewport
								&& !s.timeline.follow_paused.contains(&binding.work)
							{
								s.cancel_native_scroll_anchor();
								if let Some(scroll) = s.timeline.scroll.get(&binding.work) {
									scroll.scroll_to_bottom();
								}
							}
							accepted
						},
						None => s.refresh_native_history(binding, page),
					};

					if accepted {
						s.timeline.native.recovered();
					} else {
						s.timeline.native.safety_buffering_turn_id = None;
						s.timeline.native.notice = Some(
							"History changed or the display limit was reached. Refresh native history.",
						);
					}
				} else {
					s.timeline.native.failed(result, std::time::Instant::now());
				}

				s.refresh_native_input_receipts(cx);
				cx.notify();
			});
		}));

		cx.notify();
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Binding {
	pub work: String,
	pub thread: String,
	pub account: String,
}

#[derive(Default)]
pub(super) struct Timeline {
	preview: Preview,
	input_receipts: InputReceipts,
	pub task: Option<Task<()>>,
	pub epoch: u64,
	pub revision: u64,
	pub binding: Option<Binding>,
	pub entries: Vec<AgentTimelineEntry>,
	pub safety_buffering_turn_id: Option<String>,
	pub weather: std::collections::BTreeMap<String, Vec<WeatherForecast>>,
	summary: Vec<AgentTimelineContent>,
	pub older_cursor: Option<String>,
	pub prefetch_requested: bool,
	pub opening_session: Option<String>,
	requested: Option<(String, String)>,
	requested_turn: Option<String>,
	retry_at: Option<std::time::Instant>,
	failures: u32,
	unsupported: bool,
	browsing_window: bool,
	show_saved: bool,
	expanded_turns: BTreeSet<String>,
	viewport: Viewport,
	notice: Option<&'static str>,
	seen_cursors: BTreeSet<String>,
}
impl Timeline {
	pub(super) fn visible_export_items(&self) -> impl Iterator<Item = &AgentTimelineContent> {
		self.entries.iter().map(|entry| &entry.content).chain(self.summary.iter())
	}

	pub(super) fn summary_only(&self) -> bool {
		!self.summary.is_empty()
	}

	pub(super) fn reset(&mut self) {
		*self = Self { epoch: self.epoch.wrapping_add(1), ..Default::default() };
	}

	fn can_retry(&self, now: std::time::Instant) -> bool {
		!self.browsing_window && self.retry_at.is_none_or(|at| now >= at)
	}

	fn retry_after_turn_change(&mut self, turn: Option<&str>) {
		// A fresh paginated native thread reports method-not-found until its first
		// turn creates history. Recheck after new execution evidence, not every poll.
		if self.unsupported && turn.is_some() && turn != self.requested_turn.as_deref() {
			self.recovered();
		}
	}

	fn recovered(&mut self) {
		self.retry_at = None;
		self.failures = 0;
		self.unsupported = false;
		self.notice = None;
	}

	fn failed(
		&mut self,
		result: Option<decodex_protocol::AgentTimelineResult>,
		now: std::time::Instant,
	) {
		self.clear_page();

		self.unsupported =
			matches!(result, Some(decodex_protocol::AgentTimelineResult::Unsupported));
		self.failures = self.failures.saturating_add(1);

		// Native background migration can make a legacy thread readable without
		// another turn or process replacement. Recheck infrequently while selected.
		let delay = if self.unsupported {
			300
		} else {
			(5_u64 << self.failures.saturating_sub(1).min(3)).min(30)
		};

		self.retry_at = Some(now + Duration::from_secs(delay));
		self.notice = Some(if self.unsupported {
			"This thread does not support native history. Saved local history remains available."
		} else {
			"Native history could not be loaded. Retrying… Saved local history remains available."
		});
	}

	fn accept_summary(&mut self, binding: Binding, items: Vec<AgentTimelineContent>) {
		self.clear_page();

		self.binding = Some(binding);
		self.summary = items;

		self.recovered();

		self.retry_at = Some(std::time::Instant::now() + Duration::from_secs(30));
		self.notice = Some(
			"Showing up to 100 recent prompts and final replies. Intermediate messages and tool activity are unavailable.",
		);
	}

	fn clear_page(&mut self) {
		self.revision = self.revision.wrapping_add(1);

		self.preview.clear();

		self.viewport = Default::default();
		self.binding = None;

		self.entries.clear();
		self.weather.clear();

		self.safety_buffering_turn_id = None;

		self.summary.clear();

		self.older_cursor = None;
		self.opening_session = None;

		self.seen_cursors.clear();

		self.browsing_window = false;
	}

	fn refresh(&mut self, binding: Binding, page: AgentTimelinePage) -> bool {
		if page.thread_id != binding.thread || !valid_entries(&page.entries) {
			return false;
		}

		let overlap = page
			.entries
			.first()
			.and_then(|first| self.entries.iter().position(|old| key(old) == key(first)));

		if self.binding.as_ref() == Some(&binding)
			&& let Some(start) = overlap
			&& self.entries[start..]
				.iter()
				.zip(&page.entries)
				.all(|(old, new)| key(old) == key(new))
			&& page.entries.len() >= self.entries.len() - start
			&& bounded(self.entries[..start].iter().chain(&page.entries))
		{
			self.safety_buffering_turn_id = page.safety_buffering_turn_id;

			self.weather.extend(page.weather);
			self.entries.splice(start.., page.entries);

			return true;
		}

		// Without overlap the middle is unknown. Restart at the native page boundary;
		// retaining old rows here would hide an unobserved gap behind a false adjacency.
		self.replace(binding, page)
	}

	pub(super) fn replace(&mut self, binding: Binding, page: AgentTimelinePage) -> bool {
		if page.thread_id != binding.thread
			|| !valid_entries(&page.entries)
			|| !bounded(&page.entries)
		{
			return false;
		}

		let changed = self.binding.as_ref() != Some(&binding)
			|| self.entries != page.entries
			|| self.weather != page.weather
			|| !self.summary.is_empty();

		self.summary.clear();

		if changed {
			self.viewport = Default::default();
			self.revision = self.revision.wrapping_add(1);
		}
		if self.binding.as_ref() != Some(&binding) {
			self.expanded_turns.clear();
			self.preview.clear();
		}

		self.binding = Some(binding);
		self.safety_buffering_turn_id = page.safety_buffering_turn_id;
		self.weather = page.weather;
		self.entries = page.entries;
		self.older_cursor = page.next_cursor;
		self.opening_session = page.active_realtime_session_at_page_start;

		self.seen_cursors.clear();

		self.browsing_window = false;

		true
	}

	pub(super) fn prepend(
		&mut self,
		binding: &Binding,
		cursor: &str,
		page: AgentTimelinePage,
	) -> bool {
		if self.binding.as_ref().is_some_and(|current| {
			current.work == binding.work
				&& current.thread == binding.thread
				&& current.account != binding.account
		}) {
			self.clear_page();

			return false;
		}
		if self.binding.as_ref() != Some(binding)
			|| page.thread_id != binding.thread
			|| self.older_cursor.as_deref() != Some(cursor)
			|| self.seen_cursors.contains(cursor)
			|| page.next_cursor.as_deref() == Some(cursor)
			|| page.next_cursor.as_ref().is_some_and(|next| self.seen_cursors.contains(next))
			|| !valid_entries(&page.entries)
			|| !bounded(&page.entries)
			|| (page.entries.is_empty() && page.next_cursor.is_some())
			|| page
				.entries
				.last()
				.zip(self.entries.first())
				.is_some_and(|(older, newer)| key(older) >= key(newer))
		{
			return false;
		}

		self.seen_cursors.insert(cursor.into());

		if !page.entries.is_empty() {
			self.opening_session = page.active_realtime_session_at_page_start;
		}

		self.weather.extend(page.weather);
		self.entries.splice(0..0, page.entries);

		self.revision = self.revision.wrapping_add(1);

		while !bounded(&self.entries) {
			self.entries.pop();

			self.browsing_window = true;
		}

		self.older_cursor = page.next_cursor;

		self.viewport.retain(&self.entries);

		true
	}
}

pub(super) fn key(entry: &AgentTimelineEntry) -> (u64, u8, &str) {
	let (kind, id) = match &entry.content {
		AgentTimelineContent::TurnBoundary { turn_id, completed: false, .. } => (0, turn_id),
		AgentTimelineContent::Item { item_id, .. } => (1, item_id),
		AgentTimelineContent::Speech { item_id, .. }
		| AgentTimelineContent::VoiceBoundary { item_id, .. }
		| AgentTimelineContent::Promotion { item_id, .. } => (2, item_id),
		AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } => (3, turn_id),
	};

	(entry.position, kind, id)
}

fn bounded<'a>(entries: impl IntoIterator<Item = &'a AgentTimelineEntry>) -> bool {
	let mut bytes = 0;
	let mut count = 0;

	for entry in entries {
		count += 1;

		let Ok(encoded) = serde_json::to_vec(entry) else {
			return false;
		};

		bytes += encoded.len();

		if count > 1_000 || bytes > 2 * 1_024 * 1_024 {
			return false;
		}
	}

	true
}

fn valid_entries(entries: &[AgentTimelineEntry]) -> bool {
	entries.windows(2).all(|pair| key(&pair[0]) < key(&pair[1]))
}

#[cfg(test)]
mod tests {
	use std::future;

	use gpui::AppContext as _;

	use crate::shell::agent_surface::native_timeline::{
		AgentHistoryResult, AgentSurface, AgentTimelineContent, AgentTimelineEntry,
		AgentTimelinePage, Binding, EntityId, Timeline,
	};

	#[gpui::test]
	fn pending_native_read_does_not_flash_local_records(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());
			s.timeline.native.requested = Some((work.id.clone(), "thread".into()));
			s.timeline.native.task = Some(cx.spawn(async |_, _| future::pending::<()>().await));

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_some());
		assert!(visual.debug_bounds("saved-local-history").is_none());

		// Connection feedback in the composer takes precedence over history loading.
		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().connection_initializing = true;
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear());
		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_none());
		assert!(visual.debug_bounds("loading-feedback-Connecting to Codex…").is_some());
		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().connection_initializing = false;
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear());
		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_some());
		assert!(visual.debug_bounds("loading-feedback-Connecting to Codex…").is_none());

		// A failed native read still permits the saved-history fallback.
		surface.update(visual, |s, cx| {
			s.timeline.native.task = None;

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_none());
		assert!(visual.debug_bounds("saved-local-history").is_some());

		// Retrying a failed read must retain the fallback, too.
		surface.update(visual, |s, cx| {
			s.timeline.native.failed(None, std::time::Instant::now());

			s.timeline.native.task = Some(cx.spawn(async |_, _| future::pending::<()>().await));

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_none());
		assert!(visual.debug_bounds("saved-local-history").is_some());
	}

	#[gpui::test]
	fn summary_recovery_renders_notice_and_copies_only_message_text(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		let mut copy_key = String::new();

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.expect("snapshot")
				.work_items
				.iter_mut()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.expect("work");

			work.codex_thread_id = Some("thread".into());

			let binding = Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			};

			copy_key = format!(
				"copy-{}",
				serde_json::json!(["summary", work.id, work.codex_thread_id, "turn", "answer"])
			);
			s.timeline.native.requested = Some((work.id.clone(), "thread".into()));

			s.timeline.native.accept_summary(
				binding,
				vec![AgentTimelineContent::Item {
					collaboration: None,
					phase: None,
					turn_id: "turn".into(),
					item_id: "answer".into(),
					kind: "agentMessage".into(),
					text: "Recovered **reply**".into(),
					truncated: false,
					app_ui: false,
					activity: None,
					attachments: vec![],
				}],
			);
			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("native-summary-message").is_some());
		assert!(visual.debug_bounds("native-history-notice").is_some());

		let button =
			visual.debug_bounds(Box::leak(copy_key.into_boxed_str())).expect("summary copy action");

		visual.simulate_click(button.center(), gpui::Modifiers::default());
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|v| v.text()),
				Some("Recovered **reply**".into())
			)
		});
		surface.update(visual, |s, cx| {
			s.timeline.native.reset();
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("native-summary-message").is_none());
	}

	#[test]
	fn summary_recovery_never_reuses_timeline_positions_or_cursors() {
		let binding =
			Binding { work: "work".into(), thread: "thread".into(), account: "account".into() };
		let item = AgentTimelineContent::Item {
			collaboration: None,
			phase: None,
			turn_id: "turn".into(),
			item_id: "answer".into(),
			kind: "agentMessage".into(),
			text: "Recovered".into(),
			truncated: false,
			app_ui: false,
			activity: None,
			attachments: vec![],
		};
		let mut state = Timeline::default();

		state.entries.push(AgentTimelineEntry { position: 42, content: item.clone() });

		state.older_cursor = Some("old".into());
		state.opening_session = Some("voice".into());

		state.accept_summary(binding.clone(), vec![item.clone()]);

		assert!(state.entries.is_empty());
		assert!(state.older_cursor.is_none() && state.opening_session.is_none());
		assert_eq!(state.summary, vec![item.clone()]);
		assert!(state.notice.expect("summary notice").contains("Intermediate messages"));
		assert!(state.refresh(
			binding,
			AgentTimelinePage {
				thread_id: "thread".into(),
				entries: vec![AgentTimelineEntry { position: 77, content: item }],
				next_cursor: Some("real-cursor".into()),
				weather: Default::default(),
				safety_buffering_turn_id: None,
				active_realtime_session_at_page_start: None
			}
		));

		state.recovered();

		assert!(state.summary.is_empty() && state.notice.is_none());
		assert_eq!(state.entries[0].position, 77);
		assert_eq!(state.older_cursor.as_deref(), Some("real-cursor"));

		state.reset();

		assert!(state.summary.is_empty() && state.binding.is_none());
	}

	#[gpui::test]
	fn prompt_handback_replaces_old_history_only_with_fresh_bound_pages(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s.selected.clone().unwrap();

			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|item| item.id == work)
				.unwrap()
				.codex_thread_id = Some("thread".into());

			let history = AgentHistoryResult::Available {
				questions: vec![],
				questions_truncated: false,
				questions_recovering: false,
				misalignment: None,
				usage: None,
				entries: vec![],
				has_more: false,
				next_before: None,
				live: vec![],
			};

			s.history = Some((work.clone(), history.clone()));

			s.timeline.older_history.insert(work.clone(), (vec![], Some(99)));

			let binding =
				Binding { work: work.clone(), thread: "thread".into(), account: "account".into() };

			s.timeline
				.native
				.replace(binding, page(vec![boundary(9, false)], Some("old-cursor"), None));

			let epoch = s.timeline.native.epoch;
			let timeline = decodex_protocol::AgentTimelineResult::Available {
				work_id: EntityId::new(&work).unwrap(),
				account_id: EntityId::new("account").unwrap(),
				page: page(vec![boundary(1, true)], None, None),
			};

			assert!(
				s.restore_prompt_presentation(
					&work,
					"thread",
					AgentHistoryResult::Unavailable,
					timeline.clone(),
					cx
				)
				.is_err()
			);
			assert_eq!(s.timeline.native.entries[0].position, 9);
			assert!(s.timeline.older_history.contains_key(&work));

			let mut incomplete = history.clone();

			if let AgentHistoryResult::Available { questions_recovering, .. } = &mut incomplete {
				*questions_recovering = true;
			}

			assert!(
				s.restore_prompt_presentation(&work, "thread", incomplete, timeline.clone(), cx)
					.is_err()
			);

			let mut crossed = timeline.clone();

			if let decodex_protocol::AgentTimelineResult::Available { page, .. } = &mut crossed {
				page.thread_id = "foreign".into();
			}

			assert!(
				s.restore_prompt_presentation(&work, "thread", history.clone(), crossed, cx)
					.is_err()
			);

			s.restore_prompt_presentation(&work, "thread", history, timeline, cx).unwrap();

			assert_ne!(s.timeline.native.epoch, epoch);
			assert_eq!(s.timeline.native.entries, vec![boundary(1, true)]);
			assert!(s.timeline.native.older_cursor.is_none());
			assert!(!s.timeline.older_history.contains_key(&work));
			assert!(s.timeline.cache.contains_key(&work));
		});
	}

	fn binding() -> Binding {
		Binding { work: "work".into(), thread: "thread".into(), account: "account".into() }
	}
	fn boundary(position: u64, completed: bool) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position,
			content: AgentTimelineContent::TurnBoundary {
				turn_id: "turn".into(),
				completed,
				status: None,
				duration_ms: None,
				usage: None,
				usage_summary: None,
				error: None,
			},
		}
	}
	fn page(
		entries: Vec<AgentTimelineEntry>,
		next: Option<&str>,
		session: Option<&str>,
	) -> AgentTimelinePage {
		AgentTimelinePage {
			thread_id: "thread".into(),
			entries,
			next_cursor: next.map(str::to_owned),
			weather: Default::default(),
			safety_buffering_turn_id: None,
			active_realtime_session_at_page_start: session.map(str::to_owned),
		}
	}
	#[test]
	fn safety_buffering_refresh_clears_without_changing_transcript_layout() {
		let mut state = Timeline::default();
		let mut current = page(vec![boundary(1, false)], Some("older"), None);

		current.safety_buffering_turn_id = Some("turn".into());

		assert!(state.replace(binding(), current.clone()));

		let revision = state.revision;

		assert_eq!(state.safety_buffering_turn_id.as_deref(), Some("turn"));

		current.safety_buffering_turn_id = None;

		assert!(state.refresh(binding(), current.clone()));
		assert!(state.safety_buffering_turn_id.is_none());
		assert_eq!(state.revision, revision);

		current.safety_buffering_turn_id = Some("turn".into());

		assert!(state.refresh(binding(), current));

		state.failed(None, std::time::Instant::now());

		assert!(state.safety_buffering_turn_id.is_none());
	}

	#[test]
	fn unchanged_refresh_keeps_layout_revision_but_same_length_edits_invalidate_it() {
		let mut state = Timeline::default();

		assert!(state.replace(binding(), page(vec![boundary(1, false)], None, None)));

		let revision = state.revision;

		assert!(state.replace(binding(), page(vec![boundary(1, false)], None, None)));
		assert_eq!(state.revision, revision);
		assert!(state.replace(binding(), page(vec![boundary(1, true)], None, None)));
		assert_ne!(state.revision, revision);
	}

	#[test]
	fn failed_reads_retry_without_retaining_unverified_rows_or_hammering_legacy_threads() {
		let now = std::time::Instant::now();
		let mut state =
			Timeline { requested: Some(("work".into(), "thread".into())), ..Default::default() };

		assert!(state.replace(binding(), page(vec![boundary(1, false)], None, None)));

		state.failed(None, now);

		assert!(state.entries.is_empty() && state.binding.is_none());
		assert_eq!(state.requested, Some(("work".into(), "thread".into())));
		assert!(!state.can_retry(now + std::time::Duration::from_secs(4)));
		assert!(state.can_retry(now + std::time::Duration::from_secs(5)));

		state.failed(None, now);

		assert!(!state.can_retry(now + std::time::Duration::from_secs(9)));
		assert!(state.can_retry(now + std::time::Duration::from_secs(10)));

		state.failed(Some(decodex_protocol::AgentTimelineResult::Unsupported), now);

		assert!(!state.can_retry(now + std::time::Duration::from_secs(60)));
		assert!(!state.can_retry(now + std::time::Duration::from_secs(299)));
		assert!(state.can_retry(now + std::time::Duration::from_secs(300)));

		state.recovered();

		assert!(state.can_retry(now));
		assert!(state.notice.is_none());

		state.reset();

		assert!(state.requested.is_none());
		assert_eq!(state.epoch, 1);
	}

	#[test]
	fn cold_native_history_retries_on_new_turn_without_polling_unsupported_history() {
		let now = std::time::Instant::now();
		let mut state = Timeline::default();

		state.failed(Some(decodex_protocol::AgentTimelineResult::Unsupported), now);
		state.retry_after_turn_change(None);

		assert!(!state.can_retry(now + std::time::Duration::from_secs(299)));

		state.retry_after_turn_change(Some("first-turn"));

		assert!(state.can_retry(now));

		// A legacy thread can still refuse the retry. Do not keep polling it for
		// the same acknowledged turn, including after terminal status changes.
		state.requested_turn = Some("first-turn".into());

		state.failed(Some(decodex_protocol::AgentTimelineResult::Unsupported), now);
		state.retry_after_turn_change(Some("first-turn"));

		assert!(!state.can_retry(now + std::time::Duration::from_secs(299)));

		// New execution while the earlier read was pending must also rearm it.
		state.retry_after_turn_change(Some("second-turn"));

		assert!(state.can_retry(now));
		assert!(state.replace(binding(), page(vec![boundary(1, true)], None, None)));
		assert_eq!(state.entries.len(), 1);
	}

	#[test]
	fn older_pages_keep_native_boundary_order_without_fake_local_ids() {
		let mut state = Timeline::default();

		assert!(
			state.replace(binding(), page(vec![boundary(5, true)], Some("older"), Some("voice")))
		);
		assert!(state.prepend(&binding(), "older", page(vec![boundary(5, false)], None, None)));
		assert_eq!(state.entries.len(), 2);
		assert_eq!(state.opening_session, None);
		assert_eq!(state.older_cursor, None);
		assert!(!state.prepend(&binding(), "older", page(vec![boundary(5, false)], None, None)));
	}
	#[test]
	fn stale_account_thread_cursor_and_overlapping_pages_do_not_mutate_history() {
		let mut state = Timeline::default();

		assert!(
			state.replace(binding(), page(vec![boundary(10, true)], Some("older"), Some("voice")))
		);

		for field in ["thread", "work"] {
			let mut stale = binding();

			match field {
				"thread" => stale.thread = "other".into(),
				_ => stale.work = "other".into(),
			}

			assert!(!state.prepend(&stale, "older", page(vec![boundary(9, false)], None, None)));
		}

		assert!(!state.prepend(&binding(), "older", page(vec![boundary(10, true)], None, None)));
		assert!(!state.prepend(
			&binding(),
			"older",
			page(vec![boundary(9, false)], Some("older"), None)
		));
		assert_eq!(state.entries, vec![boundary(10, true)]);
		assert_eq!(state.opening_session.as_deref(), Some("voice"));

		let mut other_account = binding();

		other_account.account = "other".into();

		assert!(!state.prepend(
			&other_account,
			"older",
			page(vec![boundary(9, false)], None, None)
		));
		assert!(state.entries.is_empty() && state.binding.is_none());
	}

	#[test]
	fn refresh_preserves_loaded_prefix_only_when_native_pages_overlap() {
		let mut state = Timeline::default();

		assert!(state.replace(
			binding(),
			page(vec![boundary(5, false), boundary(6, true)], Some("old"), Some("session"))
		));
		assert!(state.refresh(
			binding(),
			page(vec![boundary(6, true), boundary(7, false)], Some("new"), None)
		));
		assert_eq!(state.entries.len(), 3);
		assert_eq!(state.older_cursor.as_deref(), Some("old"));
		assert_eq!(state.opening_session.as_deref(), Some("session"));
		assert!(state.refresh(binding(), page(vec![boundary(20, true)], Some("gap"), None)));
		assert_eq!(state.entries, vec![boundary(20, true)]);
		assert_eq!(state.older_cursor.as_deref(), Some("gap"));

		let mut other = binding();

		other.account = "other".into();

		assert!(
			state.refresh(other, page(vec![boundary(20, true), boundary(21, false)], None, None))
		);
		assert_eq!(state.binding.as_ref().unwrap().account, "other");
		assert_eq!(state.entries.len(), 2);
	}

	#[test]
	fn cache_limit_moves_to_an_earlier_window_without_losing_continuation() {
		let mut state = Timeline::default();

		assert!(state.replace(
			binding(),
			page((1..=1_000).map(|n| boundary(n, false)).collect(), Some("old"), None)
		));
		assert!(state.prepend(&binding(), "old", page(vec![boundary(0, false)], None, None)));
		assert_eq!(state.entries.len(), 1_000);
		assert_eq!(state.entries.first(), Some(&boundary(0, false)));
		assert_eq!(state.entries.last(), Some(&boundary(999, false)));
		assert_eq!(state.older_cursor, None);
		assert!(state.browsing_window);
		assert!(!state.can_retry(std::time::Instant::now()));

		state.reset();

		assert!(state.can_retry(std::time::Instant::now()));
	}

	#[test]
	fn revoked_page_clears_identity_cursor_and_records_without_reusing_request_epoch() {
		let mut state = Timeline { epoch: 5, ..Default::default() };

		assert!(
			state.replace(binding(), page(vec![boundary(1, false)], Some("old"), Some("voice")))
		);

		state.clear_page();

		assert!(
			state.binding.is_none()
				&& state.entries.is_empty()
				&& state.older_cursor.is_none()
				&& state.opening_session.is_none()
		);
		assert_eq!(state.epoch, 5);
	}
}
