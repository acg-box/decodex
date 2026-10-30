//! Find in retained conversation pages without a second history store.
use gpui::{AnyElement, AppContext as _, Entity, Focusable as _, KeyDownEvent, SharedString};

use crate::{
	composer_input::{ComposerEvent, ComposerInput, SubmitComposer},
	shell::agent_surface::native_timeline::{
		self, AgentSurface, AgentTimelineContent, AgentTimelineEntry, Context, FluentBuilder,
		InteractiveElement, IntoElement, ParentElement, Styled,
	},
	ui_theme,
};

type Key = (u64, u8, String);

#[derive(Default)]
pub(super) struct Panel {
	pub open: bool,
	input: Option<Entity<ComposerInput>>,
	query: String,
	pub current: Option<Key>,
	pub searching_earlier: bool,
	retained: Option<Retained>,
}

struct Retained {
	key: Key,
	turn: String,
	text: String,
	truncated: bool,
}

impl AgentSurface {
	pub(crate) fn open_conversation_find(
		&mut self,
		window: &mut gpui::Window,
		cx: &mut Context<Self>,
	) {
		self.reset_item_context();
		if !self.conversation_work().is_some_and(|work| self.native_history_active(&work))
			|| self.timeline.native.summary_only()
		{
			self.feedback = crate::ui_feedback::Feedback::neutral(
				"Wait for conversation history before searching.",
			);
			cx.notify();
			return;
		}
		if self.timeline.native.find.input.is_none() {
			let input = cx.new(|cx| {
				ComposerInput::with_placeholder(
					0,
					"Find in conversation",
					"Find in conversation",
					cx,
				)
			});
			cx.subscribe(&input, |s, source, event, cx| {
				if s.timeline
					.native
					.find
					.input
					.as_ref()
					.is_none_or(|input| input.entity_id() != source.entity_id())
				{
					return;
				}
				if matches!(event, ComposerEvent::Changed) {
					s.timeline.native.find.query = s
						.timeline
						.native
						.find
						.input
						.as_ref()
						.map(|input| input.read(cx).content().to_owned())
						.unwrap_or_default();
					s.timeline.native.find.current = None;
					s.timeline.native.find.retained = None;
					s.select_conversation_match(false, cx);
				}
			})
			.detach();
			self.timeline.native.find.input = Some(input);
		}
		self.timeline.native.find.open = true;
		if let Some(input) = &self.timeline.native.find.input {
			window.activate_window();
			window.focus(&input.focus_handle(cx), cx);
		}
		cx.notify();
	}

	pub(crate) fn conversation_find_open(&self) -> bool {
		self.timeline.native.find.open
			&& !self.timeline.native.summary_only()
			&& self.conversation_work().is_some_and(|work| self.native_history_active(&work))
	}

