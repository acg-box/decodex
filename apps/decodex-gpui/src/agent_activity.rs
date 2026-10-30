//! Compact, anchored navigation through saved conversation turns.
use std::{
	cell::Cell,
	collections::{BTreeMap, BTreeSet},
	fmt::{Display, Formatter},
	rc::Rc,
	time::Duration,
};

use gpui::{
	AnyElement, AppContext as _, Div, FontWeight, KeyDownEvent, Pixels, Role, ScrollEvent,
	ScrollHandle, Stateful,
};
use ui_theme::{BLUE, BODY_LINE_HEIGHT, SURFACE_OVERLAY_MATERIAL, TEXT, TEXT_MUTED};

use crate::{
	shell::{
		agent_surface::{
			self, AgentHistoryResult, AgentSurface, AgentWorkItemDto, Context, FluentBuilder,
			InteractiveElement, IntoElement, ParentElement, Render, SharedString,
			StatefulInteractiveElement, Styled, Window, markdown, native_timeline, ui_theme,
			voice::{history, history::VoiceBlock},
			workspace,
		},
		workspace_symbols,
		workspace_symbols::Symbol,
	},
	ui_motion, ui_scroll,
	ui_theme::HISTORY_RAIL_WIDTH,
};
use decodex_protocol::{AgentHistoryEntryDto, AgentTimelineContent, AgentTimelineEntry};

const HISTORY_ANCHOR_INSET: f32 = 56.0;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum HistoryKey {
	Local(i64),
	Native { thread: String, position: u64, kind: u8, id: String },
}
impl HistoryKey {
	pub(super) fn native(thread: &str, entry: &AgentTimelineEntry) -> Self {
		let (position, kind, id) = native_timeline::key(entry);

		Self::Native { thread: thread.into(), position, kind, id: id.into() }
	}
}

impl Display for HistoryKey {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Local(id) => write!(f, "{id}"),
			Self::Native { thread, position, kind, id } =>
				write!(f, "native-{}", serde_json::json!([thread, position, kind, id])),
		}
	}
}

impl AgentSurface {
	pub(super) fn prepare_history_marks(&mut self) {
		if self.item_context_window_active() {
			self.timeline.marks.clear();
			self.timeline.marks_revision = None;
			return;
		}
		let native = self.conversation_work().is_some_and(|work| self.native_history_active(&work));
		let work = self.selected.clone().map(|work| (work, native));

		if self.timeline.marks_work != work {
			self.timeline.wheel_scroll = None;

			self.timeline.marks.clear();

			self.timeline.selected = None;
			self.timeline.hover = None;
			self.timeline.navigation = None;
			self.timeline.marks_work = work;
			self.timeline.marks_revision = None;
		}
		if native {
			self.prepare_native_history_marks();

			return;
		}

		let Some((work, AgentHistoryResult::Available { entries, .. })) =
			self.history.as_ref().filter(|(work, _)| Some(work) == self.selected.as_ref())
		else {
			self.timeline.marks.clear();

			return;
		};
		let mut saved = BTreeMap::new();

		if let Some((older, _)) = self.timeline.older_history.get(work) {
			for entry in older {
				saved.insert(entry.id, entry);
			}
		}

		for entry in entries {
			saved.insert(entry.id, entry);
		}

		self.timeline.scroll.entry(work.clone()).or_default();
		self.timeline
			.marks
			.retain(|id, _| matches!(id, HistoryKey::Local(id) if saved.contains_key(id)));

		let saved_entries = saved.values().copied().collect::<Vec<_>>();
		let voice_groups = history::saved_groups(&saved_entries);
		let voice_hidden: BTreeSet<_> = voice_groups
			.iter()
			.flat_map(|(indices, _)| indices.iter().skip(1).map(|index| saved_entries[*index].id))
			.collect();

		self.timeline
			.marks
			.retain(|key, _| !matches!(key, HistoryKey::Local(id) if voice_hidden.contains(id)));

		let mut current = None;

		for (index, entry) in saved_entries.iter().enumerate() {
			if voice_hidden.contains(&entry.id) {
				continue;
			}

			let voice = voice_groups.iter().find(|(indices, _)| indices[0] == index);

			if voice.is_some() || entry.kind == "user" || entry.kind == "instruction" {
				let mark =
					self.timeline.marks.entry(HistoryKey::Local(entry.id)).or_insert_with(|| {
						HistoryMark {
							position: Rc::new(Cell::new(f32::INFINITY)),
							hit_bounds: Rc::new(Cell::new(None)),
							question: voice.map_or_else(
								|| preview(&entry.text),
								|(_, text)| format!("Voice conversation · {}", preview(text)),
							),
							time: format!(
								"{} · {} UTC",
								time::OffsetDateTime::from_unix_timestamp(
									entry.created_at_micros / 1_000_000
								)
								.map(|t| t.date().to_string())
								.unwrap_or_default(),
								workspace::clock_label(entry.created_at_micros)
							),
							answer: String::new(),
						}
					});

				mark.answer.clear();

				current = Some(HistoryKey::Local(entry.id));
			} else if entry.kind == "assistant"
				&& let Some(id) = current.as_ref()
			{
				self.timeline.marks.get_mut(id).expect("current mark").answer =
					preview(&entry.text);
			}
		}
	}

	pub(super) fn saved_voice_history_entry(
		&self,
		entry: &AgentHistoryEntryDto,
		work: &str,
		text: String,
		cx: &mut Context<Self>,
	) -> AnyElement {
		self.anchor_history_row(
			&HistoryKey::Local(entry.id),
			VoiceBlock {
				key: format!("saved-{work}-{}", entry.id),
				title: "Voice conversation".into(),
				text,
				viewport: Some(self.voice_history_viewport(cx)),
			}
			.into_any_element(),
		)
	}

	pub(super) fn anchored_history_entry(
		&self,
		entry: &AgentHistoryEntryDto,
		work: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut body = if entry.kind == "user"
			&& let Some(text) = history::handoff(&entry.text)
		{
			VoiceBlock {
				key: format!("saved-{work}-{}", entry.id),
				title: "Voice conversation".into(),
				text,
				viewport: Some(self.voice_history_viewport(cx)),
			}
			.into_any_element()
		} else {
			agent_surface::history_entry(entry).into_any_element()
		};

		if entry.kind == "assistant"
			&& let Some(action) =
				self.voice_read_action(work, &format!("saved-{}", entry.id), &entry.text, true, cx)
		{
			body = gpui::div().child(body).child(action).into_any_element();
		}

		self.anchor_history_row(&HistoryKey::Local(entry.id), body)
	}

	fn prepare_native_history_marks(&mut self) {
		let Some(binding) = &self.timeline.native.binding else {
			return;
		};
		let revision = (self.timeline.native.epoch, self.timeline.native.revision);

		if self.timeline.marks_revision == Some(revision) {
			return;
		}

		self.timeline.marks_revision = Some(revision);

		self.timeline.scroll.entry(binding.work.clone()).or_default();

		let voice_groups = history::groups(&self.timeline.native.entries);
		let voice_headers: BTreeMap<_, _> =
			voice_groups.iter().map(|g| (g.indices[0], g)).collect();
		let voice_hidden: BTreeSet<_> =
			voice_groups.iter().flat_map(|g| g.indices.iter().skip(1).copied()).collect();
		let mut retained = BTreeSet::new();
		let mut current = None;

		for (index, entry) in self.timeline.native.entries.iter().enumerate() {
			if voice_hidden.contains(&index)
				|| voice_headers.get(&index).is_some_and(|g| g.hidden())
			{
				continue;
			}

			let entry = voice_headers.get(&index).map_or(entry, |g| {
				&self.timeline.native.entries[g.anchor_index(&self.timeline.native.entries)]
			});
			let (user, text, label) = if let Some(group) = voice_headers.get(&index) {
				(true, &group.text, "Voice conversation")
			} else {
				match &entry.content {
					AgentTimelineContent::Item { kind, text, .. }
						if matches!(
							kind.as_str(),
							"userMessage" | "agentInput" | "agentMessage"
						) =>
						(
							kind != "agentMessage",
							text,
							if kind == "agentInput" {
								"Task input"
							} else {
								"Conversation message"
							},
						),
					AgentTimelineContent::Speech { role, text, .. } =>
						(role == "user", text, "Voice message"),
					_ => continue,
				}
			};

			if user {
				let key = HistoryKey::native(&binding.thread, entry);

				retained.insert(key.clone());

				let mark = self.timeline.marks.entry(key.clone()).or_insert_with(|| HistoryMark {
					position: Rc::new(Cell::new(0.0)),
					hit_bounds: Rc::new(Cell::new(None)),
					question: String::new(),
					time: label.into(),
					answer: String::new(),
				});

				mark.question = if voice_headers.contains_key(&index) {
					if text.trim().is_empty() {
						voice_headers[&index].empty_status().into()
					} else {
						format!("Voice conversation · {}", preview(text))
					}
				} else {
					preview(text)
				};

				mark.answer.clear();

				current = Some(key);
			} else if let Some(key) = &current {
				self.timeline.marks.get_mut(key).expect("current native mark").answer =
					preview(text);
			}
		}

		self.timeline.marks.retain(|key, _| retained.contains(key));

		if self.timeline.selected.as_ref().is_some_and(|key| !retained.contains(key)) {
			self.timeline.selected = None;
		}
	}

