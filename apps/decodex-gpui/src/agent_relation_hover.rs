//! One hover owner for the canvas. Child text and changing edge labels are not hit targets.
use std::time::{Duration, Instant};

const LEAVE_GRACE: Duration = Duration::from_millis(120);

#[derive(Default, Clone, Copy, PartialEq)]
enum Source {
	#[default]
	Canvas,
	Allocation,
}

#[derive(Default)]
pub(super) struct Hover {
	key: Option<String>,
	left: Option<Instant>,
	source: Source,
}
impl Hover {
	pub(super) fn current(&self, now: Instant) -> Option<&str> {
		self.key
			.as_deref()
			.filter(|_| self.left.is_none_or(|left| now.duration_since(left) < LEAVE_GRACE))
	}

	pub(super) fn update(&mut self, next: Option<String>, now: Instant) -> bool {
		self.update_from(Source::Canvas, next, now)
	}

	pub(super) fn allocation(&mut self, key: &str, inside: bool, now: Instant) -> bool {
		if inside {
			self.update_from(Source::Allocation, Some(key.into()), now)
		} else if self.key.as_deref() == Some(key) {
			self.update_from(Source::Allocation, None, now)
		} else {
			false
		}
	}

	fn update_from(&mut self, source: Source, next: Option<String>, now: Instant) -> bool {
		if let Some(next) = next {
			let changed =
				self.source != source || self.key.as_ref() != Some(&next) || self.left.is_some();
			self.key = Some(next);
			self.left = None;
			self.source = source;
			changed
		} else if self.source == source && self.key.is_some() && self.left.is_none() {
			self.left = Some(now);
			true
		} else {
			false
		}
	}

	pub(super) fn pending(&self, now: Instant) -> bool {
		self.left.is_some_and(|left| now.duration_since(left) < LEAVE_GRACE)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn leaving_one_surface_does_not_clear_the_other_surfaces_hover() {
		let now = Instant::now();
		let mut hover = Hover::default();
		hover.update(Some("one".into()), now);
		hover.allocation("two", true, now);
		assert!(!hover.update(None, now));
		assert_eq!(hover.current(now + LEAVE_GRACE), Some("two"));
		hover.allocation("three", true, now + LEAVE_GRACE);
		assert!(!hover.allocation("two", false, now + LEAVE_GRACE));
		assert_eq!(hover.current(now + 2 * LEAVE_GRACE), Some("three"));
		hover.update(Some("one".into()), now + LEAVE_GRACE);
		assert!(!hover.allocation("three", false, now + LEAVE_GRACE));
		assert_eq!(hover.current(now + 2 * LEAVE_GRACE), Some("one"));
		hover.update(None, now + 2 * LEAVE_GRACE);
		assert_eq!(hover.current(now + 3 * LEAVE_GRACE), None);
	}

	#[test]
	fn crossing_a_small_gap_or_repainting_children_does_not_clear_the_relation() {
		let now = Instant::now();
		let mut hover = Hover::default();
		assert!(hover.update(Some("one".into()), now));
		assert!(!hover.update(Some("one".into()), now));
		assert!(hover.update(None, now));
		assert_eq!(hover.current(now + Duration::from_millis(80)), Some("one"));
		assert!(hover.update(Some("two".into()), now + Duration::from_millis(90)));
		assert_eq!(hover.current(now + Duration::from_millis(90)), Some("two"));
		assert!(!hover.pending(now + Duration::from_millis(90)));
		hover.update(None, now + Duration::from_millis(100));
		assert_eq!(hover.current(now + Duration::from_millis(221)), None);
		assert!(!hover.pending(now + Duration::from_millis(221)));
	}
}
