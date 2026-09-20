//! Native timeline page state. Native IDs remain distinct from local inbox row IDs.
use super::*;
use decodex_protocol::{ChiefTimelineContent as Content, ChiefTimelineEntry, ChiefTimelinePage};
use std::collections::BTreeSet;
#[path = "chief_timeline_inputs.rs"] mod inputs;
#[path = "chief_timeline_media.rs"] mod media;
#[path = "chief_timeline_receipts.rs"] mod receipts;
#[path = "chief_timeline_render.rs"] mod render;
#[path = "chief_timeline_scroll.rs"] mod scroll;

impl ChiefSurface {
	pub(super) fn refresh_open_native_history(&mut self, cx: &mut Context<Self>) {
		let Some((work, thread)) = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot
				.work_items
				.iter()
				.find(|work| Some(&work.id) == self.selected.as_ref())
				.and_then(|work| Some((work.id.clone(), work.codex_thread_id.clone()?)))
		}) else {
			self.native_history.reset();
			return;
		};
		if self.native_history.requested.as_ref() != Some(&(work.clone(), thread.clone())) {
			self.native_history.reset();
			self.native_history.viewport.request_latest();
		}
		let turn = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot.work_items.iter().find(|item| item.id == work)?.active_turn_id.as_deref()
		});
		self.native_history.retry_after_turn_change(turn);
		if self.native_history.can_retry(std::time::Instant::now()) {
			self.load_native_timeline(&work, &thread, false, cx);
		}
	}

	pub(super) fn native_history_active(&self, work: &ChiefWorkItemDto) -> bool {
		!self.native_history.show_saved
			&& self.native_history.binding.as_ref().is_some_and(|binding| {
				binding.work == work.id && Some(&binding.thread) == work.codex_thread_id.as_ref()
			})
	}

	pub(super) fn native_timeline_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let owner = work.id.clone();
		let thread = work.codex_thread_id.clone();
		let mut panel = div().flex().flex_col().gap_2().child(
			div().debug_selector(|| "native-latest-action".into()).child(self.workspace_action(
				"native-timeline-refresh".into(),
				"Latest native history".into(),
				move |s, cx| {
					if let Some(thread) = &thread {
						s.read_latest_native_history(&owner, thread, cx);
					}
				},
				cx,
			)),
		);
		if self.native_history.requested.as_ref().is_some_and(|(id, thread)| {
			id == &work.id && Some(thread) == work.codex_thread_id.as_ref()
		}) {
			if self.native_history.task.is_some() {
				panel = panel.child(muted("Loading native history…"));
			} else if let Some(message) = self.native_history.notice {
				panel = panel.child(muted(message));
			}
		}
		if self
			.native_history
			.binding
			.as_ref()
			.is_some_and(|b| b.work == work.id && Some(&b.thread) == work.codex_thread_id.as_ref())
		{
			panel = panel.child(
				div().debug_selector(|| "native-history-source-toggle".into()).child(
					self.workspace_action(
						"native-history-source".into(),
						if self.native_history.show_saved {
							"Show conversation"
						} else {
							"Show saved local records"
						}
						.into(),
						|s, cx| {
							s.cancel_native_scroll_anchor();
							s.native_history.show_saved = !s.native_history.show_saved;
							s.history_navigation = None;
							cx.notify();
						},
						cx,
					),
				),
			);
			if self.native_history.show_saved {
				return panel
					.child(muted(
						"Saved local records can include earlier thread bindings and delivery receipts.",
					))
					.into_any_element();
			}
			let binding = self.native_history.binding.as_ref().expect("matching binding").clone();
			if self.native_history.browsing_window {
				panel = panel.child(muted("Showing an earlier history window. Select Latest native history to return to recent messages."));
			}
			if self.native_history.older_cursor.is_some() {
				panel = panel.child(self.workspace_action(
					"native-timeline-older".into(),
					"Read earlier native history".into(),
					move |s, cx| s.load_native_timeline(&binding.work, &binding.thread, true, cx),
					cx,
				));
			}
			if self.native_history.opening_session.is_some() {
				panel = panel.child(muted("Voice conversation continued from an earlier page."));
			}
			for entry in &self.native_history.entries {
				panel = panel.child(self.native_timeline_row(work, entry, cx));
			}
		}
		panel.into_any_element()
	}

	fn read_latest_native_history(&mut self, work: &str, thread: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work) || self.native_history.task.is_some() {
			return;
		}
		self.cancel_native_scroll_anchor();
		self.native_history.epoch = self.native_history.epoch.wrapping_add(1);
		self.native_history.recovered();
		self.native_history.show_saved = false;
		self.native_history.viewport.request_latest();
		self.history_follow_paused.remove(work);
		self.history_navigation = None;
		self.set_voice_follow(true);
		self.load_native_timeline(work, thread, false, cx);
	}

	fn refresh_native_history(&mut self, binding: Binding, page: ChiefTimelinePage) -> bool {
		let jump = self.native_history.viewport.take_latest_request();
		let work = binding.work.clone();
		let accepted = if jump {
			self.native_history.replace(binding, page)
		} else {
			self.native_history.refresh(binding, page)
		};
		if accepted && jump {
			self.transcript_scroll.entry(work).or_default().scroll_to_bottom();
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
		if self.selected.as_deref() != Some(work) || self.native_history.task.is_some() {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let cursor = if older { self.native_history.older_cursor.clone() } else { None };
		if older && cursor.is_none() {
			return;
		}
		let (Ok(work_id), Ok(thread_id)) = (EntityId::new(work), EntityId::new(thread)) else {
			return;
		};
		if older {
			self.history_follow_paused.insert(work.into());
			self.history_navigation = None;
			self.set_voice_follow(false);
		}
		let (work, thread) = (work.to_owned(), thread.to_owned());
		let epoch = self.native_history.epoch;
		self.native_history.requested = Some((work.clone(), thread.clone()));
		self.native_history.requested_turn = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot.work_items.iter().find(|item| item.id == work)?.active_turn_id.clone()
		});
		let sent_cursor = cursor.clone();
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime
				.block_on(ChiefClient::new(profile).timeline(
					work_id,
					thread_id,
					cursor.map(WireText::new).transpose().ok()?,
				))
				.ok()
		});
		self.native_history.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.native_history.epoch != epoch {
					return;
				}
				s.native_history.task = None;
				if s.selected.as_deref() != Some(&work)
					|| !s.snapshot.as_ref().is_some_and(|v| {
						v.work_items
							.iter()
							.any(|w| w.id == work && w.codex_thread_id.as_deref() == Some(&thread))
					}) {
					return;
				}
				if let Some(decodex_protocol::ChiefTimelineResult::Available {
					account_id,
					page,
					..
				}) = result
				{
					let binding = Binding { work, thread, account: account_id.as_str().into() };
					let accepted = match sent_cursor {
						Some(cursor) => s.prepend_native_history(&binding, &cursor, page),
						None => s.refresh_native_history(binding, page),
					};
					if accepted {
						s.native_history.recovered();
					} else {
						s.native_history.notice = Some(
							"History changed or the display limit was reached. Refresh native history.",
						);
					}
				} else {
					s.native_history.failed(result, std::time::Instant::now());
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
	preview: media::Preview,
	input_receipts: inputs::InputReceipts,
	pub task: Option<Task<()>>,
	pub epoch: u64,
	pub binding: Option<Binding>,
	pub entries: Vec<ChiefTimelineEntry>,
	pub older_cursor: Option<String>,
	pub opening_session: Option<String>,
	requested: Option<(String, String)>,
	requested_turn: Option<String>,
	retry_at: Option<std::time::Instant>,
	failures: u32,
	unsupported: bool,
	browsing_window: bool,
	show_saved: bool,
	viewport: scroll::Viewport,
	notice: Option<&'static str>,
	seen_cursors: BTreeSet<String>,
}