	pub(in super::super) fn conversation_find_bar(
		&self,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		if !self.conversation_find_open() {
			return None;
		}
		let find = &self.timeline.native.find;
		let input = find.input.as_ref().filter(|_| find.open)?;
		let matches = self.conversation_find_matches();
		let index =
			find.current.as_ref().and_then(|key| matches.iter().position(|value| value == key));
		let count = if find.query.trim().is_empty() {
			"Search loaded history".into()
		} else {
			format!("{} / {} records", index.map_or(0, |index| index + 1), matches.len())
		};
		Some(
			gpui::div()
				.debug_selector(|| "conversation-find".into())
				.flex_none()
				.px(gpui::px(16.))
				.pb(gpui::px(8.))
				.flex()
				.flex_col()
				.gap_1()
				.capture_key_down(cx.listener(|s, event: &KeyDownEvent, window, cx| {
					if event.keystroke.key == "escape" {
						if !event.is_held {
							s.timeline.native.find.open = false;
							s.timeline.native.find.current = None;
							s.timeline.native.find.retained = None;
							s.timeline.native.find.searching_earlier = false;
							s.focus_message(window, cx);
							cx.notify();
						}
						cx.stop_propagation();
					} else if event.keystroke.key == "enter" {
						if !event.is_held {
							s.select_conversation_match(event.keystroke.modifiers.shift, cx);
						}
						cx.stop_propagation();
					}
				}))
				// The editor's Enter action must never reach the message composer.
				.on_action(cx.listener(|s, _: &SubmitComposer, _, cx| {
					s.select_conversation_match(false, cx);
					cx.stop_propagation();
				}))
				.child(
					gpui::div()
						.flex()
						.items_center()
						.gap_2()
						.child(gpui::div().flex_1().min_w_0().child(input.clone()))
						.child(crate::shell::agent_surface::muted(count))
						.child(self.workspace_action(
							"find-previous".into(),
							"Previous".into(),
							|s, cx| s.select_conversation_match(true, cx),
							cx,
						))
						.child(self.workspace_action(
							"find-next".into(),
							"Next".into(),
							|s, cx| s.select_conversation_match(false, cx),
							cx,
						))
						.child(self.workspace_action(
							"find-close".into(),
							"Close".into(),
							|s, cx| {
								s.timeline.native.find.open = false;
								s.timeline.native.find.current = None;
								s.timeline.native.find.retained = None;
								s.timeline.native.find.searching_earlier = false;
								cx.notify();
							},
							cx,
						)),
				)
				.child(
					gpui::div()
						.flex()
						.items_center()
						.gap_2()
						.child(crate::shell::agent_surface::muted(
							if self.timeline.native.browsing_window {
								"Earlier history window. Shortened output is not fully searched."
							} else {
								"Loaded history, including folded activity. Shortened output is not fully searched."
							},
						))
						.when(self.timeline.native.older_cursor.is_some(), |row| {
							row.child(if self.timeline.native.task.is_some() {
								crate::ui_loading::loading("Loading earlier history")
									.into_any_element()
							} else {
								self.workspace_action(
									"find-earlier".into(),
									"Load earlier and search".into(),
									|s, cx| {
										if let Some(binding) = s.timeline.native.binding.clone() {
											s.timeline.native.find.searching_earlier = true;
											s.load_native_timeline(
												&binding.work,
												&binding.thread,
												true,
												cx,
											);
										}
									},
									cx,
								)
							})
						}),
				)
				.into_any_element(),
		)
	}

	fn conversation_find_matches(&self) -> Vec<Key> {
		let query = self.timeline.native.find.query.trim().to_lowercase();
		if query.is_empty() {
			return Vec::new();
		}
		let work = self.conversation_work();
		let hidden: std::collections::BTreeSet<_> =
			crate::shell::agent_surface::voice::history::groups(&self.timeline.native.entries)
				.into_iter()
				.filter(|group| group.hidden())
				.flat_map(|group| group.indices)
				.collect();
		self.timeline
			.native
			.entries
			.iter()
			.enumerate()
			.filter(|(index, entry)| {
				if hidden.contains(index) {
					return false;
				}
				let text = self.find_entry_text(entry, work.as_ref());
				text.to_lowercase().contains(&query)
			})
			.map(|(_, entry)| entry_key(entry))
			.collect()
	}

