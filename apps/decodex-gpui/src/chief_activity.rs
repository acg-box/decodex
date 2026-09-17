//! Compact, anchored navigation through saved conversation turns.
use super::*;
use gpui::{AnyElement, canvas, point, size};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};

#[derive(Clone)]
pub(super) struct HistoryMark {
	position: Rc<Cell<f32>>,
	question: String,
	time: String,
	answer: String,
}

pub(super) struct HistoryNavigation {
	work: String,
	from: f32,
	to: f32,
	started: std::time::Instant,
}

fn navigation_offset(position: f32, maximum: f32) -> f32 {
	(-(position - 56.0)).clamp(-maximum.max(0.0), 0.0)
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
	positions.iter().rposition(|position| *position <= offset + 32.0).unwrap_or(0)
}

impl ChiefSurface {
	pub(super) fn prepare_history_marks(&mut self) {
		if self.history_marks_work != self.selected {
			self.history_marks.clear();
			self.history_hover = None;
			self.history_navigation = None;
			self.history_marks_work = self.selected.clone();
		}
		let Some((work, ChiefHistoryResult::Available { entries, .. })) =
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
		self.history_marks.retain(|id, _| saved.contains_key(id));
		let mut current = None;
		for entry in saved.values() {
			if entry.kind == "user" || entry.kind == "instruction" {
				let mark = self.history_marks.entry(entry.id).or_insert_with(|| HistoryMark {
					position: Rc::new(Cell::new(0.0)),
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
				});
				mark.answer.clear();
				current = Some(entry.id);
			} else if entry.kind == "assistant"
				&& let Some(id) = current
			{
				self.history_marks.get_mut(&id).expect("current mark").answer =
					preview(&entry.text);
			}
		}
	}

	pub(super) fn anchored_history_entry(
		&self,
		entry: &decodex_protocol::ChiefHistoryEntryDto,
	) -> AnyElement {
		let Some(mark) = self.history_marks.get(&entry.id) else {
			return history_entry(entry).into_any_element();
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
			.on_children_prepainted(move |bounds, _, _| {
				if let Some(bounds) = bounds.first() {
					position.set(f32::from(
						bounds.origin.y - scroll.bounds().origin.y - scroll.offset().y,
					));
				}
			})
			.id(SharedString::from(format!("history-anchor-{}", entry.id)))
			.child(history_entry(entry))
			.into_any_element()
	}

	fn jump_to_history(&mut self, id: i64, cx: &mut Context<Self>) {
		if let Some(mark) = self.history_marks.get(&id)
			&& let Some(work) = self.selected.as_ref()
			&& let Some(scroll) = self.transcript_scroll.get(work)
		{
			self.history_navigation = Some(HistoryNavigation {
				work: work.clone(),
				from: scroll.offset().y.into(),
				to: navigation_offset(mark.position.get(), scroll.max_offset().y.into()),
				started: std::time::Instant::now(),
			});
			cx.notify();
		}
	}

	pub(super) fn scroll_history(
		&mut self,
		event: &gpui::ScrollWheelEvent,
		cx: &mut Context<Self>,
	) {
		self.history_navigation = None;
		if let Some(scroll) = self.selected.as_ref().and_then(|id| self.transcript_scroll.get(id)) {
			let delta = event.delta.pixel_delta(px(ui_theme::BODY_LINE_HEIGHT));
			let offset = (scroll.offset().y + delta.y).clamp(-scroll.max_offset().y, px(0.0));
			scroll.set_offset(point(px(0.0), offset));
			let following = (offset + scroll.max_offset().y).abs() < px(24.);
			self.set_voice_follow(following);
			cx.stop_propagation();
			cx.notify();
		}
	}

	pub(super) fn animate_history_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(navigation) = &self.history_navigation else {
			return;
		};
		if self.selected.as_ref() != Some(&navigation.work) {
			self.history_navigation = None;
			return;
		}
		let t = (navigation.started.elapsed().as_secs_f32() / 0.28).min(1.0);
		if let Some(scroll) = self.transcript_scroll.get(&navigation.work) {
			let offset =
				navigation.from + (navigation.to - navigation.from) * (1.0 - (1.0 - t).powi(3));
			scroll.set_offset(point(
				px(0.0),
				px(offset.clamp(-f32::from(scroll.max_offset().y).max(0.0), 0.0)),
			));
		}
		if t < 1.0 {
			window.request_animation_frame();
			cx.notify();
		} else {
			self.history_navigation = None;
		}
	}