impl Timeline {
	fn reset(&mut self) {
		*self = Self { epoch: self.epoch.wrapping_add(1), ..Default::default() };
	}

	fn can_retry(&self, now: std::time::Instant) -> bool {
		!self.unsupported && !self.browsing_window && self.retry_at.is_none_or(|at| now >= at)
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
		result: Option<decodex_protocol::ChiefTimelineResult>,
		now: std::time::Instant,
	) {
		self.clear_page();
		self.unsupported =
			matches!(result, Some(decodex_protocol::ChiefTimelineResult::Unsupported));
		self.failures = self.failures.saturating_add(1);
		self.retry_at = Some(
			now + std::time::Duration::from_secs(
				(5_u64 << self.failures.saturating_sub(1).min(3)).min(30),
			),
		);
		self.notice = Some(if self.unsupported {
			"This thread does not support native history. Saved local history remains available."
		} else {
			"Native history could not be loaded. Retrying… Saved local history remains available."
		});
	}

	fn clear_page(&mut self) {
		self.preview.clear();
		self.viewport = Default::default();
		self.binding = None;
		self.entries.clear();
		self.older_cursor = None;
		self.opening_session = None;
		self.seen_cursors.clear();
		self.browsing_window = false;
	}

	fn refresh(&mut self, binding: Binding, page: ChiefTimelinePage) -> bool {
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
			self.entries.splice(start.., page.entries);
			return true;
		}
		// Without overlap the middle is unknown. Restart at the native page boundary;
		// retaining old rows here would hide an unobserved gap behind a false adjacency.
		self.replace(binding, page)
	}

	pub(super) fn replace(&mut self, binding: Binding, page: ChiefTimelinePage) -> bool {
		if page.thread_id != binding.thread
			|| !valid_entries(&page.entries)
			|| !bounded(&page.entries)
		{
			return false;
		}
		self.viewport = Default::default();
		if self.binding.as_ref() != Some(&binding) {
			self.preview.clear();
		}
		self.binding = Some(binding);
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
		page: ChiefTimelinePage,
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
		self.entries.splice(0..0, page.entries);
		while !bounded(&self.entries) {
			self.entries.pop();
			self.browsing_window = true;
		}
		self.older_cursor = page.next_cursor;
		self.viewport.retain(&self.entries);
		true
	}
}

