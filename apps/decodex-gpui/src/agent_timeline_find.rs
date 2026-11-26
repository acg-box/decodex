//! Search native messages and retained local activity in the current conversation.
use gpui::{
	AnyElement, AnyWindowHandle, AppContext as _, Entity, FocusHandle, Focusable as _, SharedString,
};

use crate::{
	composer_input::{ComposerEvent, ComposerInput},
	shell::agent_surface::native_timeline::{
		self, AgentSurface, AgentTimelineContent, AgentTimelineEntry, Context, FluentBuilder,
		InteractiveElement, IntoElement, ParentElement, Styled,
	},
	ui_theme,
};

use decodex_protocol::{AgentClient, ConversationMatch, EntityId, ItemContextRequest, WireText};

type Key = (u64, u8, String);

#[derive(Clone, Eq, PartialEq)]
enum Target {
	Loaded(Key),
	Message(usize),
}

#[derive(Default)]
struct Search {
	hits: Vec<ConversationMatch>,
	cursor: Option<WireText>,
	selected: Option<usize>,
	task: Option<gpui::Task<()>>,
	failed: bool,
}

#[derive(Default)]
pub(super) struct Panel {
	pub open: bool,
	input: Option<Entity<ComposerInput>>,
	query: String,
	search: Search,
	pub current: Option<Key>,
	pub searching_earlier: bool,
	retained: Option<Retained>,
	return_focus: Option<ReturnFocus>,
}

