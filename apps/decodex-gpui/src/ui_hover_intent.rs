//! Dwell and departure grace for surfaces revealed by pointer intent.
use gpui::{Context, Task};

use super::{HOVER_CLOSE_DELAY as CLOSE_DELAY, HOVER_OPEN_DELAY as OPEN_DELAY};

/// Keep one pending target without restarting its timer on internal movement.
pub(crate) struct HoverIntent<K> {
	target: Option<K>,
	pending: Option<Task<()>>,
}
impl<K> Default for HoverIntent<K> {
	fn default() -> Self {
		Self { target: None, pending: None }
	}
}
impl<K: Clone + Eq + 'static> HoverIntent<K> {
	pub(crate) fn target(&self) -> Option<&K> {
		self.target.as_ref()
	}

	pub(crate) fn cancel(&mut self) {
		self.pending = None;
		self.target = None;
	}

	pub(crate) fn settled(&mut self) {
		self.pending = None;
	}

	pub(crate) fn update<T: 'static>(
		&mut self,
		key: K,
		inside: bool,
		current: Option<K>,
		cx: &mut Context<T>,
		apply: impl FnOnce(&mut T, Option<K>, &mut Context<T>) + 'static,
	) {
		let next = if inside {
			Some(key)
		} else if self.target.as_ref() == Some(&key) {
			None
		} else {
			return;
		};
		if self.target == next && self.pending.is_some() {
			return;
		}
		self.pending = None;
		self.target.clone_from(&next);
		if next == current {
			return;
		}
		let delay = if inside { OPEN_DELAY } else { CLOSE_DELAY };
		self.pending = Some(cx.spawn(async move |owner, cx| {
			cx.background_executor().timer(delay).await;
			let _ = owner.update(cx, move |owner, cx| {
				super::begin_interaction();
				apply(owner, next, cx);
			});
		}));
	}
}

#[cfg(test)]
mod tests {
	use super::{CLOSE_DELAY, HoverIntent, OPEN_DELAY};
	use gpui::{AppContext as _, Context};
	use std::time::Duration;

	#[derive(Default)]
	struct Fixture {
		hover: HoverIntent<u8>,
		open: Option<u8>,
		changes: Vec<Option<u8>>,
	}
	fn elapse(cx: &mut gpui::TestAppContext, delay: Duration) {
		cx.run_until_parked();
		cx.executor().advance_clock(delay);
		cx.run_until_parked();
	}
	impl Fixture {
		fn hover(&mut self, key: u8, inside: bool, cx: &mut Context<Self>) {
			self.hover.update(key, inside, self.open, cx, |s, next, cx| {
				s.hover.settled();
				s.open = next;
				s.changes.push(next);
				cx.notify();
			});
		}
	}

	#[gpui::test]
	fn brief_crossings_cancel_and_internal_movement_keeps_the_original_dwell(
		cx: &mut gpui::TestAppContext,
	) {
		let state = cx.new(|_| Fixture::default());
		state.update(cx, |s, cx| s.hover(1, true, cx));
		elapse(cx, OPEN_DELAY / 2);
		state.update(cx, |s, cx| s.hover(1, false, cx));
		elapse(cx, OPEN_DELAY);
		state.read_with(cx, |s, _| assert!(s.changes.is_empty()));
		state.update(cx, |s, cx| s.hover(1, true, cx));
		elapse(cx, OPEN_DELAY / 2);
		state.update(cx, |s, cx| s.hover(1, true, cx));
		elapse(cx, OPEN_DELAY - OPEN_DELAY / 2 - Duration::from_millis(1));
		state.read_with(cx, |s, _| assert!(s.open.is_none()));
		elapse(cx, Duration::from_millis(1));
		state.read_with(cx, |s, _| assert_eq!(s.open, Some(1)));
		state.update(cx, |s, cx| s.hover(1, false, cx));
		elapse(cx, CLOSE_DELAY / 2);
		state.update(cx, |s, cx| s.hover(1, true, cx));
		elapse(cx, CLOSE_DELAY);
		state.read_with(cx, |s, _| assert_eq!(s.changes, vec![Some(1)]));
		state.update(cx, |s, cx| s.hover(1, false, cx));
		elapse(cx, CLOSE_DELAY);
		state.read_with(cx, |s, _| assert_eq!(s.changes, vec![Some(1), None]));
	}

	#[gpui::test]
	fn stale_exit_cannot_cancel_the_next_target_and_explicit_cancel_stops_pending_work(
		cx: &mut gpui::TestAppContext,
	) {
		let state = cx.new(|_| Fixture::default());
		state.update(cx, |s, cx| {
			s.hover(1, true, cx);
			s.hover(2, true, cx);
			s.hover(1, false, cx);
		});
		elapse(cx, OPEN_DELAY);
		state.read_with(cx, |s, _| assert_eq!(s.open, Some(2)));
		state.update(cx, |s, cx| {
			s.hover(2, false, cx);
			s.hover.cancel();
		});
		elapse(cx, CLOSE_DELAY);
		state.read_with(cx, |s, _| assert_eq!(s.changes, vec![Some(2)]));
	}
}