fn bounded<'a>(entries: impl IntoIterator<Item = &'a ChiefTimelineEntry>) -> bool {
	let mut bytes = 0;
	let mut count = 0;
	for entry in entries {
		count += 1;
		let Ok(encoded) = serde_json::to_vec(entry) else {
			return false;
		};
		bytes += encoded.len();
		if count > 1000 || bytes > 2 * 1024 * 1024 {
			return false;
		}
	}
	true
}

fn valid_entries(entries: &[ChiefTimelineEntry]) -> bool {
	entries.windows(2).all(|pair| key(&pair[0]) < key(&pair[1]))
}

pub(super) fn key(entry: &ChiefTimelineEntry) -> (u64, u8, &str) {
	let (kind, id) = match &entry.content {
		Content::TurnBoundary { turn_id, completed: false, .. } => (0, turn_id),
		Content::Item { item_id, .. } => (1, item_id),
		Content::Speech { item_id, .. }
		| Content::VoiceBoundary { item_id, .. }
		| Content::Promotion { item_id, .. } => (2, item_id),
		Content::TurnBoundary { turn_id, completed: true, .. } => (3, turn_id),
	};
	(entry.position, kind, id)
}

#[cfg(test)]
mod tests {
	use super::*;
	fn binding() -> Binding {
		Binding { work: "work".into(), thread: "thread".into(), account: "account".into() }
	}
	fn boundary(position: u64, completed: bool) -> ChiefTimelineEntry {
		ChiefTimelineEntry {
			position,
			content: Content::TurnBoundary {
				turn_id: "turn".into(),
				completed,
				status: None,
				duration_ms: None,
				usage_summary: None,
				error: None,
			},
		}
	}
	fn page(
		entries: Vec<ChiefTimelineEntry>,
		next: Option<&str>,
		session: Option<&str>,
	) -> ChiefTimelinePage {
		ChiefTimelinePage {
			thread_id: "thread".into(),
			entries,
			next_cursor: next.map(str::to_owned),
			active_realtime_session_at_page_start: session.map(str::to_owned),
		}
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
		state.failed(Some(decodex_protocol::ChiefTimelineResult::Unsupported), now);
		assert!(!state.can_retry(now + std::time::Duration::from_secs(60)));
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
		state.failed(Some(decodex_protocol::ChiefTimelineResult::Unsupported), now);
		state.retry_after_turn_change(None);
		assert!(!state.can_retry(now + std::time::Duration::from_secs(300)));
		state.retry_after_turn_change(Some("first-turn"));
		assert!(state.can_retry(now));
		// A legacy thread can still refuse the retry. Do not keep polling it for
		// the same acknowledged turn, including after terminal status changes.
		state.requested_turn = Some("first-turn".into());
		state.failed(Some(decodex_protocol::ChiefTimelineResult::Unsupported), now);
		state.retry_after_turn_change(Some("first-turn"));
		assert!(!state.can_retry(now + std::time::Duration::from_secs(300)));
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
			page((1..=1000).map(|n| boundary(n, false)).collect(), Some("old"), None)
		));
		assert!(state.prepend(&binding(), "old", page(vec![boundary(0, false)], None, None)));
		assert_eq!(state.entries.len(), 1000);
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