struct ReturnFocus {
	window: AnyWindowHandle,
	handle: Option<FocusHandle>,
	panel: Option<crate::shell::agent_surface::workspace_size::Panel>,
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
					s.reset_item_context();
					s.timeline.native.find.current = None;
					s.timeline.native.find.retained = None;
					s.timeline.native.find.search = Search::default();
					s.select_conversation_match(false, cx);
					s.read_conversation_matches(cx);
				}
			})
			.detach();
			self.timeline.native.find.input = Some(input);
		}
		if !self.timeline.native.find.open {
			#[cfg(all(target_os = "macos", not(test)))]
			let handle = self.active_native_composer_focus(cx).or_else(|| window.focused(cx));
			#[cfg(not(all(target_os = "macos", not(test))))]
			let handle = window.focused(cx);
			self.timeline.native.find.return_focus = Some(ReturnFocus {
				window: window.window_handle(),
				handle,
				panel: self.workspace.focused_panel,
			});
		}
		self.timeline.native.find.open = true;
		if self.timeline.native.find.search.hits.is_empty()
			&& self.timeline.native.find.search.task.is_none()
		{
			self.read_conversation_matches(cx);
		}
		if let Some(input) = &self.timeline.native.find.input {
			window.activate_window();
			window.focus(&input.focus_handle(cx), cx);
		}
		cx.notify();
	}

	pub(in super::super) fn dismiss_conversation_search(&mut self) {
		self.timeline.native.find = Panel::default();
	}

	fn close_conversation_find(&mut self, cx: &mut Context<Self>) {
		let find = &mut self.timeline.native.find;
		find.open = false;
		find.search = Search::default();
		find.current = None;
		find.retained = None;
		find.searching_earlier = false;
		if let Some(origin) = find.return_focus.take() {
			self.workspace.focused_panel = origin.panel;
			cx.defer(move |cx| {
				let _ = origin.window.update(cx, |_, window, cx| {
					if let Some(handle) = origin.handle {
						window.focus(&handle, cx);
					} else {
						window.blur(cx);
					}
				});
			});
		}
		cx.notify();
	}

	pub(crate) fn conversation_find_open(&self) -> bool {
		self.timeline.native.find.open
			&& !self.timeline.native.summary_only()
			&& self.conversation_work().is_some_and(|work| self.native_history_active(&work))
	}

	fn find_match_count(&self) -> String {
		let find = &self.timeline.native.find;
		let matches = self.conversation_targets();
		let selected = self.selected_find_target();
		let index = matches.iter().position(|value| Some(value) == selected.as_ref());
		if find.query.trim().is_empty() {
			String::new()
		} else if matches.is_empty() && find.search.task.is_some() {
			"Searching…".into()
		} else if matches.is_empty() && find.search.failed {
			"No loaded matches".into()
		} else {
			format!(
				"{} / {}{}",
				index.map_or(0, |i| i + 1),
				matches.len(),
				if find.search.cursor.is_some() { "+" } else { "" }
			)
		}
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
		let count = self.find_match_count();
		Some(
			gpui::div()
				.debug_selector(|| "conversation-find".into())
				.flex_none()
				.px(gpui::px(16.))
				.pb(gpui::px(8.))
				.flex()
				.flex_col()
				.gap_1()
				.key_context("ConversationFind")
				.on_action(cx.listener(|s, _: &crate::shell::NextMatch, _, cx| {
					s.select_conversation_match(false, cx)
				}))
				.on_action(cx.listener(|s, _: &crate::shell::PreviousMatch, _, cx| {
					s.select_conversation_match(true, cx)
				}))
				.on_action(cx.listener(|s, _: &crate::shell::CloseFind, _, cx| {
					s.close_conversation_find(cx)
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
							Self::close_conversation_find,
							cx,
						)),
				)
				.child(
					gpui::div()
						.flex()
						.items_center()
						.gap_2()
						.when(find.search.failed, |row| {
							row.child(crate::shell::agent_surface::muted(
								"Message search unavailable.",
							))
							.child(self.workspace_action(
								"find-retry".into(),
								"Retry".into(),
								Self::read_conversation_matches,
								cx,
							))
						})
						.when(
							find.search.cursor.is_some()
								&& find.search.task.is_none()
								&& !find.search.failed,
							|row| {
								row.child(self.workspace_action(
									"find-more".into(),
									"More matches".into(),
									Self::read_conversation_matches,
									cx,
								))
							},
						)
						.when(self.timeline.native.older_cursor.is_some(), |row| {
							row.child(if self.timeline.native.task.is_some() {
								crate::ui_loading::loading("Loading earlier history")
									.into_any_element()
							} else {
								self.workspace_action(
									"find-earlier".into(),
									"Search earlier activity".into(),
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

	fn read_conversation_matches(&mut self, cx: &mut Context<Self>) {
		let Some(binding) = self.timeline.native.binding.clone() else { return };
		let Some(profile) = self.profile.clone() else { return };
		let source = self.snapshot.as_ref().and_then(|s| s.runtime_source.clone());
		let find = &mut self.timeline.native.find;
		let query = find.query.trim().to_owned();
		if query.is_empty() || find.search.task.is_some() {
			return;
		}
		let cursor = find.search.cursor.clone();
		find.search.failed = false;
		find.search.task = Some(cx.spawn(async move |surface, cx| {
			let executor = cx.background_executor().clone();
			if cursor.is_none() {
				executor.timer(std::time::Duration::from_millis(180)).await;
			}
			let expected = binding.clone();
			let term = query.clone();
			let page = executor
				.spawn(async move {
					let runtime =
						tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
					runtime
						.block_on(AgentClient::new(profile).search_conversation(
							EntityId::new(binding.work).ok()?,
							EntityId::new(binding.thread).ok()?,
							WireText::new(term).ok()?,
							cursor,
						))
						.ok()
						.flatten()
				})
				.await;
			let _ = surface.update(cx, |s, cx| {
				s.timeline.native.find.search.task = None;
				if !s.conversation_find_open()
					|| s.timeline.native.binding.as_ref() != Some(&expected)
					|| s.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref())
						!= source.as_ref()
					|| s.timeline.native.find.query.trim() != query
				{
					return;
				}
				let search = &mut s.timeline.native.find.search;
				if let Some(page) = page {
					search.hits.extend(page.data);
					search.cursor = page.next_cursor;
					if s.selected_find_target().is_none() {
						s.select_conversation_match(false, cx);
					}
				} else {
					search.failed = true;
				}
				cx.notify();
			});
		}));
	}

	fn selected_find_target(&self) -> Option<Target> {
		let find = &self.timeline.native.find;
		find.search
			.selected
			.map(Target::Message)
			.or_else(|| find.current.clone().map(Target::Loaded))
	}

	fn conversation_targets(&self) -> Vec<Target> {
		let loaded = self.matching_entries();
		let mut seen: std::collections::HashSet<_> = loaded
			.iter()
			.filter_map(|entry| match &entry.content {
				AgentTimelineContent::Item { turn_id, item_id, .. } =>
					Some((self.entry_thread(entry)?, turn_id.as_str(), item_id.as_str())),
				_ => None,
			})
			.collect();
		let mut targets: Vec<_> =
			loaded.into_iter().map(|entry| Target::Loaded(entry_key(entry))).collect();
		targets.extend(
			self.timeline
				.native
				.find
				.search
				.hits
				.iter()
				.enumerate()
				.filter(|(_, h)| {
					seen.insert((h.thread_id.as_str(), h.turn_id.as_str(), h.item_id.as_str()))
				})
				.map(|(i, _)| Target::Message(i)),
		);
		targets
	}

	fn matching_entries(&self) -> Vec<&AgentTimelineEntry> {
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
			.map(|(_, entry)| entry)
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
		let matches = self.conversation_targets();
		let selected = self.selected_find_target();
		let index = matches.iter().position(|target| Some(target) == selected.as_ref());
		let next = match (index, previous, matches.len()) {
			(_, _, 0) => None,
			(Some(index), true, count) => Some((index + count - 1) % count),
			(Some(index), false, count) => Some((index + 1) % count),
			(None, true, count) => Some(count - 1),
			(None, false, _) => Some(0),
		};
		self.timeline.native.find.retained = None;
		self.timeline.native.find.current = None;
		self.timeline.native.find.search.selected = None;
		match next.map(|index| &matches[index]) {
			Some(Target::Message(index)) => {
				let Some(binding) = self.timeline.native.binding.as_ref() else { return };
				let Ok(work_id) = EntityId::new(&binding.work) else { return };
				self.timeline.native.find.search.selected = Some(*index);
				let hit = &self.timeline.native.find.search.hits[*index];
				self.locate_find_match(
					ItemContextRequest {
						work_id,
						thread_id: hit.thread_id.clone(),
						turn_id: hit.turn_id.clone(),
						item_id: hit.item_id.clone(),
						cursor: None,
						before: false,
					},
					cx,
				);
				return;
			},
			Some(Target::Loaded(key)) => {
				self.reset_item_context();
				self.timeline.native.find.current = Some(key.clone());
			},
			None => {},
		}
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

	pub(in super::super) fn located_match_excerpt(&self) -> Option<String> {
		let find = &self.timeline.native.find;
		find.search.hits.get(find.search.selected?).map(|hit| hit.snippet.clone())
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
	use gpui::{
		AppContext as _, Focusable as _, InteractiveElement as _, ParentElement as _, Styled as _,
	};

	struct FocusFixture {
		surface: gpui::Entity<AgentSurface>,
		pane: gpui::FocusHandle,
	}
	impl gpui::Render for FocusFixture {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			_: &mut gpui::Context<Self>,
		) -> impl gpui::IntoElement {
			gpui::div().size_full().track_focus(&self.pane).child(self.surface.clone())
		}
	}

	#[derive(Clone, Copy)]
	enum FocusSource {
		Unfocused,
		Pane,
		Composer,
	}

	#[gpui::test]
	fn find_escape_and_close_restore_the_original_focus_even_after_reopening(
		cx: &mut gpui::TestAppContext,
	) {
		cx.update(crate::shell::bind_keys);
		for source in [FocusSource::Unfocused, FocusSource::Pane, FocusSource::Composer] {
			for close_button in [false, true] {
				let (fixture, visual) = cx.add_window_view(|_, cx| {
					let surface = cx.new(AgentSurface::new);
					surface.update(cx, prepare);
					FocusFixture { surface, pane: cx.focus_handle() }
				});
				let (surface, pane) =
					fixture.read_with(visual, |f, _| (f.surface.clone(), f.pane.clone()));
				let original = match source {
					FocusSource::Unfocused => None,
					FocusSource::Pane => Some(pane),
					FocusSource::Composer =>
						Some(surface.read_with(visual, |s, cx| s.composer.focus_handle(cx))),
				};
				visual.update(|window, cx| {
					if let Some(focus) = &original {
						window.focus(focus, cx);
					} else {
						window.blur(cx);
					}
					surface.update(cx, |s, cx| {
						s.workspace.focused_panel =
							Some(crate::shell::agent_surface::workspace_size::Panel::Right);
						s.open_conversation_find(window, cx);
						s.open_conversation_find(window, cx);
					});
				});
				visual.run_until_parked();
				visual.update(crate::ui_frame::draw_current_frame);
				if close_button {
					let close = visual.debug_bounds("find-close").expect("find close button");
					visual.simulate_click(close.center(), Default::default());
				} else {
					visual.simulate_keystrokes("escape");
				}
				visual.run_until_parked();
				assert_eq!(visual.update(|window, cx| window.focused(cx)), original);
				surface.read_with(visual, |s, cx| {
					assert!(!s.conversation_find_open());
					assert_eq!(s.composer.read(cx).content(), "Keep this draft");
					assert_eq!(
						s.workspace.focused_panel,
						Some(crate::shell::agent_surface::workspace_size::Panel::Right)
					);
				});
			}
		}
	}

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
			source_threads: Default::default(),
			thread_id: "thread".into(),
			entries,
			next_cursor: next.map(str::to_owned),
			weather: Default::default(),
			observed_activities: Vec::new(),
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
				started_at_micros: None,
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

	#[gpui::test]
	fn native_matches_keep_the_source_context_and_merge_with_loaded_activity(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, prepare);
		surface.update(visual, |s, cx| {
			let snapshot = s.snapshot.as_mut().unwrap();
			snapshot.assistant.agent_id = Some("agent".into());
			snapshot.assistant.previous_thread_ids = vec!["previous".into()];
			snapshot.runtime_source = Some(super::EntityId::new("runtime").unwrap());
			let find = &mut s.timeline.native.find;
			find.open = true;
			find.query = "needle".into();
			find.search.hits = ["thread", "previous"]
				.map(|thread| super::ConversationMatch {
					thread_id: super::EntityId::new(thread).unwrap(),
					turn_id: super::WireText::new("turn").unwrap(),
					item_id: super::WireText::new("item-12").unwrap(),
					snippet: "Earlier needle".into(),
				})
				.to_vec();
			assert_eq!(s.conversation_targets().len(), 3);
			s.select_conversation_match(true, cx);
			assert_eq!(s.located_match_excerpt().as_deref(), Some("Earlier needle"));
			assert!(s.is_located_item(Some("previous"), &s.timeline.native.entries[2].content));
			assert!(!s.is_located_item(Some("thread"), &s.timeline.native.entries[2].content));
		});
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
			assert_eq!(s.matching_entries().len(), 2);
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
			assert_eq!(s.matching_entries().len(), 2);
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
			assert!(s.matching_entries().is_empty());
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
			assert_eq!(s.matching_entries().len(), 1);
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