	pub(super) fn anchored_native_history_entry(
		&self,
		work: &AgentWorkItemDto,
		entry: &AgentTimelineEntry,
		row: AnyElement,
	) -> AnyElement {
		self.anchor_history_row(
			&HistoryKey::native(work.codex_thread_id.as_deref().unwrap_or_default(), entry),
			row,
		)
	}

	fn anchor_history_row(&self, key: &HistoryKey, row: AnyElement) -> AnyElement {
		let Some(mark) = self.timeline.marks.get(key) else {
			return row;
		};
		let position = mark.position.clone();
		let scroll = self
			.timeline
			.scroll
			.get(self.selected.as_deref().unwrap_or_default())
			.cloned()
			.unwrap_or_default();

		gpui::div()
			.w_full()
			.flex_none()
			.on_children_prepainted(move |bounds, window, cx| {
				if let Some(bounds) = bounds.first() {
					let measured =
						f32::from(bounds.origin.y - scroll.bounds().origin.y - scroll.offset().y);

					if (position.get() - measured).abs() > 0.5 {
						position.set(measured);

						ui_motion::request_frame(window, cx);
					}
				}
			})
			.id(SharedString::from(format!("history-anchor-{key}")))
			.child(row)
			.into_any_element()
	}

	fn jump_to_history(&mut self, id: HistoryKey, cx: &mut Context<Self>) {
		self.timeline.latest_follow_work = None;

		self.cancel_native_scroll_anchor();
		self.set_voice_follow(false);

		if let Some(mark) = self.timeline.marks.get(&id)
			&& let Some(work) = self.selected.as_ref()
			&& let Some(scroll) = self.timeline.scroll.get(work)
		{
			self.timeline.selected = Some(id.clone());
			self.timeline.navigation = Some(HistoryNavigation {
				work: work.clone(),
				id: Some(id),
				from: scroll.offset().y.into(),
				to: navigation_offset(mark.position.get(), scroll.max_offset().y.into()),
				started: std::time::Instant::now(),
			});

			cx.notify();
		}
	}

