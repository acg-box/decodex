//! Native timeline page state. Native IDs remain distinct from local inbox row IDs.
#[path = "agent_timeline_completion.rs"] mod completion;
#[path = "agent_timeline_find.rs"] mod find;
#[path = "agent_timeline_groups.rs"] mod groups;
#[path = "agent_timeline_inputs.rs"] mod inputs;
#[path = "agent_timeline_media.rs"] mod media;
#[path = "agent_timeline_motion.rs"] mod motion;
#[path = "agent_timeline_receipts.rs"] mod receipts;
#[path = "agent_timeline_render.rs"] mod render;
#[path = "agent_timeline_scroll.rs"] mod scroll;
#[path = "agent_timeline_waits.rs"] mod waits;

pub(super) use render::activity_label;

use std::{cell::RefCell, collections::BTreeSet, mem, time::Duration};

use gpui::{AnyElement, Div, FocusHandle, Role, StatefulInteractiveElement as _};
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotResult;
#[cfg(test)] use crate::shell::agent_surface::wire_test_support;
use crate::{
	shell::agent_surface::{
		self, AgentClient, AgentHistoryResult, AgentSurface, AgentWorkItemDto, Context, EntityId,
		FluentBuilder, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
		StyledImage, Task, WireText, auth_recovery_entry, markdown,
		voice::history::{self, Conversation, VoiceBlock},
	},
	ui_loading::{self, ContentArrival},
	ui_motion,
};
use completion::{Closing, Completions};
use decodex_protocol::{
	AgentSnapshotDto, AgentTimelineContent, AgentTimelineEntry, AgentTimelinePage, WeatherForecast,
};
use inputs::InputReceipts;
use media::Preview;
use motion::Arrivals;
use scroll::{ROW_GAP, Viewport};

const SUMMARY_NOTICE: &str =
	"Up to 100 recent prompts and final replies. Intermediate activity is unavailable.";

impl AgentSurface {
	fn is_revealed_history_item(&self, entry: &AgentTimelineEntry) -> bool {
		self.is_conversation_match(entry) || self.is_located_item(&entry.content)
	}

	pub(super) fn pause_item_history_navigation(&mut self) {
		self.timeline.native.cancel_read();
		self.timeline.latest_follow_work = None;
		self.timeline.navigation = None;
		self.timeline.wheel_scroll = None;
		self.timeline.native.find.open = false;
		self.timeline.native.find.current = None;
		self.timeline.native.find.searching_earlier = false;
		if let Some(work) = &self.selected {
			self.timeline.follow_paused.insert(work.clone());
		}
		self.set_voice_follow(false);
	}

	pub(super) fn locate_loaded_history_item(
		&mut self,
		request: &decodex_protocol::ItemContextRequest,
	) -> bool {
		if !self.timeline.native.binding.as_ref().is_some_and(|binding| {
			binding.work == request.work_id.as_str() && binding.thread == request.thread_id.as_str()
		}) {
			return false;
		}
		let Some(entry) = self.timeline.native.entries.iter().find(|entry| {
			matches!(&entry.content, AgentTimelineContent::Item { turn_id, item_id, .. }
				if turn_id == request.turn_id.as_str() && item_id == request.item_id.as_str())
		}) else {
			return false;
		};
		let (position, kind, id) = key(entry);
		let key = (position, kind, id.to_owned());
		self.timeline.native.show_saved = false;
		self.timeline.native.viewport.find(key);
		true
	}

	#[cfg(feature = "visual-capture")]
	pub(crate) fn visual_mermaid_scroll_page(&mut self, cx: &mut Context<Self>) {
		self.visual_workspace_page("markdown", cx);

		let work = "agent";
		let thread = "mermaid-scroll-fixture";
		let fixtures: Vec<(String, String)> =
			serde_json::from_str(include_str!("agent_mermaid/fixtures.json")).expect("fixtures");
		let diagrams: Vec<_> = fixtures
			.into_iter()
			.filter(|(kind, _)| matches!(kind.as_str(), "flowchart" | "sequence" | "pie"))
			.collect();
		let plain = std::env::var("DECODEX_VISUAL_SCROLL_CONTENT").as_deref() == Ok("plain");
		let entries = (0..36)
			.map(|position| {
				let (kind, source) = &diagrams[position as usize % diagrams.len()];
				let language = if plain { "text" } else { "mermaid" };

				AgentTimelineEntry {
					position,
					content: AgentTimelineContent::Item {
						turn_id: format!("turn-{position}"),
						item_id: format!("item-{position}"),
						kind: "agentMessage".into(),
						text: format!(
							"## Review {} · {kind}\n\n中文与 English 混合内容，检查滚动时的文字、图形和布局稳定性。\n\n```{language}\n{source}\n%% Entry {position}\n```\n\nThis response keeps the original source available for copying.\n",
							position + 1
						),
						phase: None,
						app_ui: false,
						truncated: false,
						activity: None,
						attachments: vec![],
						collaboration: None,
					},
				}
			})
			.collect();

		for item in &mut self.snapshot.as_mut().expect("fixture snapshot").work_items {
			if item.id == work {
				item.codex_thread_id = Some(thread.into());
			}
		}

		self.timeline.native = Timeline {
			binding: Some(Binding {
				work: work.into(),
				thread: thread.into(),
				account: "scroll-fixture".into(),
			}),
			requested: Some((work.into(), thread.into())),
			entries,
			..Default::default()
		};

		self.timeline.follow_paused.insert(work.into());
		self.timeline
			.scroll
			.entry(work.into())
			.or_default()
			.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
		cx.notify();
	}

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
			return Err("Question history recovery incomplete");
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
			return Err("Could not load history page");
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
		self.prune_async_question_drafts(work, &history);
		self.timeline.cache.insert(work.into(), history.clone());

