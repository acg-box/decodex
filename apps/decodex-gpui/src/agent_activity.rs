//! Compact, anchored navigation through saved conversation turns.
use std::{
	cell::Cell,
	collections::{BTreeMap, BTreeSet},
	fmt::{Display, Formatter},
	rc::Rc,
};

use gpui::{
	AnyElement, KeyDownEvent, Pixels, Role, ScrollHandle, ScrollWheelEvent, canvas, point, size,
};
use ui_theme::{BLUE, BODY_LINE_HEIGHT, SURFACE_OVERLAY_MATERIAL, TEXT, TEXT_MUTED};

use crate::{
	shell::{
		agent_surface::{native_timeline, *},
		workspace_symbols,
		workspace_symbols::Symbol,
	},
	ui_motion, ui_scroll,
};
use decodex_protocol::{AgentHistoryEntryDto, AgentTimelineEntry};

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
		let native = self.snapshot.as_ref().is_some_and(|snapshot| {
			snapshot.work_items.iter().any(|work| {
				Some(&work.id) == self.selected.as_ref() && self.native_history_active(work)
			})
		});
		let work = self.selected.clone().map(|work| (work, native));

		if self.history_marks_work != work {
			self.wheel_scroll = None;

			self.history_marks.clear();

			self.history_selected = None;
			self.history_hover = None;
			self.history_navigation = None;
			self.history_marks_work = work;
			self.history_marks_revision = None;
		}
		if native {
			self.prepare_native_history_marks();

			return;
		}

		let Some((work, AgentHistoryResult::Available { entries, .. })) =
			self.history.as_ref().filter(|(work, _)| Some(work) == self.selected.as_ref())
		else {
			self.history_marks.clear();

			return;
		};
		let mut saved = BTreeMap::new();

		if let Some((older, _)) = self.older_history.get(work) {
			for entry in older {
				saved.insert(entry.id, entry);
			}
		}

		for entry in entries {
			saved.insert(entry.id, entry);
		}

		self.transcript_scroll.entry(work.clone()).or_default();
		self.history_marks
			.retain(|id, _| matches!(id, HistoryKey::Local(id) if saved.contains_key(id)));

		let mut current = None;

		for entry in saved.values() {
			if entry.kind == "user" || entry.kind == "instruction" {
				let mark =
					self.history_marks.entry(HistoryKey::Local(entry.id)).or_insert_with(|| {
						HistoryMark {
							position: Rc::new(Cell::new(f32::INFINITY)),
							hit_bounds: Rc::new(Cell::new(None)),
							question: preview(&entry.text),
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
				self.history_marks.get_mut(id).expect("current mark").answer = preview(&entry.text);
			}
		}
	}

	pub(super) fn anchored_history_entry(
		&self,
		entry: &AgentHistoryEntryDto,
		work: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut body = history_entry(entry).into_any_element();

		if entry.kind == "assistant"
			&& let Some(action) =
				self.voice_read_action(work, &format!("saved-{}", entry.id), &entry.text, true, cx)
		{
			body = div().child(body).child(action).into_any_element();
		}

		self.anchor_history_row(&HistoryKey::Local(entry.id), body)
	}

	fn prepare_native_history_marks(&mut self) {
		use decodex_protocol::AgentTimelineContent as Content;

		let Some(binding) = &self.native_history.binding else {
			return;
		};
		let revision = (self.native_history.epoch, self.native_history.revision);

		if self.history_marks_revision == Some(revision) {
			return;
		}

		self.history_marks_revision = Some(revision);

		self.transcript_scroll.entry(binding.work.clone()).or_default();

		let mut retained = BTreeSet::new();
		let mut current = None;

		for entry in &self.native_history.entries {
			let (user, text, label) = match &entry.content {
				Content::Item { kind, text, .. }
					if kind == "userMessage" || kind == "agentMessage" =>
					(kind == "userMessage", text, "Conversation message"),
				Content::Speech { role, text, .. } => (role == "user", text, "Voice message"),
				_ => continue,
			};

			if user {
				let key = HistoryKey::native(&binding.thread, entry);

				retained.insert(key.clone());

				let mark = self.history_marks.entry(key.clone()).or_insert_with(|| HistoryMark {
					position: Rc::new(Cell::new(0.0)),
					hit_bounds: Rc::new(Cell::new(None)),
					question: String::new(),
					time: label.into(),
					answer: String::new(),
				});

				mark.question = preview(text);

				mark.answer.clear();

				current = Some(key);
			} else if let Some(key) = &current {
				self.history_marks.get_mut(key).expect("current native mark").answer =
					preview(text);
			}
		}

		self.history_marks.retain(|key, _| retained.contains(key));

		if self.history_selected.as_ref().is_some_and(|key| !retained.contains(key)) {
			self.history_selected = None;
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
		let Some(mark) = self.history_marks.get(key) else {
			return row;
		};
		let position = mark.position.clone();
		let scroll = self
			.transcript_scroll
			.get(self.selected.as_deref().unwrap_or_default())
			.cloned()
			.unwrap_or_default();

		div()
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
		self.latest_follow_work = None;

		self.cancel_native_scroll_anchor();
		self.set_voice_follow(false);

		if let Some(mark) = self.history_marks.get(&id)
			&& let Some(work) = self.selected.as_ref()
			&& let Some(scroll) = self.transcript_scroll.get(work)
		{
			self.history_selected = Some(id.clone());
			self.history_navigation = Some(HistoryNavigation {
				work: work.clone(),
				id: Some(id),
				from: scroll.offset().y.into(),
				to: navigation_offset(mark.position.get(), scroll.max_offset().y.into()),
				started: std::time::Instant::now(),
			});

			cx.notify();
		}
	}

	pub(super) fn scroll_history(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
		let delta = event.delta.pixel_delta(px(BODY_LINE_HEIGHT));
		// macOS also sends phase-only and horizontal gesture events. They do
		// not move the transcript and must not cancel its current scroll state.
		if delta.y == px(0.) {
			return;
		}

		self.latest_follow_work = None;

		self.cancel_native_scroll_anchor();

		self.history_navigation = None;
		self.history_selected = None;

		if let Some(scroll) = self.selected.as_ref().and_then(|id| self.transcript_scroll.get(id)) {
			let smooth = ui_scroll::smooth(event.delta);

			if smooth {
				let now = std::time::Instant::now();
				let current = f32::from(scroll.offset().y);
				let wheel = self.wheel_scroll.get_or_insert_with(|| WheelScroll {
					work: self.selected.clone().unwrap_or_default(),
					motion: crate::ui_scroll::Motion::new(current, now),
				});

				wheel.motion.retarget(current, delta.y.into(), scroll.max_offset().y.into(), now);
			} else {
				self.wheel_scroll = None;

				let offset = (scroll.offset().y + delta.y).clamp(-scroll.max_offset().y, px(0.));

				scroll.set_offset(point(px(0.), offset));
			}

			let following = !smooth
				&& delta.y < px(0.)
				&& (scroll.offset().y + scroll.max_offset().y).abs() < px(1.);

			if let Some(id) = &self.selected {
				if following {
					self.history_follow_paused.remove(id);
				} else {
					self.history_follow_paused.insert(id.clone());
				}
			}

			self.set_voice_follow(following);

			if delta.y > px(0.) {
				self.prefetch_older_history(cx);
			}

			cx.stop_propagation();
			cx.notify();
		}
	}

	pub(super) fn animate_history_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.history_navigation.is_some() || self.latest_follow_work.is_some() {
			self.wheel_scroll = None;
		}

		if let Some(wheel) = &self.wheel_scroll {
			if self.selected.as_ref() != Some(&wheel.work) {
				self.wheel_scroll = None;
			} else if let Some(scroll) = self.transcript_scroll.get(&wheel.work) {
				let (offset, moving) = if ui_scroll::enabled() {
					wheel.motion.sample(std::time::Instant::now())
				} else {
					(wheel.motion.to, false)
				};
				let offset = offset.clamp(-f32::from(scroll.max_offset().y).max(0.), 0.);

				scroll.set_offset(point(px(0.), px(offset)));

				if moving {
					ui_motion::request_frame(window, cx);

					cx.notify();
				} else {
					let following = wheel.motion.to < wheel.motion.from
						&& (offset + f32::from(scroll.max_offset().y)).abs() < 1.;

					if following {
						self.history_follow_paused.remove(&wheel.work);
						self.set_voice_follow(true);
					}

					self.wheel_scroll = None;
				}
			}
		}

		let Some(navigation) = &self.history_navigation else {
			return;
		};

		if self.selected.as_ref() != Some(&navigation.work)
			|| navigation.id.as_ref().is_some_and(|id| !self.history_marks.contains_key(id))
		{
			self.history_navigation = None;

			return;
		}

		let t = (navigation.started.elapsed().as_secs_f32() / 0.28).min(1.0);

		if let Some(scroll) = self.transcript_scroll.get(&navigation.work) {
			let target = match navigation.id.as_ref() {
				None => -f32::from(scroll.max_offset().y),
				Some(id) => self.history_marks.get(id).map_or(navigation.to, |m| {
					navigation_offset(m.position.get(), scroll.max_offset().y.into())
				}),
			};
			let offset = navigation.from + (target - navigation.from) * (1.0 - (1.0 - t).powi(3));

			scroll.set_offset(point(
				px(0.0),
				px(offset.clamp(-f32::from(scroll.max_offset().y).max(0.0), 0.0)),
			));
		}

		if t < 1.0 {
			ui_motion::request_frame(window, cx);

			cx.notify();
		} else {
			let (work, id, started) =
				(navigation.work.clone(), navigation.id.clone(), navigation.started);
			let surface = cx.entity().downgrade();

			cx.defer(move |cx| {
				let _ = surface.update(cx, |s, cx| {
					if !s.history_navigation.as_ref().is_some_and(|current| {
						current.work == work && current.id == id && current.started == started
					}) {
						return;
					}
					if id.is_none() {
						s.latest_follow_work = Some(work.clone());

						s.history_follow_paused.remove(&work);
						s.set_voice_follow(true);
					}
					if s.selected.as_ref() == Some(&work)
						&& let (Some(mark), Some(scroll)) = (
							id.as_ref().and_then(|id| s.history_marks.get(id)),
							s.transcript_scroll.get(&work),
						) {
						scroll.set_offset(point(
							scroll.offset().x,
							px(navigation_offset(
								mark.position.get(),
								scroll.max_offset().y.into(),
							)),
						));
					}

					s.history_navigation = None;

					cx.notify();
				});
			});
		}
	}

	pub(super) fn toggle_connection_details(&mut self, cx: &mut Context<Self>) {
		// A footer resize is not conversation navigation. Preserve bottom-follow before
		// the new footer height changes the scroll range, or keep the reader's offset.
		if let Some(work) = self.selected.as_ref()
			&& let Some(scroll) = self.transcript_scroll.get(work)
			&& (scroll.offset().y + scroll.max_offset().y).abs() < px(1.)
		{
			self.latest_follow_work = Some(work.clone());
		}

		self.connection_details_expanded = !self.connection_details_expanded;

		cx.notify();
	}

	pub(super) fn follow_latest_after_send(&mut self, cx: &mut Context<Self>) {
		self.latest_follow_work = self.selected.clone();
		self.history_selected = None;
		self.history_navigation = None;
		self.older_scroll_anchor = None;

		if let Some(work) = &self.selected {
			self.history_follow_paused.remove(work);
			self.transcript_scroll.entry(work.clone()).or_default();
		}

		self.set_voice_follow(true);
		cx.notify();
	}

	pub(super) fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
		let Some(work) = self.selected.clone() else {
			return;
		};
		let Some(scroll) = self.transcript_scroll.get(&work) else {
			return;
		};

		self.history_selected = None;

		self.history_follow_paused.insert(work.clone());

		self.history_navigation = Some(HistoryNavigation {
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
		let visible = self
			.selected
			.as_ref()
			.and_then(|id| self.transcript_scroll.get(id))
			.is_some_and(|scroll| scroll.max_offset().y + scroll.offset().y > px(48.));
		let working = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| {
				snapshot.work_items.iter().find(|work| Some(&work.id) == self.selected.as_ref())
			})
			.is_some_and(|work| {
				matches!(
					work.dispatch_state,
					AgentDispatchStateDto::Dispatching | AgentDispatchStateDto::Running
				) && !self.thread_in_use(&work.id)
			});
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

		div()
			.absolute()
			.left_0()
			.right_0()
			.flex()
			.justify_center()
			.bottom(px(if self.selected_is_manager() {
				self.composer_footer_height + 8.
			} else {
				12.
			}))
			.when(opacity > 0.001, |d| {
				d.child(
					div()
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
						.h(px(28.))
						.w(px(width))
						.rounded_full()
						.bg(rgba(SURFACE_OVERLAY_MATERIAL))
						.flex()
						.items_center()
						.justify_center()
						.gap(px(5.))
						.opacity(opacity)
						.cursor_pointer()
						.hover(|d| d.bg(rgba(0x302d397c)))
						.on_click(cx.listener(|s, _, _, cx| s.jump_to_latest(cx)))
						.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
							if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
								s.jump_to_latest(cx);
								cx.stop_propagation();
							}
						}))
						.when(working, |d| {
							d.child(div().flex().items_center().gap(px(2.)).children((0..3).map(
								|i| {
									div().size(px(3.)).rounded_full().bg(rgb(BLUE)).opacity(
										0.35 + 0.65 * ((phase - i as f32 * 0.7).sin() + 1.) / 2.,
									)
								},
							)))
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
		// History nodes are transient while switching agents. Only the explicit
		// visibility control may resize the rail and move the transcript.
		ui_motion::reveal(
			"history-rail-reveal",
			if self.timeline_visible { 44.0 } else { 0.0 },
			true,
			self.history_rail(window, cx),
		)
		.into_any_element()
	}

	fn active_history_index(&self, scroll: &ScrollHandle) -> usize {
		let last = self.history_marks.len().saturating_sub(1);
		let at_end = scroll.max_offset().y > px(0.)
			&& (scroll.offset().y + scroll.max_offset().y).abs() < px(1.);

		self.history_selected
			.as_ref()
			.and_then(|id| self.history_marks.keys().position(|key| key == id))
			.unwrap_or_else(|| {
				if at_end || (self.selected.is_some() && self.latest_follow_work == self.selected) {
					last
				} else {
					current_mark(
						&self.history_marks.values().map(|m| m.position.get()).collect::<Vec<_>>(),
						-f32::from(scroll.offset().y),
					)
				}
			})
	}

	pub(super) fn history_rail(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		if self.history_marks.is_empty() {
			return div().into_any_element();
		}

		let scroll = self
			.transcript_scroll
			.get(self.selected.as_deref().unwrap_or_default())
			.cloned()
			.unwrap_or_default();
		let positions: Vec<_> =
			self.history_marks.values().map(|mark| mark.position.clone()).collect();
		let working = self.snapshot.as_ref().is_some_and(|snapshot| {
			snapshot.work_items.iter().any(|work| {
				Some(&work.id) == self.selected.as_ref()
					&& work.dispatch_state == AgentDispatchStateDto::Running
			})
		});
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
		let spacing = ((f32::from(scroll.bounds().size.height) - 32.0) / positions.len() as f32)
			.clamp(2.0, 11.0);
		let mut rail = div()
			.id("conversation-history-rail")
			.w(px(44.0))
			.flex_none()
			.h_full()
			.py_4()
			.flex()
			.flex_col()
			.justify_center()
			.overflow_hidden();

		for (index, (id, mark)) in self.history_marks.iter().enumerate() {
			let influence = ui_motion::value(
				SharedString::from(format!(
					"rail-magnify-{}-{id}",
					self.selected.as_deref().unwrap_or_default()
				)),
				dock_influence(index, self.history_hover),
				window,
				cx,
			);
			let tip = mark.clone();
			let id = id.clone();
			let keyboard_id = id.clone();
			let hit_bounds = mark.hit_bounds.clone();

			rail = rail.child(
				div()
					.id(SharedString::from(format!("history-tick-{id}")))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Jump to: {}", mark.question))
					.w_full()
					.flex_none()
					.h(px(spacing))
					.cursor_pointer()
					.on_hover(cx.listener(move |s, hovered: &bool, _, cx| {
						if *hovered {
							s.history_hover = Some(index);
						} else if s.history_hover == Some(index) {
							s.history_hover = None;
						}

						cx.notify();
					}))
					.tooltip(move |_, cx| cx.new(|_| HistoryPreview(tip.clone())).into())
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
						canvas(
							move |bounds, _, _| hit_bounds.set(Some(bounds)),
							move |bounds, _, window, _| {
								let activity =
									(1. - (index as f32 - active_position).abs()).clamp(0., 1.);
								let color = if working && index == last {
									rgb(BLUE)
								} else {
									rgba((TEXT << 8) | (100. + 155. * activity) as u32)
								};
								let width = 7.0 + influence * 16.0;
								let height = 2.0 + influence;

								window.paint_quad(gpui::fill(
									gpui::Bounds::new(
										point(
											bounds.center().x - px(width / 2.0),
											bounds.center().y - px(height / 2.),
										),
										size(px(width), px(height)),
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

pub(super) struct WheelScroll {
	work: String,
	motion: crate::ui_scroll::Motion,
}

struct HistoryPreview(HistoryMark);
impl Render for HistoryPreview {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.w(px(320.0))
			.p_3()
			.rounded(px(10.0))
			.bg(rgba(0x24242af5))
			.border_1()
			.border_color(rgba(0xffffff16))
			.flex()
			.flex_col()
			.gap_2()
			.text_size(px(12.0))
			.line_height(px(18.0))
			.child(div().text_size(px(10.0)).text_color(rgb(TEXT_MUTED)).child(self.0.time.clone()))
			.child(div().text_color(rgb(TEXT)).child(self.0.question.clone()))
			.when(!self.0.answer.is_empty(), |panel| {
				panel.child(div().text_color(rgb(TEXT_MUTED)).child(self.0.answer.clone()))
			})
	}
}

fn navigation_offset(position: f32, maximum: f32) -> f32 {
	(-(position - HISTORY_ANCHOR_INSET)).clamp(-maximum.max(0.0), 0.0)
}

fn dock_influence(index: usize, hover: Option<usize>) -> f32 {
	hover.map_or(0.0, |hover| (1.0 - index.abs_diff(hover) as f32 / 3.0).max(0.0).powi(2))
}

fn preview(text: &str) -> String {
	let text = markdown::plain_text(text);
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
	use crate::shell::agent_surface::activity::*;

	use std::thread;

	#[gpui::test]
	fn agent_loading_keeps_transcript_horizontal_bounds(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(900.)));

		let saved = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			(s.selected.clone(), s.history.clone())
		});

		visual.update(|w, cx| w.draw(cx).clear());

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| w.draw(cx).clear());

		let original = visual.debug_bounds("workspace-transcript").unwrap();

		for loading in [true, false] {
			surface.update(visual, |s, cx| {
				s.selected = if loading { Some("release".into()) } else { saved.0.clone() };
				s.history = if loading { None } else { saved.1.clone() };

				cx.notify();
			});

			for delay in [0, 100, 140] {
				thread::sleep(std::time::Duration::from_millis(delay));

				visual.update(|w, cx| w.draw(cx).clear());

				let bounds = visual.debug_bounds("workspace-transcript").unwrap();

				assert_eq!(
					bounds.origin.x, original.origin.x,
					"history loading must not move text"
				);
				assert_eq!(bounds.size.width, original.size.width);
			}
		}
		// The explicit toggle still controls the reserved rail width.
		surface.update(visual, |s, cx| {
			s.timeline_visible = false;

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear());

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| w.draw(cx).clear());

		assert!(
			visual.debug_bounds("workspace-transcript").unwrap().size.width > original.size.width
		);
	}

	#[gpui::test]
	fn deferred_navigation_finish_does_not_end_a_newer_jump(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		surface.update(visual, |s, cx| {
			s.jump_to_history(HistoryKey::Local(1), cx);

			s.history_navigation.as_mut().unwrap().started -= std::time::Duration::from_secs(1);
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
			surface.update(cx, |s, cx| s.jump_to_history(HistoryKey::Local(3), cx));
		});

		surface.update(visual, |s, cx| {
			assert_eq!(s.history_navigation.as_ref().unwrap().id, Some(HistoryKey::Local(3)));

			s.history_navigation.as_mut().unwrap().started -= std::time::Duration::from_secs(1);

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		surface.read_with(visual, |s, _| {
			assert!(s.history_navigation.is_none());
			assert_eq!(s.history_selected, Some(HistoryKey::Local(3)));
		});
	}

	#[gpui::test]
	fn native_jump_finishes_against_layout_after_an_older_page_arrives(
		cx: &mut gpui::TestAppContext,
	) {
		use decodex_protocol::{
			AgentTimelineContent as Content, AgentTimelineEntry, AgentTimelinePage,
		};

		let row = |position, user| AgentTimelineEntry {
			position,
			content: Content::Item {
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
		};
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(400.)));

		let binding = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

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

			assert!(s.native_history.replace(
				binding.clone(),
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: vec![row(10, true), row(11, false), row(20, true), row(21, false)],
					next_cursor: Some("older".into()),
					weather: Default::default(),
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				}
			));

			cx.notify();

			binding
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let target = HistoryKey::native("thread", &row(20, true));
		let bounds =
			surface.read_with(visual, |s, _| s.history_marks[&target].hit_bounds.get().unwrap());

		visual.simulate_click(bounds.center(), Default::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.history_navigation.as_ref().unwrap().id, Some(target.clone()));

			s.history_navigation.as_mut().unwrap().started -= std::time::Duration::from_secs(1);

			assert!(s.prepend_native_history(
				&binding,
				"older",
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: vec![row(1, false)],
					next_cursor: None,
					weather: Default::default(),
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				}
			));

			cx.notify();
		});

		for _ in 0..3 {
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			visual.run_until_parked();
		}

		surface.read_with(visual, |s, _| {
			let scroll = &s.transcript_scroll[&binding.work];
			let expected = navigation_offset(
				s.history_marks[&target].position.get(),
				scroll.max_offset().y.into(),
			);
			let actual = f32::from(scroll.offset().y);

			assert!(
				(actual - expected).abs() < 1.,
				"jump stopped at {actual}, target is {expected}"
			);
			assert!(s.history_navigation.is_none());
			assert_eq!(s.history_selected.as_ref(), Some(&target));
		});
	}

	#[gpui::test]
	fn native_rail_keeps_same_position_speech_and_text_distinct_and_retires_evicted_targets(
		cx: &mut gpui::TestAppContext,
	) {
		use decodex_protocol::{
			AgentTimelineContent as Content, AgentTimelineEntry, AgentTimelinePage,
		};

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(400.)));

		let keys = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

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
					content: Content::Item {
						phase: None,
						app_ui: false,
						turn_id: "turn".into(),
						item_id: "same-id".into(),
						kind: "userMessage".into(),
						text: text.clone(),
						truncated: false,
						activity: None,
						attachments: vec![],
					},
				},
				AgentTimelineEntry {
					position: 5,
					content: Content::Speech {
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

			assert!(s.native_history.replace(
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
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None
				}
			));

			cx.notify();

			keys
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let bounds = surface.read_with(visual, |s, _| {
			assert_eq!(s.history_marks.len(), 2);
			assert_ne!(keys[0], keys[1]);
			assert!(
				s.history_marks[&keys[1]].position.get() > s.history_marks[&keys[0]].position.get()
			);

			s.history_marks[&keys[0]].hit_bounds.get().unwrap()
		});

		visual.simulate_click(bounds.center(), Default::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.history_selected.as_ref(), Some(&keys[0]));
			assert_eq!(s.history_navigation.as_ref().unwrap().id, Some(keys[0].clone()));

			s.native_history.entries.remove(0);

			s.native_history.revision += 1;

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		surface.read_with(visual, |s, _| {
			assert!(s.history_navigation.is_none() && s.history_selected.is_none());
			assert!(!s.history_marks.contains_key(&keys[0]));
			assert!(s.history_marks.contains_key(&keys[1]));
		});
	}

	#[gpui::test]
	#[ignore = "Manual CPU draw benchmark; use --release, not a GPU FPS measurement"]
	fn long_history_scroll_draw_benchmark(cx: &mut gpui::TestAppContext) {
		use decodex_protocol::{AgentTimelineContent as Content, AgentTimelineEntry};

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(900.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

			let work = s.snapshot.as_mut().unwrap().work_items.iter_mut().find(|w| w.id == "agent").unwrap();

			work.codex_thread_id = Some("benchmark-thread".into());

			let entries = (0..300).map(|i| AgentTimelineEntry {
				position: i,
				content: if i % 3 == 2 { Content::TurnBoundary {
					turn_id: format!("turn-{}", i / 3), completed: true, status: Some("completed".into()),
					duration_ms: Some(3_200), usage: None, usage_summary: None, error: None,
				} } else { Content::Item { phase: None,
					app_ui: false,
					turn_id: format!("turn-{}", i / 3), item_id: format!("message-{i}"),
					kind: if i % 3 == 0 { "userMessage" } else { "agentMessage" }.into(),
					text: format!("## Message {i}\n\n{}", "Review **the result** and `src/main.rs`.\n\n- Keep history readable.\n- Preserve the scroll position.\n\n".repeat(4)),
					truncated: false, activity: None, attachments: vec![],
				} },
			}).collect();

			assert!(s.native_history.replace(super::super::native_timeline::Binding {
				work: "agent".into(), thread: "benchmark-thread".into(), account: "benchmark".into(),
			}, decodex_protocol::AgentTimelinePage {
				thread_id: "benchmark-thread".into(), entries, next_cursor: None,
				weather: Default::default(), safety_buffering_turn_id: None, active_realtime_session_at_page_start: None,
			}));

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear());

		let anchors = surface.read_with(visual, |s, _| {
			s.history_marks
				.iter()
				.map(|(key, mark)| (key.clone(), mark.position.get()))
				.collect::<BTreeMap<_, _>>()
		});
		let mut samples = Vec::new();
		let mut maximum = None;

		for frame in 0..70 {
			surface.update(visual, |s, cx| {
				s.latest_follow_work = None;

				s.history_follow_paused.insert("agent".into());
				s.transcript_scroll["agent"]
					.set_offset(point(px(0.), px(-2_000. - frame as f32 * 17.)));
				cx.notify();
			});

			let start = std::time::Instant::now();

			visual.update(|w, cx| w.draw(cx).clear());

			if frame >= 10 {
				samples.push(start.elapsed().as_secs_f64() * 1_000.);

				let current =
					surface.read_with(visual, |s, _| s.transcript_scroll["agent"].max_offset().y);

				if let Some(expected) = maximum {
					assert_eq!(current, expected, "windowing must retain exact scroll extent");
				}

				maximum = Some(current);

				surface.read_with(visual, |s, _| {
					for (key, expected) in &anchors {
						assert!(
							(s.history_marks[key].position.get() - expected).abs() < 0.5,
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
			assert_eq!(current_mark(&positions, -navigation_offset(*position, 3_000.)), index);
		}
	}

	#[gpui::test]
	fn rail_hit_targets_select_their_own_message(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;
		});

		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		let ids =
			surface.read_with(visual, |s, _| s.history_marks.keys().cloned().collect::<Vec<_>>());

		for id in ids {
			let bounds =
				surface.read_with(visual, |s, _| s.history_marks[&id].hit_bounds.get().unwrap());

			visual.simulate_mouse_down(
				bounds.center(),
				gpui::MouseButton::Left,
				Default::default(),
			);
			visual.simulate_mouse_up(bounds.center(), gpui::MouseButton::Left, Default::default());

			surface.update(visual, |s, cx| {
				assert_eq!(s.history_selected, Some(id.clone()));
				assert_eq!(s.history_navigation.as_ref().unwrap().id, Some(id.clone()));

				s.history_navigation.as_mut().unwrap().started -= std::time::Duration::from_secs(1);

				cx.notify();
			});

			visual.update(|w, cx| {
				w.draw(cx).clear();
			});
			surface.update(visual, |s, _| assert_eq!(s.history_selected, Some(id.clone())));
		}
	}

	#[gpui::test]
	fn history_marks_jump_to_real_message_anchors(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.0), px(320.0)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		visual.update(|_, cx| {
			surface.update(cx, |s, cx| {
				assert_eq!(s.history_marks.len(), 2);
				assert!(
					s.history_marks[&HistoryKey::Local(3)].position.get()
						> s.history_marks[&HistoryKey::Local(1)].position.get()
				);

				s.jump_to_history(HistoryKey::Local(3), cx);

				s.history_navigation.as_mut().unwrap().started -= std::time::Duration::from_secs(1);

				cx.notify();
			})
		});

		visual.run_until_parked();
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		surface.update(visual, |s, _| {
			assert!(s.transcript_scroll["agent"].offset().y < px(0.0));
			assert!(
				s.transcript_scroll["agent"].offset().y
					>= -s.transcript_scroll["agent"].max_offset().y
			);
			assert_eq!(s.history_marks.len(), 2);
		});
	}

	#[gpui::test]
	fn mouse_wheel_animates_and_trackpad_takes_over_immediately(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(300.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		surface.update(visual, |s, cx| {
			let scroll = s.transcript_scroll["agent"].clone();

			scroll.set_offset(point(px(0.), px(-50.)));

			let event = gpui::ScrollWheelEvent {
				delta: gpui::ScrollDelta::Lines(point(0., -1.)),
				..Default::default()
			};

			s.scroll_history(&event, cx);

			assert_eq!(scroll.offset().y, px(-50.), "notches must not jump immediately");

			let first = s.wheel_scroll.as_ref().unwrap().motion.to;

			s.scroll_history(&event, cx);

			let wheel = s.wheel_scroll.as_ref().unwrap();

			assert!(wheel.motion.to < first, "successive notches accumulate");

			let midpoint =
				wheel.motion.sample(wheel.motion.started + std::time::Duration::from_millis(60)).0;

			assert!(midpoint < -50. && midpoint > wheel.motion.to);

			scroll.set_offset(point(px(0.), px(midpoint)));
			s.scroll_history(
				&gpui::ScrollWheelEvent {
					delta: gpui::ScrollDelta::Lines(point(0., 1.)),
					..Default::default()
				},
				cx,
			);

			assert!(
				s.wheel_scroll.as_ref().unwrap().motion.to > midpoint,
				"reversal cancels pending forward travel"
			);

			s.scroll_history(
				&gpui::ScrollWheelEvent {
					delta: gpui::ScrollDelta::Pixels(point(px(0.), px(0.25))),
					..Default::default()
				},
				cx,
			);

			assert!(s.wheel_scroll.is_none());
			assert_eq!(scroll.offset().y, px(midpoint + 0.25));
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

		visual.simulate_resize(size(px(1_400.), px(300.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		surface.update(visual, |s, cx| {
			let scroll = s.transcript_scroll["agent"].clone();

			scroll.set_offset(point(px(0.), px(-40.)));

			s.latest_follow_work = Some("agent".into());

			s.history_follow_paused.remove("agent");

			for phase in [gpui::TouchPhase::Started, gpui::TouchPhase::Ended] {
				s.scroll_history(
					&gpui::ScrollWheelEvent {
						delta: gpui::ScrollDelta::Pixels(point(px(3.), px(0.))),
						touch_phase: phase,
						..Default::default()
					},
					cx,
				);

				assert_eq!(s.latest_follow_work.as_deref(), Some("agent"));
				assert!(!s.history_follow_paused.contains("agent"));
				assert_eq!(scroll.offset().y, px(-40.));
			}
			for _ in 0..8 {
				s.scroll_history(
					&gpui::ScrollWheelEvent {
						delta: gpui::ScrollDelta::Pixels(point(px(0.), px(0.25))),
						..Default::default()
					},
					cx,
				);
			}

			assert_eq!(
				scroll.offset().y,
				px(-38.),
				"precise deltas accumulate without rounding or duplication"
			);
			assert!(s.history_follow_paused.contains("agent"));
		});
	}

	#[gpui::test]
	fn wheel_scroll_keeps_adjacent_messages_accessible(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.0), px(300.0)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		// Panel animation uses wall time, including in optimized test builds.
		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|window, cx| window.draw(cx).clear());

		let scroll = surface.read_with(visual, |s, _| s.transcript_scroll["agent"].clone());

		assert!(scroll.max_offset().y >= px(100.), "fixture must allow the full wheel delta");
		assert!(
			scroll.bounds().bottom() > px(280.),
			"history must extend behind the floating composer instead of clipping above it"
		);

		let position = scroll.bounds().center();

		visual.simulate_event(gpui::ScrollWheelEvent {
			position,
			delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-100.0))),
			..Default::default()
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert_eq!(scroll.offset().y, px(-100.0), "wheel delta must be applied once");

		let previous = scroll.offset().y;

		visual.simulate_event(gpui::ScrollWheelEvent {
			position,
			delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(60.0))),
			..Default::default()
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(scroll.offset().y > previous);

		scroll.set_offset(point(px(0.), -scroll.max_offset().y));
		visual.simulate_event(gpui::ScrollWheelEvent {
			position,
			delta: gpui::ScrollDelta::Pixels(point(px(0.), px(5.))),
			..Default::default()
		});

		assert!(
			surface.read_with(visual, |s, _| s.history_follow_paused.contains("agent")),
			"even a small upward wheel step must pause automatic bottom-follow"
		);
	}

	#[gpui::test]
	fn prepending_history_keeps_the_visible_message_at_the_same_position(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(320.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

			if let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history {
				entries[1].text = "Existing conversation paragraph. ".repeat(80);
			}
		});

		visual.update(|window, cx| window.draw(cx).clear());

		for _ in 0..40 {
			visual.update(|window, cx| window.draw(cx).clear());
			visual.run_until_parked();
		}

		let before = surface.update(visual, |s, cx| {
			let scroll = s.transcript_scroll["agent"].clone();

			scroll.set_offset(point(px(0.), px(-50.)));
			s.history_follow_paused.insert("agent".into());

			let before = s.history_marks[&HistoryKey::Local(1)].position.get()
				+ f32::from(scroll.offset().y);

			s.older_scroll_anchor = Some(HistoryScrollAnchor {
				work: "agent".into(),
				offset: f32::from(scroll.offset().y),
				maximum: f32::from(scroll.max_offset().y),
				message: Some((
					HistoryKey::Local(1),
					s.history_marks[&HistoryKey::Local(1)].position.get(),
				)),
			});

			let Some((_, AgentHistoryResult::Available { entries, .. })) = &s.history else {
				panic!("fixture")
			};
			let mut older = entries[0].clone();

			older.id = -10;
			older.text = "Earlier conversation paragraph. ".repeat(50);

			s.older_history.insert("agent".into(), (vec![older], None));
			cx.notify();

			before
		});

		for _ in 0..4 {
			visual.update(|window, cx| window.draw(cx).clear());
			visual.run_until_parked();
		}

		surface.read_with(visual, |s, _| {
			let after = s.history_marks[&HistoryKey::Local(1)].position.get()
				+ f32::from(s.transcript_scroll["agent"].offset().y);

			assert!(
				(after - before).abs() < 1.,
				"prepend must preserve the reading anchor: {before} -> {after}; offset {:?} max {:?} mark {}",
				s.transcript_scroll["agent"].offset(),
				s.transcript_scroll["agent"].max_offset(),
				s.history_marks[&HistoryKey::Local(1)].position.get()
			);
		});
	}

	#[gpui::test]
	fn jump_to_latest_scrolls_to_bottom_and_resumes_follow(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(320.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

			s.history_follow_paused.insert("agent".into());
			s.transcript_scroll
				.entry("agent".into())
				.or_default()
				.set_offset(point(px(0.), px(0.)));
			cx.notify();
		});

		visual.update(|window, cx| window.draw(cx).clear());
		visual.run_until_parked();
		visual.update(|window, cx| window.draw(cx).clear());

		let button = visual.debug_bounds("jump-to-latest").expect("button while reading history");

		surface.update(visual, |s, cx| {
			s.jump_to_latest(cx);

			if let Some(navigation) = &mut s.history_navigation {
				assert!(navigation.id.is_none());

				navigation.started -= std::time::Duration::from_secs(1);
			}

			cx.notify();
		});
		// Let time-based panel transitions settle before checking the final scroll extent.
		thread::sleep(std::time::Duration::from_millis(240));

		for _ in 0..40 {
			visual.update(|window, cx| window.draw(cx).clear());
		}

		surface.read_with(visual, |s, _| {
			let scroll = &s.transcript_scroll["agent"];

			assert!(
				(scroll.offset().y + scroll.max_offset().y).abs() < px(1.),
				"offset={:?}, max={:?}, button={button:?}",
				scroll.offset(),
				scroll.max_offset()
			);
			assert!(!s.history_follow_paused.contains("agent"));
		});
	}

	#[gpui::test]
	fn sending_leaves_old_anchor_and_scrolls_to_latest(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(320.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;
		});

		visual.update(|window, cx| window.draw(cx).clear());
		surface.update(visual, |s, cx| {
			s.jump_to_history(HistoryKey::Local(1), cx);
			s.history_follow_paused.insert("agent".into());
			s.follow_latest_after_send(cx);

			assert!(s.history_selected.is_none());
			assert!(s.history_navigation.is_none());
			assert!(!s.history_follow_paused.contains("agent"));
		});
		// Let time-based panel transitions settle before checking the final scroll extent.
		thread::sleep(std::time::Duration::from_millis(240));

		for _ in 0..40 {
			visual.update(|window, cx| window.draw(cx).clear());
		}

		surface.read_with(visual, |s, _| {
			let scroll = &s.transcript_scroll["agent"];

			assert!((scroll.offset().y + scroll.max_offset().y).abs() < px(1.));
			assert_eq!(s.active_history_index(scroll), s.history_marks.len() - 1);
		});

		assert_eq!(
			current_mark(&[0., 100., f32::INFINITY], 0.),
			0,
			"an unmeasured incoming message must not steal the active timeline marker"
		);
	}

	#[gpui::test]
	fn details_resize_preserves_latest_and_history_reading(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(size(px(1_400.), px(320.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;
		});

		visual.update(|w, cx| w.draw(cx).clear());

		surface.update(visual, |s, cx| {
			let scroll = s.transcript_scroll["agent"].clone();

			scroll.set_offset(point(px(0.), -scroll.max_offset().y));

			s.latest_follow_work = None;

			s.toggle_connection_details(cx);

			assert_eq!(s.latest_follow_work.as_deref(), Some("agent"));
			// Model the frame between a growing footer's layout and bottom-follow.
			scroll.set_offset(point(px(0.), scroll.offset().y + px(32.)));

			assert_eq!(s.active_history_index(&scroll), s.history_marks.len() - 1);
		});
		// Let time-based panel transitions settle before checking the final scroll extent.
		thread::sleep(std::time::Duration::from_millis(240));

		for _ in 0..40 {
			visual.update(|w, cx| w.draw(cx).clear());
		}

		surface.update(visual, |s, cx| {
			let scroll = s.transcript_scroll["agent"].clone();

			assert!((scroll.offset().y + scroll.max_offset().y).abs() < px(1.));

			s.latest_follow_work = None;

			scroll.set_offset(point(px(0.), px(-100.)));

			let before = scroll.offset();
			let active = s.active_history_index(&scroll);

			s.toggle_connection_details(cx);

			assert!(s.latest_follow_work.is_none());
			assert_eq!(scroll.offset(), before);
			assert_eq!(s.active_history_index(&scroll), active);
		});
	}

	#[test]
	fn navigation_cannot_scroll_past_history_and_dock_magnifies_neighbours() {
		assert_eq!(navigation_offset(5_000.0, 1_000.0), -1_000.0);
		assert_eq!(navigation_offset(0.0, 1_000.0), 0.0);
		assert_eq!(navigation_offset(500.0, 1_000.0), -444.0);
		assert!(dock_influence(3, Some(3)) > dock_influence(2, Some(3)));
		assert!(dock_influence(2, Some(3)) > dock_influence(1, Some(3)));
		assert_eq!(dock_influence(0, Some(3)), 0.0);
	}

	#[test]
	fn navigation_uses_message_positions_not_equal_height_assumptions() {
		assert_eq!(current_mark(&[0.0, 100.0, 1_800.0], 200.0), 1);
		assert_eq!(current_mark(&[0.0, 100.0, 1_800.0], 1_800.0), 2);
		assert_eq!(preview(&"字".repeat(181)).chars().count(), 181);
	}
}