	fn find_entry_text<'a>(
		&'a self,
		entry: &'a AgentTimelineEntry,
		work: Option<&crate::shell::agent_surface::AgentWorkItemDto>,
	) -> &'a str {
		match &entry.content {
			AgentTimelineContent::Item { kind, turn_id, item_id, text, .. } => self
				.find_retained_text(turn_id, item_id)
				.map(|(text, _)| text)
				.unwrap_or_else(|| {
					work.and_then(|work| self.native_item_draft(work, kind, turn_id, item_id))
						.map_or(text, |message| &message.text)
				}),
			AgentTimelineContent::Speech { text, .. } => text,
			AgentTimelineContent::TurnBoundary { error: Some(error), .. } => &error.message,
			_ => "",
		}
	}

	pub(super) fn select_conversation_match(&mut self, previous: bool, cx: &mut Context<Self>) {
		let matches = self.conversation_find_matches();
		let index = self
			.timeline
			.native
			.find
			.current
			.as_ref()
			.and_then(|current| matches.iter().position(|key| key == current));
		let next = match (index, previous, matches.len()) {
			(_, _, 0) => None,
			(Some(index), true, count) => Some((index + count - 1) % count),
			(Some(index), false, count) => Some((index + 1) % count),
			(None, true, count) => Some(count - 1),
			(None, false, _) => Some(0),
		};
		self.timeline.native.find.current = next.map(|index| matches[index].clone());
		self.timeline.native.find.retained = None;
		if let Some(entry) =
			self.timeline.native.entries.iter().find(|entry| self.is_conversation_match(entry))
			&& let AgentTimelineContent::Item { turn_id, item_id, kind, text, truncated, .. } =
				&entry.content
		{
			let work = self.conversation_work();
			let live =
				work.as_ref().and_then(|work| self.native_item_draft(work, kind, turn_id, item_id));
			self.timeline.native.find.retained = Some(Retained {
				key: entry_key(entry),
				turn: turn_id.clone(),
				text: live.map_or(text, |message| &message.text).clone(),
				truncated: live.map_or(*truncated, |message| message.truncated),
			});
		}
		if let Some(key) = &self.timeline.native.find.current {
			self.timeline.latest_follow_work = None;
			self.timeline.navigation = None;
			if let Some(work) = &self.selected {
				self.timeline.follow_paused.insert(work.clone());
			}
			self.timeline.native.viewport.find(key.clone());
		}
		cx.notify();
	}

	pub(super) fn is_conversation_match(&self, entry: &AgentTimelineEntry) -> bool {
		self.timeline.native.find.open
			&& self.timeline.native.find.current.as_ref() == Some(&entry_key(entry))
	}

	pub(super) fn find_retained_text(&self, turn: &str, item: &str) -> Option<(&str, bool)> {
		self.timeline
			.native
			.find
			.retained
			.as_ref()
			.filter(|retained| {
				self.conversation_find_open()
					&& self.timeline.native.find.current.as_ref() == Some(&retained.key)
					&& retained.turn == turn
					&& retained.key.2 == item
			})
			.map(|retained| (retained.text.as_str(), retained.truncated))
	}

	pub(super) fn find_result_row(
		&self,
		entry: &AgentTimelineEntry,
		body: AnyElement,
	) -> AnyElement {
		if !self.is_conversation_match(entry) {
			return body;
		}
		let work = self.conversation_work();
		let text = self.find_entry_text(entry, work.as_ref());
		let query = self.timeline.native.find.query.trim().to_lowercase();
		let line = match_excerpt(text, &query);
		gpui::div()
			.debug_selector(|| "conversation-find-current".into())
			.w_full()
			.min_w_0()
			.border_l_2()
			.border_color(ui_theme::rgb(ui_theme::BLUE))
			.pl_2()
			.child(
				gpui::div()
					.text_color(ui_theme::rgb(ui_theme::BLUE))
					.text_size(gpui::px(12.))
					.child(SharedString::from(line)),
			)
			.child(body)
			.into_any_element()
	}
}

fn entry_key(entry: &AgentTimelineEntry) -> Key {
	let (position, kind, id) = native_timeline::key(entry);
	(position, kind, id.into())
}