	pub(super) fn history_rail_slot(
		&self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		crate::ui_motion::reveal(
			"history-rail-reveal",
			if self.timeline_visible && !self.history_marks.is_empty() { 44.0 } else { 0.0 },
			true,
			self.history_rail(window, cx),
		)
		.into_any_element()
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
					&& work.dispatch_state == ChiefDispatchStateDto::Running
			})
		});
		let last = positions.len() - 1;
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
			let influence = crate::ui_motion::value(
				SharedString::from(format!(
					"rail-magnify-{}-{id}",
					self.selected.as_deref().unwrap_or_default()
				)),
				dock_influence(index, self.history_hover),
				window,
				cx,
			);
			let tip = mark.clone();
			let id = *id;
			let positions = positions.clone();
			let scroll = scroll.clone();
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
						s.jump_to_history(id, cx);
					}))
					.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
						if event.keystroke.key == "enter" || event.keystroke.key == "space" {
							s.jump_to_history(id, cx);
							cx.stop_propagation();
							cx.notify();
						}
					}))
					.child(
						canvas(
							|_, _, _| (),
							move |bounds, _, window, _| {
								let current = current_mark(
									&positions.iter().map(|p| p.get()).collect::<Vec<_>>(),
									-f32::from(scroll.offset().y),
								);
								let at_end = scroll.max_offset().y > px(0.0)
									&& (scroll.offset().y + scroll.max_offset().y).abs() < px(1.0);
								let active = if at_end { index == last } else { current == index };
								let color = if working && index == last {
									ui_theme::BLUE
								} else if active {
									ui_theme::TEXT
								} else {
									ui_theme::TEXT_MUTED
								};
								let width = if active { 18.0 } else { 7.0 } + influence * 16.0;
								window.paint_quad(gpui::fill(
									gpui::Bounds::new(
										point(
											bounds.center().x - px(width / 2.0),
											bounds.center().y,
										),
										size(px(width), px(2.0 + influence)),
									),
									rgb(color),
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
			.child(
				div()
					.text_size(px(10.0))
					.text_color(rgb(ui_theme::TEXT_MUTED))
					.child(self.0.time.clone()),
			)
			.child(div().text_color(rgb(ui_theme::TEXT)).child(self.0.question.clone()))
			.when(!self.0.answer.is_empty(), |panel| {
				panel
					.child(div().text_color(rgb(ui_theme::TEXT_MUTED)).child(self.0.answer.clone()))
			})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn history_marks_jump_to_real_message_anchors(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(size(px(1400.0), px(320.0)));
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
				assert!(s.history_marks[&3].position.get() > s.history_marks[&1].position.get());
				s.jump_to_history(3, cx);
				s.history_navigation.as_mut().unwrap().started -= std::time::Duration::from_secs(1);
				cx.notify();
			})
		});
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		surface.update(visual, |s, _| {
			assert!(s.transcript_scroll["chief"].offset().y < px(0.0));
			assert!(
				s.transcript_scroll["chief"].offset().y
					>= -s.transcript_scroll["chief"].max_offset().y
			);
			assert_eq!(s.history_marks.len(), 2);
		});
	}

	#[gpui::test]
	fn wheel_scroll_keeps_adjacent_messages_accessible(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(size(px(1400.0), px(400.0)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.graph_visible = false;
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let scroll = surface.read_with(visual, |s, _| s.transcript_scroll["chief"].clone());
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
	}

	#[test]
	fn navigation_cannot_scroll_past_history_and_dock_magnifies_neighbours() {
		assert_eq!(navigation_offset(5000.0, 1000.0), -1000.0);
		assert_eq!(navigation_offset(0.0, 1000.0), 0.0);
		assert_eq!(navigation_offset(500.0, 1000.0), -444.0);
		assert!(dock_influence(3, Some(3)) > dock_influence(2, Some(3)));
		assert!(dock_influence(2, Some(3)) > dock_influence(1, Some(3)));
		assert_eq!(dock_influence(0, Some(3)), 0.0);
	}

	#[test]
	fn navigation_uses_message_positions_not_equal_height_assumptions() {
		assert_eq!(current_mark(&[0.0, 100.0, 1800.0], 200.0), 1);
		assert_eq!(current_mark(&[0.0, 100.0, 1800.0], 1800.0), 2);
		assert_eq!(preview(&"字".repeat(181)).chars().count(), 181);
	}
}