	pub(super) fn scroll_history(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
		let delta = event.delta.pixel_delta(gpui::px(BODY_LINE_HEIGHT));

		// macOS also sends phase-only and horizontal gesture events. They do
		// not move the transcript and must not cancel its current scroll state.
		if delta.y == gpui::px(0.) {
			return;
		}

		self.clear_voice_reading_space();

		self.timeline.latest_follow_work = None;

		self.cancel_native_scroll_anchor();

		self.timeline.navigation = None;
		self.timeline.selected = None;

		if let Some(scroll) = self.selected.as_ref().and_then(|id| self.timeline.scroll.get(id)) {
			let smooth = ui_scroll::smooth(event.delta);

			if smooth {
				let now = std::time::Instant::now();
				let current = f32::from(scroll.offset().y);
				let wheel = self.timeline.wheel_scroll.get_or_insert_with(|| WheelScroll {
					work: self.selected.clone().unwrap_or_default(),
					motion: crate::ui_scroll::Motion::new(current, now),
				});

				wheel.motion.retarget(current, delta.y.into(), scroll.max_offset().y.into(), now);
			} else {
				self.timeline.wheel_scroll = None;

				let offset =
					(scroll.offset().y + delta.y).clamp(-scroll.max_offset().y, gpui::px(0.));

				scroll.set_offset(gpui::point(gpui::px(0.), offset));
			}

			let following = !self.item_context_window_active()
				&& !smooth
				&& delta.y < gpui::px(0.)
				&& (scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(1.);

			if let Some(id) = &self.selected {
				if following {
					self.timeline.follow_paused.remove(id);
					self.timeline.latest_follow_work = Some(id.clone());
				} else {
					self.timeline.follow_paused.insert(id.clone());
				}
			}

			self.set_voice_follow(following);

			if delta.y > gpui::px(0.) {
				self.timeline.native.prefetch_requested = true;

				self.prefetch_older_history(cx);
			}

			cx.stop_propagation();
			cx.notify();
		}
	}

	pub(super) fn animate_history_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.timeline.navigation.is_some() || self.timeline.latest_follow_work.is_some() {
			self.timeline.wheel_scroll = None;
		}

		if let Some(wheel) = &self.timeline.wheel_scroll {
			if self.selected.as_ref() != Some(&wheel.work) {
				self.timeline.wheel_scroll = None;
			} else if let Some(scroll) = self.timeline.scroll.get(&wheel.work) {
				let (offset, moving) = if ui_scroll::enabled() {
					wheel.motion.sample(std::time::Instant::now())
				} else {
					(wheel.motion.to, false)
				};
				let offset = offset.clamp(-f32::from(scroll.max_offset().y).max(0.), 0.);

				scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(offset)));

				if moving {
					ui_motion::request_frame(window, cx);
				} else {
					let following = !self.item_context_window_active()
						&& wheel.motion.to < wheel.motion.from
						&& (offset + f32::from(scroll.max_offset().y)).abs() < 1.;

					if following {
						self.timeline.follow_paused.remove(&wheel.work);
						self.timeline.latest_follow_work = Some(wheel.work.clone());
						self.set_voice_follow(true);
					}

					self.timeline.wheel_scroll = None;
				}
			}
		}

		let Some(navigation) = &self.timeline.navigation else {
			return;
		};

		if self.selected.as_ref() != Some(&navigation.work)
			|| navigation.id.as_ref().is_some_and(|id| !self.timeline.marks.contains_key(id))
		{
			self.timeline.navigation = None;

			return;
		}

		let t = navigation.progress();

		if let Some(id) = navigation.id.as_ref()
			&& let Some(scroll) = self.timeline.scroll.get(&navigation.work)
		{
			let target = self.timeline.marks.get(id).map_or(navigation.to, |m| {
				navigation_offset(m.position.get(), scroll.max_offset().y.into())
			});
			let offset = navigation.from + (target - navigation.from) * (1.0 - (1.0 - t).powi(3));

			scroll.set_offset(gpui::point(
				gpui::px(0.0),
				gpui::px(offset.clamp(-f32::from(scroll.max_offset().y).max(0.0), 0.0)),
			));
		}

		if t < 1.0 {
			ui_motion::request_frame(window, cx);
		} else {
			let (work, id, started) =
				(navigation.work.clone(), navigation.id.clone(), navigation.started);
			let surface = cx.entity().downgrade();

			cx.defer(move |cx| {
				let _ = surface.update(cx, |s, cx| {
					if !s.timeline.navigation.as_ref().is_some_and(|current| {
						current.work == work && current.id == id && current.started == started
					}) {
						return;
					}
					if id.is_none() {
						s.timeline.latest_follow_work = Some(work.clone());

						s.timeline.follow_paused.remove(&work);
						s.set_voice_follow(true);
					}
					if s.selected.as_ref() == Some(&work)
						&& let (Some(mark), Some(scroll)) = (
							id.as_ref().and_then(|id| s.timeline.marks.get(id)),
							s.timeline.scroll.get(&work),
						) {
						scroll.set_offset(gpui::point(
							scroll.offset().x,
							gpui::px(navigation_offset(
								mark.position.get(),
								scroll.max_offset().y.into(),
							)),
						));
					}

					s.timeline.navigation = None;

					cx.notify();
				});
			});
		}
	}

	pub(super) fn history_navigation_bottom_gap(&self) -> Option<f32> {
		let navigation = self.timeline.navigation.as_ref()?;

		(navigation.id.is_none() && self.selected.as_ref() == Some(&navigation.work)).then(|| {
			(navigation.from - navigation.to).max(0.) * (1. - navigation.progress()).powi(3)
		})
	}

	pub(super) fn toggle_connection_details(&mut self, cx: &mut Context<Self>) {
		// A footer resize is not conversation navigation. Preserve bottom-follow before
		// the new footer height changes the scroll range, or keep the reader's offset.
		if let Some(work) = self.selected.as_ref()
			&& let Some(scroll) = self.timeline.scroll.get(work)
			&& (scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(1.)
		{
			self.timeline.latest_follow_work = Some(work.clone());
		}

		self.workspace.connection_details_expanded = !self.workspace.connection_details_expanded;

		cx.notify();
	}

	fn clear_voice_reading_space(&self) {
		if let Some(height) =
			self.selected.as_ref().and_then(|work| self.timeline.voice_min_height.get(work))
		{
			height.set(0.);
		}
	}

	pub(super) fn follow_latest_after_send(&mut self, cx: &mut Context<Self>) {
		self.timeline.latest_follow_work = None;
		self.timeline.older_scroll_anchor = None;

		if let Some(work) = &self.selected {
			self.timeline.scroll.entry(work.clone()).or_default();
		}

		// Sending is a short navigation transition. Once it settles, streaming
		// follows the current layout exactly instead of easing after every token.
		self.jump_to_latest(cx);
	}

	pub(super) fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
		self.reset_item_context();
		self.clear_voice_reading_space();

		let Some(work) = self.selected.clone() else {
			return;
		};
		let Some(scroll) = self.timeline.scroll.get(&work) else {
			return;
		};

		self.timeline.selected = None;

		self.timeline.follow_paused.insert(work.clone());

		self.timeline.navigation = Some(HistoryNavigation {
			work,
			id: None,
			from: scroll.offset().y.into(),
			to: -f32::from(scroll.max_offset().y),
			started: std::time::Instant::now(),
		});

		self.set_voice_follow(false);
		cx.notify();
	}

	pub(super) fn latest_button(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		let voice_space = self
			.selected
			.as_ref()
			.and_then(|work| self.timeline.voice_min_height.get(work))
			.is_some_and(|height| height.get() > 0.);
		let visible =
			self.item_context_window_active()
				|| voice_space
				|| self.selected.as_ref().and_then(|id| self.timeline.scroll.get(id)).is_some_and(
					|scroll| scroll.max_offset().y + scroll.offset().y > gpui::px(48.),
				);
		let working = self
			.conversation_work()
			.is_some_and(|work| self.response_in_progress(&work) && !self.thread_in_use(&work.id));
		let width =
			ui_motion::value("jump-latest-width", if working { 56. } else { 28. }, window, cx);
		let clock =
			window.use_keyed_state("jump-latest-clock", cx, |_, _| std::time::Instant::now());
		let phase = clock.read(cx).elapsed().as_secs_f32() * 4.;

		if visible && working {
			ui_motion::request_frame(window, cx);
		}

		let opacity =
			ui_motion::value("jump-latest-opacity", if visible { 1. } else { 0. }, window, cx);

		gpui::div()
			.absolute()
			.left_0()
			.right_0()
			.flex()
			.justify_center()
			.bottom(gpui::px(self.workspace_composer_clearance() + 8.))
			.when(opacity > 0.001, |d| {
				d.child(
					gpui::div()
						.id("jump-to-latest")
						.debug_selector(|| "jump-to-latest".into())
						.occlude()
						.role(Role::Button)
						.aria_label(if working {
							"Working · Jump to latest message"
						} else {
							"Jump to latest message"
						})
						.tab_index(0)
						.h(gpui::px(28.))
						.w(gpui::px(width))
						.rounded_full()
						.bg(crate::ui_theme::rgba(SURFACE_OVERLAY_MATERIAL))
						.flex()
						.items_center()
						.justify_center()
						.gap(gpui::px(5.))
						.opacity(opacity)
						.cursor_pointer()
						.on_scroll(cx.listener(|s, event, _, cx| s.scroll_history(event, cx)))
						.hover(|d| d.bg(crate::ui_theme::rgba(0x302d397c)))
						.on_click(cx.listener(|s, _, _, cx| s.jump_to_latest(cx)))
						.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
							if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
								s.jump_to_latest(cx);
								cx.stop_propagation();
							}
						}))
						.when(working, |d| {
							d.child(gpui::div().flex().items_center().gap(gpui::px(2.)).children(
								(0..3).map(|i| {
									gpui::div()
										.size(gpui::px(3.))
										.rounded_full()
										.bg(crate::ui_theme::rgb(BLUE))
										.opacity(
											0.35 + 0.65 * ((phase - i as f32 * 0.7).sin() + 1.)
												/ 2.,
										)
								}),
							))
						})
						.child(workspace_symbols::icon(Symbol::ArrowDown)),
				)
			})
			.into_any_element()
	}

	pub(super) fn history_rail_slot(
		&self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if self.timeline.marks.len() < 2 {
			return gpui::div().into_any_element();
		}

		// Overlay the existing text inset so navigation never changes line wrapping.
		gpui::div()
			.absolute()
			.top_0()
			.right_0()
			.bottom_0()
			.w(gpui::px(HISTORY_RAIL_WIDTH))
			.flex_none()
			.overflow_hidden()
			.child(
				gpui::div()
					.absolute()
					.inset_0()
					.bottom(gpui::px(self.workspace_composer_clearance()))
					.child(self.history_rail(window, cx)),
			)
			.into_any_element()
	}

	pub(super) fn preserve_history_selection(&mut self) {
		let scroll = self.timeline.scroll_for(self.selected.as_deref());
		let index = self.active_history_index(&scroll);

		// Disclosure changes layout, not the reader's navigation target. Wheel
		// scrolling and explicit navigation release or replace this selection.
		self.timeline.selected = self.timeline.marks.keys().nth(index).cloned();
	}

	fn active_history_index(&self, scroll: &ScrollHandle) -> usize {
		let last = self.timeline.marks.len().saturating_sub(1);
		let at_end = scroll.max_offset().y > gpui::px(0.)
			&& (scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(1.);

		self.timeline
			.selected
			.as_ref()
			.and_then(|id| self.timeline.marks.keys().position(|key| key == id))
			.unwrap_or_else(|| {
				if at_end
					|| (self.selected.is_some()
						&& self.timeline.latest_follow_work == self.selected)
				{
					last
				} else {
					current_mark(
						&self.timeline.marks.values().map(|m| m.position.get()).collect::<Vec<_>>(),
						-f32::from(scroll.offset().y),
					)
				}
			})
	}

	pub(super) fn history_rail(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		if self.timeline.marks.len() < 2 {
			return gpui::div().into_any_element();
		}

		let scroll = self.timeline.scroll_for(self.selected.as_deref());
		let positions: Vec<_> =
			self.timeline.marks.values().map(|mark| mark.position.clone()).collect();
		let working = self.conversation_work().is_some_and(|work| self.response_in_progress(&work));
		let last = positions.len() - 1;
		let current = self.active_history_index(&scroll);
		let active_position = ui_motion::value(
			SharedString::from(format!(
				"rail-active-{}",
				self.selected.as_deref().unwrap_or_default()
			)),
			current as f32,
			window,
			cx,
		);
		let mut rail = self.history_rail_surface(cx);

		for (index, (id, mark)) in self.timeline.marks.iter().enumerate() {
			let influence = ui_motion::value(
				SharedString::from(format!(
					"rail-magnify-{}-{id}",
					self.selected.as_deref().unwrap_or_default()
				)),
				dock_influence(index, self.timeline.hover),
				window,
				cx,
			);
			let tip = mark.clone();
			let id = id.clone();
			let keyboard_id = id.clone();
			let hit_bounds = mark.hit_bounds.clone();

			rail = rail.child(
				gpui::div()
					.id(SharedString::from(format!("history-tick-{id}")))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Jump to: {}", mark.question))
					.w_full()
					// Size the hit target and its tick together in the current layout.
					// Reading the previous scroll bounds lags behind a Dock resize.
					.flex_1()
					.min_h_0()
					.max_h(gpui::px(11.))
					.cursor_pointer()
					.on_hover(cx.listener(move |s, hovered: &bool, _, cx| {
						if *hovered {
							s.timeline.hover = Some(index);
						} else if s.timeline.hover == Some(index) {
							s.timeline.hover = None;
						}

						cx.notify();
					}))
					// Navigation previews already have their text; do not use the
					// generic tooltip's 500 ms discovery delay.
					.tooltip_show_delay(Duration::ZERO)
					.tooltip(move |window, cx| {
						cx.new(|cx| HistoryPreview::new(tip.clone(), window, cx)).into()
					})
					.on_click(cx.listener(move |s, _, _, cx| {
						s.jump_to_history(id.clone(), cx);
					}))
					.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
						if event.keystroke.key == "enter" || event.keystroke.key == "space" {
							s.jump_to_history(keyboard_id.clone(), cx);
							cx.stop_propagation();
							cx.notify();
						}
					}))
					.child(
						gpui::canvas(
							move |bounds, _, _| hit_bounds.set(Some(bounds)),
							move |bounds, _, window, _| {
								let activity =
									(1. - (index as f32 - active_position).abs()).clamp(0., 1.);
								let color = if working && index == last {
									crate::ui_theme::rgb(BLUE)
								} else {
									crate::ui_theme::rgba(
										(TEXT << 8) | (100. + 155. * activity) as u32,
									)
								};
								let width = 7.0 + influence * 7.0;
								let height = 2.0 + influence;

								window.paint_quad(gpui::fill(
									gpui::Bounds::new(
										gpui::point(
											bounds.center().x - gpui::px(width / 2.0),
											bounds.center().y - gpui::px(height / 2.),
										),
										gpui::size(gpui::px(width), gpui::px(height)),
									),
									color,
								));
							},
						)
						.size_full(),
					),
			);
		}

		rail.into_any_element()
	}

	fn history_rail_surface(&self, cx: &mut Context<Self>) -> Stateful<Div> {
		gpui::div()
			.id("conversation-history-rail")
			.debug_selector(|| "conversation-history-rail".into())
			.on_scroll(cx.listener(|s, event, _, cx| s.scroll_history(event, cx)))
			.w_full()
			.flex_none()
			.h_full()
			// Keep the first tick below the conversation's expand control.
			.pt(gpui::px(36.))
			.pb_4()
			.flex()
			.flex_col()
			.justify_center()
			.overflow_hidden()
	}
}