/// Keep the matching term visible even in one long output line. Map folded text
/// back to original UTF-8 boundaries because one character can lowercase to two.
fn match_excerpt(text: &str, query: &str) -> String {
	let line = text.lines().find(|line| line.to_lowercase().contains(query)).unwrap_or(text);
	let offset = line.to_lowercase().find(query).unwrap_or(0);
	let mut folded_bytes = 0;
	let characters: Vec<_> = line.chars().collect();
	let start = characters
		.iter()
		.position(|ch| {
			let here = folded_bytes;
			folded_bytes += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
			here >= offset
		})
		.unwrap_or(0)
		.saturating_sub(80);
	let count = query.chars().count().max(180) + 80;
	format!(
		"{}{}{}",
		if start > 0 { "…" } else { "" },
		characters.iter().skip(start).take(count).collect::<String>(),
		if characters.len() > start + count { "…" } else { "" }
	)
}

#[cfg(test)]
mod tests {
	use super::{AgentSurface, AgentTimelineContent, AgentTimelineEntry, entry_key, match_excerpt};
	use crate::shell::agent_surface::native_timeline::{Binding, Timeline};
	use decodex_protocol::AgentTimelinePage;

	fn item(position: u64, kind: &str, text: &str) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position,
			content: AgentTimelineContent::Item {
				turn_id: "turn".into(),
				item_id: format!("item-{position}"),
				kind: kind.into(),
				text: text.into(),
				truncated: false,
				phase: None,
				app_ui: false,
				activity: None,
				attachments: vec![],
				collaboration: None,
			},
		}
	}

	fn page(entries: Vec<AgentTimelineEntry>, next: Option<&str>) -> AgentTimelinePage {
		AgentTimelinePage {
			thread_token_total: None,
			thread_token_breakdown: None,
			thread_metrics: None,
			safety_buffering_turn_id: None,
			thread_id: "thread".into(),
			entries,
			next_cursor: next.map(str::to_owned),
			weather: Default::default(),
			active_realtime_session_at_page_start: None,
		}
	}

	fn prepare(s: &mut AgentSurface, cx: &mut gpui::Context<AgentSurface>) {
		s.visual_workspace_fixture(cx);
		s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == "agent")
			.unwrap()
			.codex_thread_id = Some("thread".into());
		let mut state = Timeline::default();
		let mut entries = vec![
			item(10, "userMessage", "Question"),
			item(11, "commandExecution", "构建 needle in hidden activity"),
			item(12, "agentMessage", "Second needle"),
		];
		entries.push(AgentTimelineEntry {
			position: 13,
			content: AgentTimelineContent::TurnBoundary {
				turn_id: "turn".into(),
				completed: true,
				status: Some("completed".into()),
				error: None,
				usage: None,
				usage_summary: None,
				duration_ms: None,
				timing: None,
			},
		});
		assert!(state.replace(
			Binding { work: "agent".into(), thread: "thread".into(), account: "account".into() },
			page(entries, Some("older"))
		));
		state.expanded_turns.insert("manual-other-turn".into());
		state.requested = Some(("agent".into(), "thread".into()));
		s.timeline.native = state;
	}

	#[test]
	fn find_excerpt_keeps_a_late_unicode_match_on_original_boundaries() {
		let text = format!("İ{}目标 ABC{}", "长".repeat(500), "尾".repeat(500));
		let excerpt = match_excerpt(&text, "目标 abc");
		assert!(excerpt.contains("目标 ABC"));
		assert!(excerpt.starts_with('…') && excerpt.ends_with('…'));
		assert!(excerpt.chars().count() < 300);
	}

	#[gpui::test]
	fn find_reads_folded_text_keeps_manual_disclosures_and_owns_enter(
		cx: &mut gpui::TestAppContext,
	) {
		cx.update(crate::shell::bind_keys);
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, prepare);
		visual
			.update(|window, cx| surface.update(cx, |s, cx| s.open_conversation_find(window, cx)));
		surface.update(visual, |s, cx| {
			s.timeline
				.native
				.find
				.input
				.as_ref()
				.unwrap()
				.update(cx, |input, cx| input.set_content("needle", cx));
		});
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);
		surface.read_with(visual, |s, _| {
			assert_eq!(s.conversation_find_matches().len(), 2);
			assert_eq!(
				s.timeline.native.find.current,
				Some(entry_key(&s.timeline.native.entries[1]))
			);
			assert_eq!(
				s.timeline.native.expanded_turns.iter().map(String::as_str).collect::<Vec<_>>(),
				["manual-other-turn"]
			);
		});
		assert!(visual.debug_bounds("conversation-find").is_some());
		visual.simulate_keystrokes("enter enter enter");
		visual.simulate_resize(gpui::size(gpui::px(1_300.), gpui::px(840.)));
		visual.update(crate::ui_frame::draw_current_frame);
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
			assert!(!s.sending && s.submission.command.is_none());
			assert!(s.timeline.native.find.current.is_some());
		});
		visual.simulate_keystrokes("escape");
		surface.read_with(visual, |s, _| {
			assert!(!s.conversation_find_open());
			assert!(s.timeline.native.find.current.is_none());
			assert_eq!(s.timeline.native.expanded_turns.len(), 1);
		});
	}

	#[gpui::test]
	fn find_retains_the_matched_live_revision_through_refresh_and_resize(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			prepare(s, cx);
			s.timeline.native.entries.pop();
			if let Some((_, decodex_protocol::AgentHistoryResult::Available { live, .. })) =
				&mut s.history
			{
				live.push(decodex_protocol::AgentLiveMessageDto {
					kind: Default::default(),
					turn_id: "turn".into(),
					item_id: "item-12".into(),
					text: "Original live needle".into(),
					truncated: false,
				});
			}
		});
		visual
			.update(|window, cx| surface.update(cx, |s, cx| s.open_conversation_find(window, cx)));
		surface.update(visual, |s, cx| {
			s.timeline.native.find.query = "needle".into();
			s.select_conversation_match(true, cx);
			assert_eq!(
				s.find_retained_text("turn", "item-12"),
				Some(("Original live needle", false))
			);
			if let Some((_, decodex_protocol::AgentHistoryResult::Available { live, .. })) =
				&mut s.history
			{
				live[0].text = "New live revision after refresh".into();
			}
		});
		visual.simulate_resize(gpui::size(gpui::px(1_300.), gpui::px(840.)));
		visual.update(crate::ui_frame::draw_current_frame);
		surface.read_with(visual, |s, _| {
			assert_eq!(
				s.find_retained_text("turn", "item-12"),
				Some(("Original live needle", false))
			);
			assert_eq!(s.conversation_find_matches().len(), 2);
		});
		surface.update(visual, |s, _| {
			s.timeline.native.find.open = false;
			assert!(s.find_retained_text("turn", "item-12").is_none());
		});
	}

	#[gpui::test]
	fn find_uses_the_existing_older_page_and_rejects_a_changed_source(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, prepare);
		visual
			.update(|window, cx| surface.update(cx, |s, cx| s.open_conversation_find(window, cx)));
		surface.update(visual, |s, cx| {
			s.timeline.native.find.query = "earlier needle".into();
			assert!(s.conversation_find_matches().is_empty());
			s.timeline.native.find.searching_earlier = true;
			s.finish_native_timeline_read(
				Some(decodex_protocol::AgentTimelineResult::Available {
					work_id: decodex_protocol::EntityId::new("agent").unwrap(),
					account_id: decodex_protocol::EntityId::new("account").unwrap(),
					page: page(vec![item(1, "agentMessage", "Earlier needle")], None),
				}),
				("agent".into(), "thread".into()),
				Some("older".into()),
				false,
				cx,
			);
			assert_eq!(s.conversation_find_matches().len(), 1);
			assert_eq!(
				s.timeline.native.find.current,
				Some(entry_key(&s.timeline.native.entries[0]))
			);
			assert!(s.timeline.native.older_cursor.is_none());
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| work.id == "agent")
				.unwrap()
				.codex_thread_id = Some("different-thread".into());
			assert!(!s.conversation_find_open());
			assert!(s.conversation_find_bar(cx).is_none());
		});
	}
}
