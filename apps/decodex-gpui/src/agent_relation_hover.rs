//! One hover owner for the canvas. Child text and changing edge labels are not hit targets.
use std::time::{Duration, Instant};

const LEAVE_GRACE: Duration = Duration::from_millis(120);

#[derive(Default)]
pub(super) struct Hover {
	key: Option<String>,
	left: Option<Instant>,
}
impl Hover {
	pub(super) fn current(&self, now: Instant) -> Option<&str> {
		self.key
			.as_deref()
			.filter(|_| self.left.is_none_or(|left| now.duration_since(left) < LEAVE_GRACE))
	}

	pub(super) fn update(&mut self, next: Option<String>, now: Instant) -> bool {
		if let Some(next) = next {
			let changed = self.key.as_ref() != Some(&next) || self.left.is_some();
			self.key = Some(next);
			self.left = None;
			changed
		} else if self.key.is_some() && self.left.is_none() {
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