#[derive(Clone)]
pub(super) struct HistoryMark {
	pub(super) position: Rc<Cell<f32>>,
	hit_bounds: Rc<Cell<Option<gpui::Bounds<Pixels>>>>,
	question: String,
	time: String,
	answer: String,
}

#[derive(Clone)]
pub(super) struct HistoryScrollAnchor {
	pub(super) work: String,
	pub(super) offset: f32,
	pub(super) maximum: f32,
	pub(super) message: Option<(HistoryKey, f32)>,
}

pub(super) struct HistoryNavigation {
	work: String,
	id: Option<HistoryKey>,
	from: f32,
	to: f32,
	started: std::time::Instant,
}
impl HistoryNavigation {
	fn progress(&self) -> f32 {
		(self.started.elapsed().as_secs_f32() / 0.28).min(1.)
	}
}

pub(super) struct WheelScroll {
	work: String,
	motion: crate::ui_scroll::Motion,
}

struct HistoryPreview {
	mark: HistoryMark,
	id: SharedString,
	dismissed: bool,
}
impl HistoryPreview {
	fn new(mark: HistoryMark, window: &Window, cx: &mut Context<Self>) -> Self {
		let id = SharedString::from(format!("history-preview-{}", cx.entity_id()));
		let released = id.clone();
		let parent = window.window_handle();
		cx.on_release(move |_, cx| {
			ui_motion::dismiss_popover(parent, released.into(), cx);
		})
		.detach();
		Self { mark, id, dismissed: false }
	}
}
impl Render for HistoryPreview {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let mark = self.mark.clone();
		let owner = cx.entity().downgrade();
		gpui::div().w(gpui::px(320.)).child(
			ui_motion::popover_lazy(self.id.clone(), !self.dismissed, move |_, _| {
				history_preview_content(&mark).into_any_element()
			})
			.preview()
			.on_dismiss(move |cx| {
				let _ = owner.update(cx, |preview, cx| {
					preview.dismissed = true;
					cx.notify();
				});
			}),
		)
	}
}

fn history_preview_content(mark: &HistoryMark) -> Div {
	gpui::div()
		.debug_selector(|| "history-hover-preview".into())
		.p(gpui::px(10.))
		.flex()
		.flex_col()
		.gap(gpui::px(6.))
		.text_size(gpui::px(12.0))
		.line_height(gpui::px(18.0))
		.child(
			gpui::div()
				.text_size(gpui::px(11.0))
				.text_color(crate::ui_theme::rgb(TEXT_MUTED))
				.child(mark.time.clone()),
		)
		.child(
			gpui::div()
				.font_weight(FontWeight::MEDIUM)
				.text_color(crate::ui_theme::rgb(TEXT))
				.child(mark.question.clone()),
		)
		.when(!mark.answer.is_empty(), |panel| {
			panel.child(
				gpui::div().text_color(crate::ui_theme::rgb(TEXT_MUTED)).child(mark.answer.clone()),
			)
		})
}

fn navigation_offset(position: f32, maximum: f32) -> f32 {
	(-(position - HISTORY_ANCHOR_INSET)).clamp(-maximum.max(0.0), 0.0)
}

fn dock_influence(index: usize, hover: Option<usize>) -> f32 {
	hover.map_or(0.0, |hover| (1.0 - index.abs_diff(hover) as f32 / 3.0).max(0.0).powi(2))
}

fn preview(text: &str) -> String {
	let voice = history::handoff(text).map(|text| format!("Voice conversation · {text}"));
	let text = markdown::plain_text(voice.as_deref().unwrap_or(text));
	let mut chars = text.chars();
	let mut text: String = chars.by_ref().take(180).collect();

	if chars.next().is_some() {
		text.push('…');
	}

	text.replace('\n', " ")
}

fn current_mark(positions: &[f32], offset: f32) -> usize {
	positions
		.iter()
		.rposition(|position| *position <= offset + HISTORY_ANCHOR_INSET + 0.5)
		.unwrap_or(0)
}

#[cfg(test)]
mod tests {
	use std::thread;

	use crate::shell::agent_surface::activity::{
		self, AgentHistoryResult, AgentSurface, BTreeMap, HistoryKey, HistoryScrollAnchor,
		WheelScroll,
	};
	#[cfg(test)] use decodex_protocol::AgentTimelineEntry;
	#[cfg(test)] use decodex_protocol::AgentTimelinePage;