		self.history = Some((work.into(), history));
		self.timeline.navigation = None;

		self.timeline.follow_paused.remove(work);
		self.timeline.scroll.entry(work.into()).or_default().scroll_to_bottom();
		cx.notify();

		Ok(())
	}

	pub(super) fn history_viewport_fill_ready(&self) -> bool {
		if self.profile.is_none() || self.item_context_window_active() {
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
				&& self.timeline.native.can_prefetch_older(std::time::Instant::now());
		}

		self.history_prefetch_needed()
			&& self.timeline.older_retry_after.is_none_or(|at| at <= std::time::Instant::now())
	}

	pub(super) fn prefetch_native_history(&mut self, cx: &mut Context<Self>) -> bool {
		if self.item_context_window_active() {
			return true;
		}
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
			&& self.timeline.native.can_prefetch_older(std::time::Instant::now())
		{
			self.load_native_timeline_page(&binding.work, &binding.thread, true, underfilled, cx);
		}

		true
	}

	pub(super) fn refresh_open_native_history(&mut self, cx: &mut Context<Self>) {
		if !self.read_connection_ready() {
			return;
		}

		let Some((work, thread)) =
			self.conversation_work().and_then(|work| Some((work.id, work.codex_thread_id?)))
		else {
			self.timeline.native.reset();

			return;
		};

		let located = self.item_history_location_active();
		if !located {
			self.activate_native_timeline(&work, &thread);
		}
		// Local delivery evidence does not depend on native history or its account binding.
		self.refresh_native_input_receipts(cx);
		if located {
			return;
		}

		let selected = self.conversation_work();
		let turn = selected.as_ref().and_then(|work| work.active_turn_id.as_deref());

		self.timeline.native.retry_after_turn_change(turn);

		if self.timeline.native.can_retry(std::time::Instant::now()) {
			self.load_native_timeline(&work, &thread, false, cx);
		}
	}

	fn activate_native_timeline(&mut self, work: &str, thread: &str) {
		let target = (work.to_owned(), thread.to_owned());

		if self.timeline.native.requested.as_ref() == Some(&target) {
			return;
		}

		self.timeline.native.cancel_read();

		let epoch = self.timeline.native.epoch;
		let next = self.timeline.native_cache.remove(&target).unwrap_or_default();
		let previous = mem::replace(&mut self.timeline.native, next);

		if let Some(binding) = &previous.binding {
			self.timeline
				.native_cache
				.insert((binding.work.clone(), binding.thread.clone()), previous);
		}

		// Only a new conversation starts at the bottom. Reopening retains the
		// verified page and viewport while the ordinary refresh runs in the background.
		if self.timeline.native.binding.is_none() {
			self.timeline.native.viewport.request_latest();
		}

		self.timeline.native.epoch = epoch;
		self.timeline.native.requested = Some(target);
	}

	pub(super) fn invalidate_native_timeline(&mut self, snapshot: &AgentSnapshotDto) {
		if self.snapshot.as_ref().and_then(|old| old.runtime_source.as_ref())
			!= snapshot.runtime_source.as_ref()
		{
			self.timeline.native.reset();
			self.timeline.native_cache.clear();
		}

		self.timeline.native_cache.retain(|(id, thread), _| {
			snapshot
				.work_items
				.iter()
				.any(|work| &work.id == id && work.codex_thread_id.as_ref() == Some(thread))
		});
	}

	pub(super) fn native_history_active(&self, work: &AgentWorkItemDto) -> bool {
		!self.timeline.native.show_saved
			&& self.timeline.native.binding.as_ref().is_some_and(|binding| {
				binding.work == work.id && Some(&binding.thread) == work.codex_thread_id.as_ref()
			})
	}

	/// Native turn completion can arrive before the next work-state snapshot.
	pub(super) fn current_turn_completed(&self, work: &AgentWorkItemDto) -> bool {
		self.native_history_active(work)
			&& work.active_turn_id.as_ref().is_some_and(|turn| {
				self.timeline.native.entries.iter().rev().any(|entry| {
					matches!(&entry.content, AgentTimelineContent::TurnBoundary {
						turn_id, completed: true, ..
					} if turn_id == turn)
				})
			})
	}

	pub(super) fn response_in_progress(&self, work: &AgentWorkItemDto) -> bool {
		matches!(
			work.dispatch_state,
			super::AgentDispatchStateDto::Running | super::AgentDispatchStateDto::Dispatching
		) && !self.current_turn_completed(work)
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
		let mut controls = gpui::div().flex().flex_col().gap_2().child(
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
	) -> Option<AnyElement> {
		let mut panel = gpui::div().flex().flex_col().gap(gpui::px(ROW_GAP));
		// The history surface owns loading feedback and its handoff to content.
		let notice = self.timeline.native.notice.filter(|_| {
			self.timeline.native.requested.as_ref().is_some_and(|(id, thread)| {
				id == &work.id && Some(thread) == work.codex_thread_id.as_ref()
			})
		});

		if let Some(message) = notice {
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
				return Some(
					panel
						.child(agent_surface::muted(
							"Saved local records may include earlier thread bindings and delivery receipts.",
						))
						.into_any_element(),
				);
			}
			if self.timeline.native.browsing_window {
				panel = panel.child(agent_surface::muted(
					"Earlier history window · Open Details to refresh.",
				));
			}
			if self.timeline.native.opening_session.is_some() {
				panel = panel.child(agent_surface::muted(
					"Voice conversation continues from the previous page.",
				));
			}
			if !self.timeline.native.summary.is_empty() {
				panel = panel.child(self.native_summary_exchanges(work, cx));
			}

			panel = self.append_native_history_rows(panel, work, cx);
			panel = panel.children(self.send_previews(&work.id));
			panel = panel.children(self.native_live_rows(work));
		}
		if !self.native_history_active(work) && !self.native_history_loading(work) {
			// An absent source must not insert an empty row and an extra message gap.
			return notice.map(|_| panel.into_any_element());
		}

		Some(
			ui_loading::history(
				format!("native-history-{}", work.id),
				self.native_history_active(work),
				self.timeline.native.task.is_none()
					&& !self.native_pagination_settling()
					&& !self.history_viewport_fill_ready(),
				self.timeline.scroll.get(&work.id).cloned().unwrap_or_default(),
				panel,
			)
			.with_arrival(self.timeline.native.arrival.clone())
			.into_any_element(),
		)
	}

	pub(super) fn native_pagination_feedback(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
		if self.item_context_window_active() {
			return None;
		}
		let binding = self.timeline.native.binding.as_ref().filter(|binding| {
			!self.timeline.native.show_saved && self.selected.as_ref() == Some(&binding.work)
		})?;
		let feedback = if self.timeline.native.pagination == Pagination::Failed {
			let binding = binding.clone();

			self.workspace_action(
				"native-timeline-retry".into(),
				"Retry".into(),
				move |s, cx| s.load_native_timeline(&binding.work, &binding.thread, true, cx),
				cx,
			)
		} else {
			ui_loading::pagination(self.timeline.native.pagination == Pagination::Loading)
				.into_any_element()
		};

		Some(
			gpui::div()
				.id(SharedString::from(format!(
					"history-feedback-{}",
					self.conversation_page().unwrap_or_default()
				)))
				.absolute()
				.top(gpui::px(4.))
				.right(gpui::px(8.))
				.child(feedback)
				.into_any_element(),
		)
	}

	fn process_history_body(
		&self,
		work: &AgentWorkItemDto,
		group: &groups::Group,
		entry: &AgentTimelineEntry,
		cx: &mut Context<Self>,
	) -> Div {
		let header = self.turn_process_header(work, group, entry, cx);

		if let Some(motion) = self.timeline.native.completions.get(group)
			&& !group
				.indices
				.iter()
				.any(|index| self.is_revealed_history_item(&self.timeline.native.entries[*index]))
		{
			let indices: Vec<_> = group
				.indices
				.iter()
				.copied()
				.filter(|i| motion.outgoing.contains(&self.timeline.native.entries[*i].position))
				.collect();
			let body = self.process_history_content(work, &indices, entry, false, cx);

			return gpui::div().w_full().child(Closing {
				motion,
				position: entry.position,
				header,
				body,
			});
		}

		let owner = cx.entity();
		let indices = group.indices.clone();
		let anchor = entry.clone();
		let source_work = work.clone();

		gpui::div().w_full().debug_selector(|| "turn-process-block".into()).child(header).child(
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
						s.process_history_content(&source_work, &indices, &anchor, true, cx)
					})
				},
			),
		)
	}

	fn process_history_content(
		&self,
		work: &AgentWorkItemDto,
		indices: &[usize],
		anchor: &AgentTimelineEntry,
		archived: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut waits = waits::groups(&self.timeline.native.entries, indices);
		waits.retain(|_, group| {
			!group
				.indices
				.iter()
				.any(|index| self.is_revealed_history_item(&self.timeline.native.entries[*index]))
		});
		let hidden: BTreeSet<_> =
			waits.values().flat_map(|group| group.indices.iter().skip(1)).collect();
		let body = gpui::div().w_full().min_w_0().flex().flex_col().gap_2().children(
			indices.iter().filter(|i| !hidden.contains(i)).map(|i| {
				if let Some(group) = waits.get(i) {
					self.native_wait_group(work, group, anchor, archived, cx)
				} else {
					let entry = &self.timeline.native.entries[*i];

					let body = self.native_timeline_content(
						work,
						entry,
						&serde_json::json!([work.id, work.codex_thread_id, key(entry)]).to_string(),
						archived && !self.is_located_item(&entry.content),
						cx,
					);
					if self.is_revealed_history_item(entry) {
						self.native_scroll_row(work, entry, body, cx)
					} else {
						body
					}
				}
			}),
		);

		if archived { render::process_indent(body) } else { body.into_any_element() }
	}

	fn native_voice_history_content(
		&self,
		identity: String,
		empty_key: String,
		title: &str,
		text: &str,
		empty_status: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if text.trim().is_empty() {
			gpui::div()
				.id(SharedString::from(format!("voice-status-{empty_key}")))
				.role(Role::Status)
				.child(agent_surface::muted(empty_status.to_owned()))
				.into_any_element()
		} else {
			VoiceBlock {
				viewport: Some(self.voice_history_viewport(cx)),
				key: identity,
				title: title.into(),
				text: text.to_owned(),
			}
			.into_any_element()
		}
	}

	fn replied_native_turns(&self) -> BTreeSet<&String> {
		self.timeline
			.native
			.entries
			.iter()
			.filter_map(|entry| match &entry.content {
				AgentTimelineContent::Item { turn_id, kind, .. } if kind == "agentMessage" =>
					Some(turn_id),
				_ => None,
			})
			.collect()
	}

	fn append_native_history_rows(
		&self,
		mut panel: Div,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Div {
		let mut groups =
			groups::groups(&self.timeline.native.entries, &self.timeline.native.expanded_turns);
		for group in &mut groups {
			group.expanded |= group
				.indices
				.iter()
				.any(|index| self.is_revealed_history_item(&self.timeline.native.entries[*index]));
		}
		let mut collapsed = BTreeSet::new();
		let mut headers = std::collections::BTreeMap::new();

		for group in &groups {
			headers.insert(group.first_index, group);
			collapsed.extend(group.indices.iter().copied());
		}

		let mut waits = self.native_wait_groups();
		waits.retain(|_, group| {
			!group
				.indices
				.iter()
				.any(|index| self.is_revealed_history_item(&self.timeline.native.entries[*index]))
		});
		let wait_hidden: BTreeSet<_> =
			waits.values().flat_map(|group| group.indices.iter().skip(1).copied()).collect();

		self.prepare_native_process_folds(&groups, &collapsed, &wait_hidden, &waits);

		let mut voice_groups = history::groups(&self.timeline.native.entries);
		voice_groups.retain(|group| {
			group.hidden()
				|| !group.indices.iter().any(|index| {
					self.is_revealed_history_item(&self.timeline.native.entries[*index])
				})
		});
		let (voice_headers, voice_hidden) = Self::voice_group_indices(&voice_groups);
		let voice_tail_turns = history::tail_flush_turns(&self.timeline.native.entries);
		let hidden_items = groups::hidden_items(&self.timeline.native.entries);
		let replied_turns = self.replied_native_turns();
		let mut hidden = Vec::new();

		for (index, entry) in self.timeline.native.entries.iter().enumerate() {
			if let Some(group) = voice_headers.get(&index) {
				let entry = &self.timeline.native.entries
					[group.anchor_index(&self.timeline.native.entries)];

				if group.hidden() || self.live_voice_owns_session(&work.id, &group.session) {
					continue;
				}
				if !hidden.is_empty() {
					panel = panel.child(self.native_history_spacer(work, mem::take(&mut hidden)));
				}

				let body = self.native_voice_group_body(work, group, entry, cx);

				panel = panel.child(self.native_scroll_row(work, entry, body, cx));

				continue;
			}

			if voice_hidden.contains(&index)
				|| hidden_items.contains(&index)
				|| matches!(
					&entry.content,
					AgentTimelineContent::TurnBoundary { completed: false, .. }
				) {
				continue;
			}
			if matches!(&entry.content,
				AgentTimelineContent::TurnBoundary { completed: true, turn_id, status, error: None, .. }
				if (replied_turns.contains(turn_id) || voice_tail_turns.contains(turn_id.as_str()))
					&& !matches!(status.as_deref(), Some("interrupted" | "failed")))
			{
				continue;
			}

			if let Some(group) = headers.get(&index) {
				if !hidden.is_empty() {
					panel = panel.child(self.native_history_spacer(work, mem::take(&mut hidden)));
				}

				let mut body = self.process_history_body(work, group, entry, cx);

				if !group.indices.contains(&index) {
					body = body.child(self.native_timeline_content(
						work,
						entry,
						&serde_json::json!([work.id, work.codex_thread_id, key(entry)]).to_string(),
						false,
						cx,
					));
				}

				panel =
					panel.child(self.native_scroll_row(work, entry, body.into_any_element(), cx));

				continue;
			}

			if collapsed.contains(&index) || wait_hidden.contains(&index) {
				continue;
			}

			if let Some(group) = waits.get(&index) {
				if !hidden.is_empty() {
					panel = panel.child(self.native_history_spacer(work, mem::take(&mut hidden)));
				}

				let body = self.native_wait_group(work, group, entry, false, cx);

				panel = panel.child(self.native_timeline_row_with_content(work, entry, body, cx));

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

	fn prepare_native_process_folds(
		&self,
		groups: &[groups::Group],
		collapsed: &BTreeSet<usize>,
		wait_hidden: &BTreeSet<usize>,
		waits: &std::collections::BTreeMap<usize, waits::Group>,
	) {
		let folds = collapsed
			.iter()
			.copied()
			.chain(wait_hidden.iter().copied())
			.chain(waits.keys().copied())
			.collect();

		self.prepare_process_folds(groups, &folds);
	}

	fn native_wait_groups(&self) -> std::collections::BTreeMap<usize, waits::Group> {
		waits::groups(
			&self.timeline.native.entries,
			&(0..self.timeline.native.entries.len()).collect::<Vec<_>>(),
		)
	}

	fn voice_group_indices(
		groups: &[Conversation],
	) -> (std::collections::BTreeMap<usize, &Conversation>, BTreeSet<usize>) {
		let voice_headers: std::collections::BTreeMap<_, _> =
			groups.iter().map(|g| (g.indices[0], g)).collect();
		let voice_hidden: BTreeSet<_> =
			groups.iter().flat_map(|g| g.indices.iter().skip(1).copied()).collect();

		(voice_headers, voice_hidden)
	}

	fn native_voice_group_body(
		&self,
		work: &AgentWorkItemDto,
		group: &Conversation,
		entry: &AgentTimelineEntry,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let title =
			if group.failed { "Voice conversation · Failed" } else { "Voice conversation" };
		let body = self.native_voice_history_content(
			serde_json::json!([
				work.id,
				work.codex_thread_id,
				group.identity(&self.timeline.native.entries)
			])
			.to_string(),
			serde_json::json!([work.id, key(entry)]).to_string(),
			title,
			&group.text,
			group.empty_status(),
			cx,
		);

		self.anchored_native_history_entry(work, entry, body)
	}

	fn read_latest_native_history(&mut self, work: &str, thread: &str, cx: &mut Context<Self>) {
		self.reset_item_context();
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

	fn refresh_native_summary(
		&mut self,
		binding: Binding,
		items: Vec<AgentTimelineContent>,
	) -> bool {
		let jump = self.timeline.native.viewport.take_latest_request();

		if !jump
			&& self.timeline.native.binding.as_ref() == Some(&binding)
			&& self.timeline.native.summary == items
			&& self.timeline.native.notice == Some(SUMMARY_NOTICE)
		{
			self.timeline.native.retry_at =
				Some(std::time::Instant::now() + Duration::from_secs(30));

			return false;
		}

		let work = binding.work.clone();

		self.cancel_native_scroll_anchor();
		self.timeline.native.accept_summary(binding, items);

		if jump {
			self.timeline.scroll.entry(work).or_default().scroll_to_bottom();
		}

		true
	}

	fn refresh_native_history(&mut self, binding: Binding, page: AgentTimelinePage) -> bool {
		let jump = self.timeline.native.viewport.take_latest_request();
		let work = binding.work.clone();

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

	pub(super) fn load_native_timeline(
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
		if self.item_context_window_active() {
			return;
		}
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
			self.timeline.native.pagination = Pagination::Loading;
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

		if !older {
			// Native-only conversations return before the legacy history reader.
			// They must still advance the shared background refresh timestamp.
			self.timeline.read_at = Some(std::time::Instant::now());
		}

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

				s.finish_native_timeline_read(
					result,
					(work, thread),
					sent_cursor,
					fill_viewport,
					cx,
				);
			});
		}));

		if older || self.conversation_work().is_some_and(|work| self.native_history_loading(&work))
		{
			cx.notify();
		}
	}

	fn finish_native_timeline_read(
		&mut self,
		result: Option<decodex_protocol::AgentTimelineResult>,
		target: (String, String),
		sent_cursor: Option<String>,
		fill_viewport: bool,
		cx: &mut Context<Self>,
	) {
		let (work, thread) = target;

		if self.selected.as_deref() != Some(&work) || !self.conversation_matches(&work, &thread) {
			return;
		}

		self.timeline.native.task = None;

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
			let changed = self.refresh_native_summary(
				Binding {
					work: work.clone(),
					thread: thread.clone(),
					account: account_id.as_str().into(),
				},
				items.clone(),
			);

			if changed {
				cx.notify();
			}

			self.resume_list_branch_review(false, cx);
			return;
		}

		let mut changed = true;

		if let Some(decodex_protocol::AgentTimelineResult::Available { account_id, page, .. }) =
			result
		{
			let binding = Binding { work, thread, account: account_id.as_str().into() };

			changed = sent_cursor.is_some() || !self.timeline.native.matches_page(&binding, &page);

			let accepted = match sent_cursor {
				Some(cursor) => {
					let accepted = self.prepend_native_history(&binding, &cursor, page);

					if accepted
						&& fill_viewport
						&& !self.timeline.follow_paused.contains(&binding.work)
					{
						self.cancel_native_scroll_anchor();

						if let Some(scroll) = self.timeline.scroll.get(&binding.work) {
							scroll.scroll_to_bottom();
						}
					}

					accepted
				},
				None => self.refresh_native_history(binding, page),
			};

			if accepted {
				changed |= self.retire_native_output();

				self.timeline.native.recovered();
				self.observe_prompt_completion(cx);
				self.resume_list_branch_review(true, cx);
			} else if self.timeline.native.pagination == Pagination::Loading {
				changed = true;

				self.timeline.native.failed(None, std::time::Instant::now());
				self.resume_list_branch_review(false, cx);
			} else {
				changed = true;
				self.timeline.native.safety_buffering_turn_id = None;
				self.timeline.native.notice =
					Some("History changed or display limit reached. Refresh history.");
				self.resume_list_branch_review(false, cx);
			}
		} else {
			self.timeline.native.failed(result, std::time::Instant::now());
			self.resume_list_branch_review(false, cx);
		}

		if changed {
			if self.timeline.native.find.searching_earlier {
				self.timeline.native.find.searching_earlier = false;
				self.timeline.native.find.current = None;
				self.select_conversation_match(false, cx);
			}
			cx.notify();
		}
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
	find: find::Panel,
	arrival: ContentArrival,
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
	summary_focus: RefCell<std::collections::BTreeMap<String, FocusHandle>>,
	pub older_cursor: Option<String>,
	pub prefetch_requested: bool,
	pagination: Pagination,
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
	arrivals: Arrivals,
	completions: Completions,
	notice: Option<&'static str>,
	seen_cursors: BTreeSet<String>,
}
impl Timeline {
	fn matches_page(&self, binding: &Binding, page: &AgentTimelinePage) -> bool {
		self.binding.as_ref() == Some(binding)
			&& page.thread_id == binding.thread
			&& self.entries == page.entries
			&& self.summary.is_empty()
			&& self.weather == page.weather
			&& self.safety_buffering_turn_id == page.safety_buffering_turn_id
			&& self.older_cursor == page.next_cursor
			&& self.opening_session == page.active_realtime_session_at_page_start
			&& self.notice.is_none()
			&& self.pagination == Pagination::Idle
			&& !self.viewport.latest_requested()
	}