	#[gpui::test]
	fn disclosure_layout_keeps_rail_selection_until_reader_scrolls(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(300.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		visual.update(crate::ui_frame::draw_current_frame);

		surface.update(visual, |s, cx| {
			let scroll = s.timeline.scroll["agent"].clone();

			s.timeline.latest_follow_work = None;
			s.timeline.selected = None;

			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));

			assert_eq!(s.timeline.marks.len(), 2);

			for (index, mark) in s.timeline.marks.values().enumerate() {
				mark.position.set(index as f32 * 500.);
			}

			assert_eq!(s.active_history_index(&scroll), 0);

			s.preserve_history_selection();

			let selected = s.timeline.selected.clone();

			for position in [20., 800., 20.] {
				s.timeline.marks.values().nth(1).unwrap().position.set(position);

				assert_eq!(s.active_history_index(&scroll), 0);
				assert_eq!(s.timeline.selected, selected);
			}

			s.scroll_history(
				&gpui::ScrollEvent {
					delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-1.))),
					..Default::default()
				},
				cx,
			);

			assert!(s.timeline.selected.is_none());
			assert_eq!(s.active_history_index(&scroll), 1);
		});
	}

	#[gpui::test]
	fn history_hover_shows_without_dwell_and_clears_on_exit(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));
		surface.update(visual, |s, cx| s.visual_workspace_fixture(cx));
		visual.update(crate::ui_frame::draw_current_frame);

		let bounds = surface.read_with(visual, |s, _| {
			s.timeline.marks.values().next().unwrap().hit_bounds.get().unwrap()
		});
		let rail = visual.debug_bounds("conversation-history-rail").unwrap();
		let transcript = visual.debug_bounds("workspace-transcript").unwrap();
		let content = visual.debug_bounds("conversation-content").unwrap();

		assert!(
			rail.left() >= content.right() - gpui::px(crate::ui_theme::CONVERSATION_INSET),
			"navigation must stay outside the message text"
		);
		assert_eq!(rail.right(), transcript.right());
		assert_eq!(rail.size.width, gpui::px(crate::ui_theme::CONVERSATION_INSET));

		visual.simulate_mouse_move(bounds.center(), gpui::MouseButton::Left, Default::default());
		// No clock advance: cached content must appear before the default tooltip dwell.
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("history-hover-preview").is_some());

		visual.simulate_mouse_move(
			gpui::point(gpui::px(1.), gpui::px(1.)),
			gpui::MouseButton::Left,
			Default::default(),
		);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("history-hover-preview").is_none());
	}

	#[gpui::test]
	fn agent_loading_keeps_transcript_horizontal_bounds(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));

		let saved = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			(s.selected.clone(), s.history.clone())
		});

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_current_frame);

		let original = visual.debug_bounds("workspace-transcript").unwrap();

		for loading in [true, false] {
			surface.update(visual, |s, cx| {
				s.selected = if loading { Some("release".into()) } else { saved.0.clone() };
				s.history = if loading { None } else { saved.1.clone() };

				cx.notify();
			});

			for delay in [0, 100, 140] {
				thread::sleep(std::time::Duration::from_millis(delay));

				visual.update(crate::ui_frame::draw_current_frame);

				let bounds = visual.debug_bounds("workspace-transcript").unwrap();

				assert_eq!(
					bounds.origin.x, original.origin.x,
					"history loading must not move text"
				);
				assert_eq!(bounds.size.width, original.size.width);
			}
		}
	}

	#[gpui::test]
	fn deferred_navigation_finish_does_not_end_a_newer_jump(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		visual.update(crate::ui_frame::draw_current_frame);

		surface.update(visual, |s, cx| {
			s.jump_to_history(HistoryKey::Local(1), cx);

			s.timeline.navigation.as_mut().unwrap().started -= std::time::Duration::from_secs(1);
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
			surface.update(cx, |s, cx| s.jump_to_history(HistoryKey::Local(3), cx));
		});

		surface.update(visual, |s, cx| {
			assert_eq!(s.timeline.navigation.as_ref().unwrap().id, Some(HistoryKey::Local(3)));

			s.timeline.navigation.as_mut().unwrap().started -= std::time::Duration::from_secs(1);

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);
		surface.read_with(visual, |s, _| {
			assert!(s.timeline.navigation.is_none());
			assert_eq!(s.timeline.selected, Some(HistoryKey::Local(3)));
		});
	}

	#[gpui::test]
	fn native_jump_finishes_against_layout_after_an_older_page_arrives(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(400.)));

		let binding = surface.update(visual, |s, cx| {
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

			let binding = super::super::native_timeline::Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			};

			assert!(s.timeline.native.replace(
				binding.clone(),
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: vec![
						navigation_row(10, true),
						navigation_row(11, false),
						navigation_row(20, true),
						navigation_row(21, false)
					],
					next_cursor: Some("older".into()),
					weather: Default::default(),
					thread_token_total: None,
					thread_token_breakdown: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				}
			));

			cx.notify();

			binding
		});

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_current_frame);

		let target = HistoryKey::native("thread", &navigation_row(20, true));
		let bounds =
			surface.read_with(visual, |s, _| s.timeline.marks[&target].hit_bounds.get().unwrap());

		visual.simulate_click(bounds.center(), Default::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.timeline.navigation.as_ref().unwrap().id, Some(target.clone()));

			s.timeline.navigation.as_mut().unwrap().started -= std::time::Duration::from_secs(1);

			assert!(s.prepend_native_history(
				&binding,
				"older",
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: vec![navigation_row(1, false)],
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_token_breakdown: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				}
			));

			cx.notify();
		});

		for _ in 0..3 {
			visual.update(crate::ui_frame::draw_current_frame);
			visual.run_until_parked();
		}

		surface.read_with(visual, |s, _| {
			let scroll = &s.timeline.scroll[&binding.work];
			let expected = activity::navigation_offset(
				s.timeline.marks[&target].position.get(),
				scroll.max_offset().y.into(),
			);
			let actual = f32::from(scroll.offset().y);

			assert!(
				(actual - expected).abs() < 1.,
				"jump stopped at {actual}, target is {expected}"
			);
			assert!(s.timeline.navigation.is_none());
			assert_eq!(s.timeline.selected.as_ref(), Some(&target));
		});
	}

	fn navigation_row(position: u64, user: bool) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position,
			content: decodex_protocol::AgentTimelineContent::Item {
				collaboration: None,
				phase: None,
				app_ui: false,
				turn_id: "turn".into(),
				item_id: format!("item-{position}"),
				kind: if user { "userMessage" } else { "agentMessage" }.into(),
				text: "Content for the scrolling test.\n\n".repeat(20),
				truncated: false,
				activity: None,
				attachments: vec![],
			},
		}
	}

	#[gpui::test]
	fn native_rail_keeps_same_position_speech_and_text_distinct_and_retires_evicted_targets(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(400.)));

		let keys = surface.update(visual, |s, cx| {
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

			work.codex_thread_id = Some("native-thread".into());

			let text = "A repeated user prompt.\n\n".repeat(20);
			let entries = vec![
				AgentTimelineEntry {
					position: 5,
					content: decodex_protocol::AgentTimelineContent::Item {
						collaboration: None,
						phase: None,
						app_ui: false,
						turn_id: "turn".into(),
						item_id: "same-id".into(),
						kind: "agentInput".into(),
						text: text.clone(),
						truncated: false,
						activity: None,
						attachments: vec![],
					},
				},
				AgentTimelineEntry {
					position: 5,
					content: decodex_protocol::AgentTimelineContent::Speech {
						item_id: "same-id".into(),
						session_id: "voice".into(),
						role: "user".into(),
						text,
						truncated: false,
					},
				},
			];
			let keys = entries
				.iter()
				.map(|entry| HistoryKey::native("native-thread", entry))
				.collect::<Vec<_>>();

			assert!(s.timeline.native.replace(
				super::super::native_timeline::Binding {
					work: work.id.clone(),
					thread: "native-thread".into(),
					account: "account".into(),
				},
				AgentTimelinePage {
					thread_id: "native-thread".into(),
					entries,
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_token_breakdown: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None
				}
			));

			cx.notify();

			keys
		});

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_current_frame);

		let bounds = surface.read_with(visual, |s, _| {
			assert_eq!(s.timeline.marks.len(), 2);
			assert_ne!(keys[0], keys[1]);
			assert!(
				s.timeline.marks[&keys[1]].position.get()
					> s.timeline.marks[&keys[0]].position.get()
			);

			s.timeline.marks[&keys[0]].hit_bounds.get().unwrap()
		});

		visual.simulate_click(bounds.center(), Default::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.timeline.selected.as_ref(), Some(&keys[0]));
			assert_eq!(s.timeline.navigation.as_ref().unwrap().id, Some(keys[0].clone()));

			s.timeline.native.entries.remove(0);

			s.timeline.native.revision += 1;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);
		surface.read_with(visual, |s, _| {
			assert!(s.timeline.navigation.is_none() && s.timeline.selected.is_none());
			assert!(!s.timeline.marks.contains_key(&keys[0]));
			assert!(s.timeline.marks.contains_key(&keys[1]));
		});
	}

	#[gpui::test]
	#[ignore = "Manual CPU draw benchmark; use --release, not a GPU FPS measurement"]
	fn long_history_scroll_draw_benchmark(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s.snapshot.as_mut().unwrap().work_items.iter_mut().find(|w| w.id == "agent").unwrap();

			work.codex_thread_id = Some("benchmark-thread".into());

			let entries = (0..300).map(|i| AgentTimelineEntry {
				position: i,
				content: if i % 3 == 2 { decodex_protocol::AgentTimelineContent::TurnBoundary {
					turn_id: format!("turn-{}", i / 3), completed: true, status: Some("completed".into()),
					duration_ms: Some(3_200), usage: None, timing: None, usage_summary: None, error: None,
				} } else { decodex_protocol::AgentTimelineContent::Item { collaboration: None,
phase: None,
					app_ui: false,
					turn_id: format!("turn-{}", i / 3), item_id: format!("message-{i}"),
					kind: if i % 3 == 0 { "userMessage" } else { "agentMessage" }.into(),
					text: format!("## Message {i}\n\n{}", "Review **the result** and `src/main.rs`.\n\n- Keep history readable.\n- Preserve the scroll position.\n\n".repeat(4)),
					truncated: false, activity: None, attachments: vec![],
				} },
			}).collect();

			assert!(s.timeline.native.replace(super::super::native_timeline::Binding {
				work: "agent".into(), thread: "benchmark-thread".into(), account: "benchmark".into(),
			}, decodex_protocol::AgentTimelinePage {
				thread_id: "benchmark-thread".into(), entries, next_cursor: None,
				weather: Default::default(), thread_token_total: None, thread_token_breakdown: None,
		thread_metrics: None, safety_buffering_turn_id: None, active_realtime_session_at_page_start: None,
			}));

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		let anchors = surface.read_with(visual, |s, _| {
			s.timeline
				.marks
				.iter()
				.map(|(key, mark)| (key.clone(), mark.position.get()))
				.collect::<BTreeMap<_, _>>()
		});
		let mut samples = Vec::new();
		let mut maximum = None;

		for frame in 0..70 {
			surface.update(visual, |s, cx| {
				s.timeline.latest_follow_work = None;

				s.timeline.follow_paused.insert("agent".into());
				s.timeline.scroll["agent"]
					.set_offset(gpui::point(gpui::px(0.), gpui::px(-2_000. - frame as f32 * 17.)));
				cx.notify();
			});

			let start = std::time::Instant::now();

			visual.update(crate::ui_frame::draw_current_frame);

			if frame >= 10 {
				samples.push(start.elapsed().as_secs_f64() * 1_000.);

				let current =
					surface.read_with(visual, |s, _| s.timeline.scroll["agent"].max_offset().y);

				if let Some(expected) = maximum {
					assert_eq!(current, expected, "windowing must retain exact scroll extent");
				}

				maximum = Some(current);

				surface.read_with(visual, |s, _| {
					for (key, expected) in &anchors {
						assert!(
							(s.timeline.marks[key].position.get() - expected).abs() < 0.5,
							"grouped rows must retain each timeline anchor"
						);
					}
				});
			}
		}

		samples.sort_by(f64::total_cmp);

		eprintln!(
			"scroll draw CPU, 300 entries, 60 warm frames: median={:.3}ms p95={:.3}ms max={:.3}ms",
			samples[30], samples[57], samples[59]
		);
	}

	#[test]
	fn clicked_anchor_and_active_marker_use_the_same_inset() {
		let positions = [0., 340., 1_117., 2_400.];

		for (index, position) in positions.iter().enumerate() {
			assert_eq!(
				activity::current_mark(&positions, -activity::navigation_offset(*position, 3_000.)),
				index
			);
		}
	}

	#[gpui::test]
	fn dock_resize_keeps_rail_targets_and_history_geometry_in_sync(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
				panic!("fixture history");
			};

			*entries = (0..36)
				.map(|id| {
					let mut entry = entries[0].clone();

					entry.id = id;
					entry.text =
						format!("Reading note {id}: keep this message at its original size.");

					entry
				})
				.collect();
			s.workspace.panel_drag = Some((super::super::workspace_size::Panel::Bottom, 0., 0.));
			s.workspace.graph_visible = true;
			s.workspace.graph_panel_height = 120.;
		});

		visual.update(crate::ui_frame::draw_current_frame);
		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(400));

		visual.update(crate::ui_frame::draw_current_frame);

		let positions = surface.read_with(visual, |s, _| {
			s.timeline.marks.values().map(|mark| mark.position.get()).collect::<Vec<_>>()
		});

		for height in [560., 240., 480., 120.] {
			surface.update(visual, |s, cx| {
				s.workspace.panel_drag =
					Some((super::super::workspace_size::Panel::Bottom, 0., 0.));
				s.workspace.graph_panel_height = height;

				cx.notify();
			});

			// Check the first frame at the new size, before any follow-up notification.
			visual.update(crate::ui_frame::draw_current_frame);

			let rail = visual.debug_bounds("conversation-history-rail").unwrap();
			let targets = surface.read_with(visual, |s, _| {
				s.timeline
					.marks
					.iter()
					.enumerate()
					.map(|(index, (id, mark))| {
						assert!((mark.position.get() - positions[index]).abs() < 0.5);

						let bounds = mark.hit_bounds.get().unwrap();

						assert!(bounds.origin.y >= rail.origin.y + gpui::px(35.5));
						assert!(bounds.bottom() <= rail.bottom() - gpui::px(15.5));
						assert!(bounds.size.height > gpui::px(0.));

						(id.clone(), bounds)
					})
					.collect::<Vec<_>>()
			});

			for (id, bounds) in targets.iter().step_by(7) {
				visual.simulate_click(bounds.center(), Default::default());
				surface
					.read_with(visual, |s, _| assert_eq!(s.timeline.selected.as_ref(), Some(id)));
			}
		}
	}

	#[gpui::test]
	fn rail_hit_targets_select_their_own_message(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		visual.update(crate::ui_frame::draw_current_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_current_frame);

		let ids =
			surface.read_with(visual, |s, _| s.timeline.marks.keys().cloned().collect::<Vec<_>>());

		for id in ids {
			let bounds =
				surface.read_with(visual, |s, _| s.timeline.marks[&id].hit_bounds.get().unwrap());

			visual.simulate_mouse_down(
				bounds.center(),
				gpui::MouseButton::Left,
				Default::default(),
			);
			visual.simulate_mouse_up(bounds.center(), gpui::MouseButton::Left, Default::default());

			surface.update(visual, |s, cx| {
				assert_eq!(s.timeline.selected, Some(id.clone()));
				assert_eq!(s.timeline.navigation.as_ref().unwrap().id, Some(id.clone()));

				s.timeline.navigation.as_mut().unwrap().started -=
					std::time::Duration::from_secs(1);

				cx.notify();
			});

			visual.update(crate::ui_frame::draw_current_frame);
			surface.update(visual, |s, _| assert_eq!(s.timeline.selected, Some(id.clone())));
		}
	}

	#[gpui::test]
	fn history_marks_jump_to_real_message_anchors(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.0), gpui::px(320.0)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		visual.update(crate::ui_frame::draw_current_frame);

		visual.update(|_, cx| {
			surface.update(cx, |s, cx| {
				assert_eq!(s.timeline.marks.len(), 2);
				assert!(
					s.timeline.marks[&HistoryKey::Local(3)].position.get()
						> s.timeline.marks[&HistoryKey::Local(1)].position.get()
				);

				s.jump_to_history(HistoryKey::Local(3), cx);

				s.timeline.navigation.as_mut().unwrap().started -=
					std::time::Duration::from_secs(1);

				cx.notify();
			})
		});

		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);
		surface.update(visual, |s, _| {
			assert!(s.timeline.scroll["agent"].offset().y < gpui::px(0.0));
			assert!(
				s.timeline.scroll["agent"].offset().y >= -s.timeline.scroll["agent"].max_offset().y
			);
			assert_eq!(s.timeline.marks.len(), 2);
		});
	}

	#[gpui::test]
	fn mouse_wheel_animates_and_trackpad_takes_over_immediately(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(300.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		visual.update(crate::ui_frame::draw_current_frame);

		surface.update(visual, |s, cx| {
			let scroll = s.timeline.scroll["agent"].clone();

			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-50.)));

			let event = gpui::ScrollEvent {
				delta: gpui::ScrollDelta::Lines(gpui::point(0., -1.)),
				..Default::default()
			};

			s.scroll_history(&event, cx);

			assert_eq!(scroll.offset().y, gpui::px(-50.), "notches must not jump immediately");

			let first = s.timeline.wheel_scroll.as_ref().unwrap().motion.to;

			s.scroll_history(&event, cx);

			let wheel = s.timeline.wheel_scroll.as_ref().unwrap();

			assert!(wheel.motion.to < first, "successive notches accumulate");

			let midpoint =
				wheel.motion.sample(wheel.motion.started + std::time::Duration::from_millis(60)).0;

			assert!(midpoint < -50. && midpoint > wheel.motion.to);

			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(midpoint)));
			s.scroll_history(
				&gpui::ScrollEvent {
					delta: gpui::ScrollDelta::Lines(gpui::point(0., 1.)),
					..Default::default()
				},
				cx,
			);

			assert!(
				s.timeline.wheel_scroll.as_ref().unwrap().motion.to > midpoint,
				"reversal cancels pending forward travel"
			);

			s.scroll_history(
				&gpui::ScrollEvent {
					delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(0.25))),
					..Default::default()
				},
				cx,
			);

			assert!(s.timeline.wheel_scroll.is_none());
			assert_eq!(scroll.offset().y, gpui::px(midpoint + 0.25));
		});
	}

	#[test]
	fn wheel_animation_finishes_exactly_and_clamps_at_boundaries() {
		let now = std::time::Instant::now();
		let mut wheel =
			WheelScroll { work: "agent".into(), motion: crate::ui_scroll::Motion::new(-20., now) };

		wheel.motion.retarget(-20., -1_000., 200., now);

		assert_eq!(
			wheel.motion.sample(now + std::time::Duration::from_millis(600)),
			(-200., false)
		);

		wheel.motion.retarget(-80., 1_000., 200., now);

		assert_eq!(wheel.motion.to, 0.);
	}

	#[gpui::test]
	fn trackpad_phase_events_preserve_follow_and_fractional_deltas(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(300.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		visual.update(crate::ui_frame::draw_current_frame);

		surface.update(visual, |s, cx| {
			let scroll = s.timeline.scroll["agent"].clone();

			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-40.)));

			s.timeline.latest_follow_work = Some("agent".into());

			s.timeline.follow_paused.remove("agent");

			for phase in [gpui::TouchPhase::Started, gpui::TouchPhase::Ended] {
				s.scroll_history(
					&gpui::ScrollEvent {
						delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(3.), gpui::px(0.))),
						touch_phase: phase,
						..Default::default()
					},
					cx,
				);

				assert_eq!(s.timeline.latest_follow_work.as_deref(), Some("agent"));
				assert!(!s.timeline.follow_paused.contains("agent"));
				assert_eq!(scroll.offset().y, gpui::px(-40.));
			}
			for _ in 0..8 {
				s.scroll_history(
					&gpui::ScrollEvent {
						delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(0.25))),
						..Default::default()
					},
					cx,
				);
			}

			assert_eq!(
				scroll.offset().y,
				gpui::px(-38.),
				"precise deltas accumulate without rounding or duplication"
			);
			assert!(s.timeline.follow_paused.contains("agent"));
		});
	}

	#[gpui::test]
	fn conversation_and_composer_share_bounded_width(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
			s.workspace.sidebar_visible = false;
			s.workspace.agent_tree_visible = false;
		});

		for width in [1_600., 700.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(800.)));

			{
				visual.update(crate::ui_frame::draw_current_frame);

				let footer = visual.debug_bounds("floating-composer").unwrap();
				let composer = visual.debug_bounds("agent-composer").unwrap();

				assert!(
					(composer.center().x - footer.center().x).abs() < gpui::px(1.),
					"composer must center on the full panel, including the timeline gutter"
				);
				assert!(composer.size.width <= gpui::px(crate::ui_theme::CONVERSATION_WIDTH));
				assert!(composer.left() >= gpui::px(0.) && composer.right() <= gpui::px(width));
			}
		}
	}

	#[gpui::test]
	fn composer_growth_keeps_history_behind_the_overlay(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_000.), gpui::px(700.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		for text in [
			"Short draft".to_owned(),
			"Line one\nLine two\nLine three\nLine four".into(),
			"Long draft line\n".repeat(40),
			"Short again".into(),
		] {
			surface.update(visual, |s, cx| {
				s.composer.update(cx, |input, cx| input.set_content(&text, cx));
				cx.notify();
			});

			for _ in 0..8 {
				visual.update(crate::ui_frame::draw_current_frame);
			}

			let transcript = visual.debug_bounds("workspace-transcript").unwrap();
			let footer = visual.debug_bounds("floating-composer").unwrap();

			assert!(
				(transcript.bottom() - footer.bottom()).abs() < gpui::px(1.),
				"{text}: history and the floating input must share the same bottom edge"
			);

			let clearance = surface.read_with(visual, |s, _| s.workspace.composer_overlay_height);

			assert!(
				(clearance - f32::from(footer.size.height)).abs() < 1.,
				"tail clearance follows the growing composer"
			);
			assert!(transcript.size.height > gpui::px(0.));
		}
	}

	#[gpui::test]
	fn wheel_scroll_keeps_adjacent_messages_accessible(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.0), gpui::px(300.0)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		visual.update(crate::ui_frame::draw_current_frame);

		// Panel animation uses wall time, including in optimized test builds.
		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_current_frame);

		let scroll = surface.read_with(visual, |s, _| s.timeline.scroll["agent"].clone());

		assert!(scroll.max_offset().y >= gpui::px(100.), "fixture must allow the full wheel delta");
		assert!(
			(scroll.bounds().bottom() - visual.debug_bounds("floating-composer").unwrap().bottom())
				.abs()
				< gpui::px(1.),
			"history must continue behind the composer to the same bottom edge"
		);

		let position = scroll.bounds().center();

		visual.simulate_event(gpui::ScrollEvent {
			position,
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.0), gpui::px(-100.0))),
			..Default::default()
		});
		visual.update(crate::ui_frame::draw_current_frame);

		assert_eq!(
			scroll.offset().y,
			gpui::px(-100.0),
			"wheel delta must be applied once: pointer {position:?}, viewport {:?}, composer {:?}",
			scroll.bounds(),
			visual.debug_bounds("floating-composer").unwrap(),
		);

		let previous = scroll.offset().y;

		visual.simulate_event(gpui::ScrollEvent {
			position,
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.0), gpui::px(60.0))),
			..Default::default()
		});
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(scroll.offset().y > previous);

		scroll.set_offset(gpui::point(gpui::px(0.), -scroll.max_offset().y));
		visual.simulate_event(gpui::ScrollEvent {
			position,
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(5.))),
			..Default::default()
		});

		assert!(
			surface.read_with(visual, |s, _| s.timeline.follow_paused.contains("agent")),
			"even a small upward wheel step must pause automatic bottom-follow"
		);
	}

	#[gpui::test]
	fn prepending_history_keeps_the_visible_message_at_the_same_position(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(320.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			if let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history {
				entries[1].text = "Existing conversation paragraph. ".repeat(80);
			}
		});

		visual.update(crate::ui_frame::draw_current_frame);

		for _ in 0..40 {
			visual.update(crate::ui_frame::draw_current_frame);
			visual.run_until_parked();
		}

		let before = surface.update(visual, |s, cx| {
			let scroll = s.timeline.scroll["agent"].clone();

			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-50.)));
			s.timeline.follow_paused.insert("agent".into());

			let before = s.timeline.marks[&HistoryKey::Local(1)].position.get()
				+ f32::from(scroll.offset().y);

			s.timeline.older_scroll_anchor = Some(HistoryScrollAnchor {
				work: "agent".into(),
				offset: f32::from(scroll.offset().y),
				maximum: f32::from(scroll.max_offset().y),
				message: Some((
					HistoryKey::Local(1),
					s.timeline.marks[&HistoryKey::Local(1)].position.get(),
				)),
			});

			let Some((_, AgentHistoryResult::Available { entries, .. })) = &s.history else {
				panic!("fixture")
			};
			let mut older = entries[0].clone();

			older.id = -10;
			older.text = "Earlier conversation paragraph. ".repeat(50);

			s.timeline.older_history.insert("agent".into(), (vec![older], None));
			cx.notify();

			before
		});

		for _ in 0..4 {
			visual.update(crate::ui_frame::draw_current_frame);
			visual.run_until_parked();
		}

		surface.read_with(visual, |s, _| {
			let after = s.timeline.marks[&HistoryKey::Local(1)].position.get()
				+ f32::from(s.timeline.scroll["agent"].offset().y);

			assert!(
				(after - before).abs() < 1.,
				"prepend must preserve the reading anchor: {before} -> {after}; offset {:?} max {:?} mark {}",
				s.timeline.scroll["agent"].offset(),
				s.timeline.scroll["agent"].max_offset(),
				s.timeline.marks[&HistoryKey::Local(1)].position.get()
			);
		});
	}

	#[gpui::test]
	fn streaming_follow_uses_the_current_layout_without_a_catch_up_frame(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_000.), gpui::px(500.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			if let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history {
				entries[1].text = "A line of streamed output.\n\n".repeat(40);
			}

			s.timeline.latest_follow_work = Some("agent".into());
		});

		for _ in 0..8 {
			visual.update(crate::ui_frame::draw_current_frame);
		}

		let scroll = surface.read_with(visual, |s, _| s.timeline.scroll["agent"].clone());

		for lines in [42, 43, 48, 50, 45] {
			surface.update(visual, |s, cx| {
				if let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history {
					entries[1].text = "A line of streamed output.\n\n".repeat(lines);
				}

				cx.notify();
			});

			visual.update(crate::ui_frame::draw_current_frame);

			assert!(
				(scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(1.),
				"new content must be anchored before its first paint: offset {:?}, max {:?}",
				scroll.offset(),
				scroll.max_offset()
			);
		}
	}

	#[gpui::test]
	fn scrolling_to_bottom_resumes_follow_for_trackpad_and_wheel(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(320.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.graph_visible = false;
		});
		thread::sleep(std::time::Duration::from_millis(240));
		visual.update(crate::ui_frame::draw_current_frame);
		for delta in [
			gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-10_000.))),
			gpui::ScrollDelta::Lines(gpui::point(0., -1_000.)),
		] {
			surface.update(visual, |s, cx| {
				let scroll = &s.timeline.scroll["agent"];
				scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
				s.timeline.latest_follow_work = None;
				s.timeline.follow_paused.insert("agent".into());
				s.scroll_history(&gpui::ScrollEvent { delta, ..Default::default() }, cx);
				if let Some(wheel) = &mut s.timeline.wheel_scroll {
					wheel.motion.started -= std::time::Duration::from_secs(1);
				}
			});
			visual.update(crate::ui_frame::draw_current_frame);
			surface.read_with(visual, |s, _| {
				assert!(!s.timeline.follow_paused.contains("agent"));
				assert_eq!(s.timeline.latest_follow_work.as_deref(), Some("agent"));
			});
			surface.update(visual, |s, cx| {
				if let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history {
					entries[1].text.push_str(&"\n\nAnother complete output line.".repeat(20));
				}
				cx.notify();
			});
			visual.update(crate::ui_frame::draw_current_frame);
			surface.read_with(visual, |s, _| {
				let scroll = &s.timeline.scroll["agent"];
				assert!((scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(1.));
			});
		}
	}

	#[gpui::test]
	fn jump_to_latest_scrolls_to_bottom_and_resumes_follow(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(320.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			s.timeline.follow_paused.insert("agent".into());
			s.timeline
				.scroll
				.entry("agent".into())
				.or_default()
				.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);

		let button = visual.debug_bounds("jump-to-latest").expect("button while reading history");

		surface.update(visual, |s, cx| {
			s.jump_to_latest(cx);

			if let Some(navigation) = &mut s.timeline.navigation {
				assert!(navigation.id.is_none());

				navigation.started -= std::time::Duration::from_secs(1);
			}

			cx.notify();
		});

		// Let time-based panel transitions settle before checking the final scroll extent.
		thread::sleep(std::time::Duration::from_millis(240));

		for _ in 0..40 {
			visual.update(crate::ui_frame::draw_current_frame);
		}

		surface.read_with(visual, |s, _| {
			let scroll = &s.timeline.scroll["agent"];

			assert!(
				(scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(1.),
				"offset={:?}, max={:?}, button={button:?}",
				scroll.offset(),
				scroll.max_offset()
			);
			assert!(!s.timeline.follow_paused.contains("agent"));
		});
	}

	#[gpui::test]
	fn sending_leaves_old_anchor_and_scrolls_to_latest(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(320.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		visual.update(crate::ui_frame::draw_frame);
		surface.update(visual, |s, cx| {
			s.jump_to_history(HistoryKey::Local(1), cx);
			s.timeline.follow_paused.insert("agent".into());
			s.follow_latest_after_send(cx);

			assert!(s.timeline.selected.is_none());
			assert!(s.timeline.navigation.as_ref().is_some_and(|motion| motion.id.is_none()));
			assert!(s.timeline.latest_follow_work.is_none());
			assert!(s.timeline.follow_paused.contains("agent"));
		});

		// Let the sending transition settle before checking continuous bottom-follow.
		thread::sleep(std::time::Duration::from_millis(300));

		for _ in 0..40 {
			visual.update(crate::ui_frame::draw_frame);
		}

		surface.read_with(visual, |s, _| {
			let scroll = &s.timeline.scroll["agent"];

			assert!((scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(1.));
			assert_eq!(s.active_history_index(scroll), s.timeline.marks.len() - 1);
		});

		assert_eq!(
			activity::current_mark(&[0., 100., f32::INFINITY], 0.),
			0,
			"an unmeasured incoming message must not steal the active timeline marker"
		);
	}

	#[gpui::test]
	fn details_resize_preserves_latest_and_history_reading(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(320.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
		});

		visual.update(crate::ui_frame::draw_current_frame);

		surface.update(visual, |s, cx| {
			let scroll = s.timeline.scroll["agent"].clone();

			scroll.set_offset(gpui::point(gpui::px(0.), -scroll.max_offset().y));

			s.timeline.latest_follow_work = None;

			s.toggle_connection_details(cx);

			assert_eq!(s.timeline.latest_follow_work.as_deref(), Some("agent"));

			// Model the frame between a growing footer's layout and bottom-follow.
			scroll.set_offset(gpui::point(gpui::px(0.), scroll.offset().y + gpui::px(32.)));

			assert_eq!(s.active_history_index(&scroll), s.timeline.marks.len() - 1);
		});

		// Let time-based panel transitions settle before checking the final scroll extent.
		thread::sleep(std::time::Duration::from_millis(240));

		for _ in 0..40 {
			visual.update(crate::ui_frame::draw_current_frame);
		}

		surface.update(visual, |s, cx| {
			let scroll = s.timeline.scroll["agent"].clone();

			assert!((scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(1.));

			s.timeline.latest_follow_work = None;

			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-100.)));

			let before = scroll.offset();
			let active = s.active_history_index(&scroll);

			s.toggle_connection_details(cx);

			assert!(s.timeline.latest_follow_work.is_none());
			assert_eq!(scroll.offset(), before);
			assert_eq!(s.active_history_index(&scroll), active);
		});
	}

	#[test]
	fn navigation_cannot_scroll_past_history_and_dock_magnifies_neighbours() {
		assert_eq!(activity::navigation_offset(5_000.0, 1_000.0), -1_000.0);
		assert_eq!(activity::navigation_offset(0.0, 1_000.0), 0.0);
		assert_eq!(activity::navigation_offset(500.0, 1_000.0), -444.0);
		assert!(activity::dock_influence(3, Some(3)) > activity::dock_influence(2, Some(3)));
		assert!(activity::dock_influence(2, Some(3)) > activity::dock_influence(1, Some(3)));
		assert_eq!(activity::dock_influence(0, Some(3)), 0.0);
	}

	#[test]
	fn navigation_uses_message_positions_not_equal_height_assumptions() {
		assert_eq!(activity::current_mark(&[0.0, 100.0, 1_800.0], 200.0), 1);
		assert_eq!(activity::current_mark(&[0.0, 100.0, 1_800.0], 1_800.0), 2);
		assert_eq!(activity::preview(&"字".repeat(181)).chars().count(), 181);
	}
}