	pub(super) fn cancel_read(&mut self) {
		self.task = None;

		self.input_receipts.cancel_read();

		self.epoch = self.epoch.wrapping_add(1);

		if self.pagination == Pagination::Loading {
			self.pagination = Pagination::Idle;
		}
	}

	pub(super) fn invalidate_read(&mut self) {
		self.cancel_read();

		self.pagination = Pagination::Idle;

		self.recovered();
	}

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

	fn can_prefetch_older(&self, now: std::time::Instant) -> bool {
		// An older display window can still page backward after its newest rows are trimmed.
		self.pagination == Pagination::Idle && self.retry_at.is_none_or(|at| now >= at)
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
		if self.pagination == Pagination::Loading {
			// Keep the verified page and cursor so the reader can retry in place.
			self.pagination = Pagination::Failed;

			return;
		}

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
			"Codex history unsupported. Showing saved local records."
		} else {
			"Codex history unavailable. Showing local records while retrying…"
		});
	}

	fn accept_summary(&mut self, binding: Binding, items: Vec<AgentTimelineContent>) {
		self.clear_page();

		self.binding = Some(binding);
		self.summary = items;

		self.recovered();

		self.retry_at = Some(std::time::Instant::now() + Duration::from_secs(30));
		self.notice = Some(SUMMARY_NOTICE);
	}

	fn clear_page(&mut self) {
		self.summary_focus.borrow_mut().clear();

		self.revision = self.revision.wrapping_add(1);

		self.preview.clear();

		self.viewport = Default::default();
		self.arrivals = Default::default();
		self.completions = Default::default();
		self.binding = None;

		self.entries.clear();
		self.weather.clear();

		self.safety_buffering_turn_id = None;

		self.summary.clear();

		self.older_cursor = None;
		self.pagination = Pagination::Idle;
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
			self.arrivals.observe(&page.entries, !ui_motion::reduced());

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

		// An explicit read or a gap replaces the viewport, not an appended live row.
		self.arrivals = Default::default();

		self.completions.replace_history(self.binding.as_ref() == Some(&binding));
		self.arrivals.observe(&page.entries, false);

		self.binding = Some(binding);
		self.safety_buffering_turn_id = page.safety_buffering_turn_id;
		self.weather = page.weather;
		self.entries = page.entries;
		self.older_cursor = page.next_cursor;
		self.pagination = Pagination::Idle;
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
		self.pagination = Pagination::Idle;

		self.viewport.retain(&self.entries);

		true
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Pagination {
	#[default]
	Idle,
	Loading,
	Failed,
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

	use std::{future, thread};

	use futures_util::{SinkExt as _, StreamExt as _};
	use gpui::AppContext as _;
	use tokio_tungstenite::tungstenite::Message;

	use crate::shell::agent_surface::native_timeline::{
		AgentHistoryResult, AgentSnapshotResult, AgentSurface, AgentTimelineContent,
		AgentTimelineEntry, AgentTimelinePage, Binding, EntityId, Pagination, Timeline,
		wire_test_support,
	};
	use decodex_protocol::{
		AgentTimelineResult, CURRENT_VERSION, ClientMessage, QueryPayload, QueryResultEnvelope,
		QueryResultPayload, ServerId, ServerMessage,
	};

	#[gpui::test]
	fn confirmed_native_history_is_readable_while_execution_recovery_is_pending(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			let snapshot = s.snapshot.as_mut().unwrap();

			snapshot.connection_initializing = true;
			snapshot.runtime_source = None;

			let work = snapshot
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("selected-native-history".into());

			let work = work.id.clone();

			s.composer.update(cx, |input, cx| input.set_content("Keep my draft", cx));
			s.load_history(cx);

			assert!(s.workspace_connecting());
			assert!(s.timeline.native.requested.is_none());

			s.snapshot.as_mut().unwrap().runtime_source =
				Some(EntityId::new("confirmed-source").unwrap());

			s.load_history(cx);

			assert!(!s.workspace_connecting());
			assert!(s.read_connection_ready());
			assert_eq!(s.timeline.native.requested, Some((work, "selected-native-history".into())));
			assert!(!s.command_connection_ready());

			s.submit(cx);

			assert!(!s.sending);
			assert!(s.submission.command.is_none());
			assert_eq!(s.composer.read(cx).content(), "Keep my draft");

			cx.notify();
		});

		visual.update(|window, cx| window.draw(cx).clear(cx));

		assert!(visual.debug_bounds("workspace-connecting").is_none());
		assert!(visual.debug_bounds("workspace-transcript").is_some());
	}

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

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(
			visual.debug_bounds("loading-feedback-Loading conversation").is_none(),
			"brief reads do not flash a loader"
		);

		thread::sleep(std::time::Duration::from_millis(390));

		// Deliver the requested animation frame, as the native window does.
		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_some());
		assert!(visual.debug_bounds("saved-local-history").is_none());

		// Workspace connection feedback takes precedence over history loading.
		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().connection_initializing = true;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_none());
		assert!(visual.debug_bounds("loading-feedback-Opening workspace").is_some());

		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().connection_initializing = false;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_some());
		assert!(visual.debug_bounds("loading-feedback-Opening workspace").is_none());

		// A failed native read still permits the saved-history fallback.
		surface.update(visual, |s, cx| {
			s.timeline.native.task = None;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_none());
		assert!(visual.debug_bounds("saved-local-history").is_some());

		// Retrying a failed read must retain the fallback, too.
		surface.update(visual, |s, cx| {
			s.timeline.native.failed(None, std::time::Instant::now());

			s.timeline.native.task = Some(cx.spawn(async |_, _| future::pending::<()>().await));

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_none());
		assert!(visual.debug_bounds("saved-local-history").is_some());
	}

	#[gpui::test]
	fn earlier_history_stays_quiet_and_retries_the_failed_cursor(cx: &mut gpui::TestAppContext) {
		let (_root, profile, server) = wire_test_support::fixture(serve_older_cursor);
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(700.)));

		let binding = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
			s.profile = None;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());

			let binding = Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			};

			assert!(
				s.timeline
					.native
					.replace(binding.clone(), page(vec![boundary(10, true)], Some("older"), None))
			);

			cx.notify();

			binding
		});

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("native-timeline-older").is_none());

		surface.update(visual, |s, cx| {
			s.profile = Some(profile);
			s.timeline.native.prefetch_requested = true;

			s.timeline.follow_paused.insert(binding.work.clone());
			s.timeline.scroll[&binding.work].set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));

			// The same automatic path must work while browsing beyond the retained window.
			s.timeline.native.browsing_window = true;

			assert!(s.prefetch_native_history(cx));
			assert_eq!(s.timeline.native.pagination, Pagination::Loading);
		});

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("loading-feedback-Loading earlier history").is_none());

		for delay in [300, 16] {
			thread::sleep(std::time::Duration::from_millis(delay));

			visual.update(crate::ui_frame::draw_frame);
		}

		assert!(visual.debug_bounds("loading-feedback-Loading earlier history").is_some());

		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);

		surface.update(visual, |s, cx| {
			assert_eq!(s.timeline.native.entries, vec![boundary(10, true)]);
			assert_eq!(s.timeline.native.binding.as_ref(), Some(&binding));
			assert_eq!(s.timeline.native.older_cursor.as_deref(), Some("older"));
			assert_eq!(s.timeline.native.pagination, Pagination::Failed);
			assert!(s.timeline.native.notice.is_none());
			assert!(!s.history_viewport_fill_ready());

			s.timeline.native.prefetch_requested = true;

			s.prefetch_native_history(cx);

			assert!(s.timeline.native.task.is_none(), "failure waits for an explicit retry");
		});

		assert!(visual.debug_bounds("loading-feedback-Loading earlier history").is_none());

		let retry = visual.debug_bounds("native-timeline-retry").unwrap();

		visual.simulate_click(retry.center(), Default::default());
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);

		assert_eq!(server.join().unwrap(), vec!["older", "older"]);

		surface.read_with(visual, |s, _| {
			assert_eq!(s.timeline.native.entries, vec![boundary(9, false), boundary(10, true)]);
			assert_eq!(s.timeline.native.pagination, Pagination::Idle);
			assert!(s.timeline.native.older_cursor.is_none());
		});

		assert!(visual.debug_bounds("loading-feedback-Loading earlier history").is_none());
		assert!(visual.debug_bounds("native-timeline-retry").is_none());
	}

	#[gpui::test]
	fn summary_turns_keep_steering_order_with_right_inputs_and_full_width_answers(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());

			let binding = Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			};

			s.timeline.native.requested = Some((work.id.clone(), "thread".into()));

			let entries = [
				("one", "request", "userMessage", "First request"),
				("one", "steering", "userMessage", "Keep the first request"),
				("one", "answer", "agentMessage", "First answer"),
				("two", "next", "userMessage", "Second request"),
				("two", "answer", "agentMessage", "Second answer"),
			]
			.into_iter()
			.map(|(turn, item, kind, text)| AgentTimelineContent::Item {
				turn_id: turn.into(),
				item_id: item.into(),
				kind: kind.into(),
				text: text.into(),
				phase: None,
				app_ui: false,
				truncated: false,
				activity: None,
				attachments: vec![],
				collaboration: None,
			})
			.collect();

			s.timeline.native.accept_summary(binding, entries);
			cx.notify();
		});

		for width in [1_400., 1_800.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(1_400.)));
			visual.update(crate::ui_frame::draw_frame);

			let first = visual.debug_bounds("native-summary-exchange-one-request").unwrap();
			let next = visual.debug_bounds("native-summary-exchange-two-next").unwrap();
			let footer = visual.debug_bounds("floating-composer").unwrap();

			assert!(visual.debug_bounds("conversation-history-rail").is_none());
			assert!(
				(first.center().x - footer.center().x).abs() <= gpui::px(1.),
				"an unavailable history navigator must not offset the conversation"
			);

			let gap = gpui::px(crate::ui_theme::MESSAGE_GAP);

			assert!((next.top() - first.bottom() - gap).abs() <= gpui::px(1.));

			let request_row = visual.debug_bounds("native-summary-content-one-request").unwrap();
			let steering_row = visual.debug_bounds("native-summary-content-one-steering").unwrap();
			let answer_row = visual.debug_bounds("native-summary-content-one-answer").unwrap();

			assert_eq!(request_row.top(), first.top(), "input must not add a second gap");
			assert!((steering_row.top() - request_row.bottom() - gap).abs() <= gpui::px(1.));
			assert!((answer_row.top() - steering_row.bottom() - gap).abs() <= gpui::px(1.));

			let request =
				visual.debug_bounds(r#"["summary","agent","thread","one","request"]-0"#).unwrap();
			let steering =
				visual.debug_bounds(r#"["summary","agent","thread","one","steering"]-0"#).unwrap();
			let answer =
				visual.debug_bounds(r#"["summary","agent","thread","one","answer"]-0"#).unwrap();

			assert!(request.top() < steering.top() && steering.top() < answer.top());
			assert!(request.left() > answer.left() + gpui::px(50.));
			assert!((f32::from(first.right() - request.right()) - 12.).abs() <= 1.);
			assert!((f32::from(first.right() - steering.right()) - 12.).abs() <= 1.);
			assert!((f32::from(first.left() - answer.left())).abs() <= 1.);
			assert!((f32::from(first.size.width - answer.size.width)).abs() <= 1.);
			assert!(first.size.width > gpui::px(600.), "do not restore the narrow column");
		}

		surface.read_with(visual, |s, _| assert_eq!(s.timeline.native.summary.len(), 5));
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

		visual.update(crate::ui_frame::draw_current_frame);

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
		visual.update(crate::ui_frame::draw_current_frame);

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
		assert!(state.notice.expect("summary notice").contains("Intermediate activity"));
		assert!(state.refresh(
			binding,
			AgentTimelinePage {
				thread_id: "thread".into(),
				entries: vec![AgentTimelineEntry { position: 77, content: item }],
				next_cursor: Some("real-cursor".into()),
				weather: Default::default(),
				thread_token_total: None,
				thread_token_breakdown: None,
				thread_metrics: None,
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
	fn repeated_summary_preserves_layout_until_content_or_navigation_changes(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, _| {
			let item = AgentTimelineContent::Item {
				turn_id: "turn".into(),
				item_id: "answer".into(),
				kind: "agentMessage".into(),
				text: "A quiet reading plan".into(),
				phase: None,
				app_ui: false,
				truncated: false,
				activity: None,
				attachments: vec![],
				collaboration: None,
			};
			let mut items = vec![item];

			assert!(s.refresh_native_summary(binding(), items.clone()));

			let revision = s.timeline.native.revision;

			for _ in 0..4 {
				assert!(!s.refresh_native_summary(binding(), items.clone()));
			}

			assert_eq!(
				s.timeline.native.revision, revision,
				"identical polls must not discard measured rows or scroll anchors"
			);

			if let AgentTimelineContent::Item { text, .. } = &mut items[0] {
				text.push_str(" updated");
			}

			assert!(s.refresh_native_summary(binding(), items.clone()));
			assert_ne!(s.timeline.native.revision, revision);

			s.timeline.native.viewport.request_latest();

			assert!(
				s.refresh_native_summary(binding(), items.clone()),
				"explicit navigation must still repaint"
			);

			s.timeline.native.notice = Some("Retrying native history");

			assert!(s.refresh_native_summary(binding(), items), "recovery must still repaint");
		});
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
				expired_question_ids: Vec::new(),
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

	#[gpui::test]
	fn returning_to_an_open_conversation_keeps_verified_native_history(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			for work in &mut s.snapshot.as_mut().unwrap().work_items {
				work.codex_thread_id = Some(format!("thread-{}", work.id));
			}

			s.activate_native_timeline("agent", "thread-agent");

			let mut first = page(vec![boundary(1, true)], Some("older"), None);

			first.thread_id = "thread-agent".into();

			assert!(s.timeline.native.replace(
				Binding {
					work: "agent".into(),
					thread: "thread-agent".into(),
					account: "account".into(),
				},
				first,
			));

			let revision = s.timeline.native.revision;
			let epoch = s.timeline.native.epoch;

			s.timeline.native.task = Some(cx.spawn(async |_, _| future::pending::<()>().await));

			s.open_page("release", cx);

			assert!(s.timeline.native.binding.is_none());
			assert!(
				s.timeline.native_cache[&("agent".into(), "thread-agent".into())].task.is_none()
			);

			s.open_page("agent", cx);

			assert_eq!(s.timeline.native.entries, vec![boundary(1, true)]);
			assert_eq!(s.timeline.native.older_cursor.as_deref(), Some("older"));
			assert_eq!(s.timeline.native.revision, revision);
			assert!(s.timeline.native.epoch > epoch);
			assert!(!s.native_history_loading(&s.conversation_work().unwrap()));

			// Rebinding and changing the runtime cannot reveal another source's cache.
			s.open_page("release", cx);

			let mut snapshot = s.snapshot.clone().unwrap();

			snapshot.work_items[0].codex_thread_id = Some("replacement".into());

			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot.clone())));

			assert!(s.timeline.native_cache.is_empty());

			s.open_page("agent", cx);

			assert!(s.timeline.native.entries.is_empty());

			s.timeline
				.native_cache
				.insert(("agent".into(), "replacement".into()), Timeline::default());

			snapshot.runtime_source = Some(EntityId::new("another-runtime").unwrap());

			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot)));

			assert!(s.timeline.native_cache.is_empty());
		});
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
				timing: None,
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
			thread_token_total: None,
			thread_token_breakdown: None,
			thread_metrics: None,
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
	fn identical_pages_stay_quiet_but_completion_buffering_and_recovery_repaint() {
		let mut state = Timeline::default();
		let mut current = page(vec![boundary(1, false)], None, None);

		assert!(state.replace(binding(), current.clone()));
		assert!(state.matches_page(&binding(), &current));

		current.entries = vec![boundary(1, true)];

		assert!(!state.matches_page(&binding(), &current));
		assert!(state.refresh(binding(), current.clone()));
		assert!(state.matches_page(&binding(), &current));

		current.safety_buffering_turn_id = Some("turn".into());

		assert!(!state.matches_page(&binding(), &current));
		assert!(state.refresh(binding(), current.clone()));

		state.notice = Some("Retrying native history");

		assert!(!state.matches_page(&binding(), &current));

		state.recovered();
		state.viewport.request_latest();

		assert!(!state.matches_page(&binding(), &current));
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
		assert!(state.can_prefetch_older(std::time::Instant::now()));

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

	async fn serve_older_cursor(listener: tokio::net::UnixListener) -> Vec<String> {
		let mut cursors = Vec::new();

		while cursors.len() < 2 {
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("text query")
			};
			let ClientMessage::Query(query) = serde_json::from_str::<ClientMessage>(&text).unwrap()
			else {
				panic!("query")
			};
			let QueryPayload::GetAgentTimeline { work_id, thread_id, cursor } = query.payload
			else {
				socket.close(None).await.unwrap();

				continue;
			};

			assert_eq!(thread_id.as_str(), "thread");

			cursors.push(cursor.unwrap().as_str().to_owned());

			let result = if cursors.len() == 1 {
				AgentTimelineResult::Unavailable
			} else {
				AgentTimelineResult::Available {
					work_id,
					account_id: EntityId::new("account").unwrap(),
					page: page(vec![boundary(9, false)], None, None),
				}
			};
			let message = ServerMessage::QueryResult(QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(wire_test_support::SERVER).unwrap(),
				query_id: query.query_id,
				payload: QueryResultPayload::AgentTimeline(result),
			});

			socket
				.send(Message::Text(serde_json::to_string(&message).unwrap().into()))
				.await
				.unwrap();
		}

		cursors
	}
}
#[cfg(test)]
#[path = "agent_conversation_review.rs"]
mod conversation_review;
