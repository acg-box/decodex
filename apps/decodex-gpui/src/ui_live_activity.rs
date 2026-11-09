//! Activity-island sizing and motion. Content owners supply data and actions.

use std::time::{Duration, Instant};

use crate::ui_theme::CONTROL_GROUP_HEIGHT;

pub(crate) const MIN_CAPSULE_WIDTH: f32 = 72.;
pub(crate) const CAPSULE_HEIGHT: f32 = CONTROL_GROUP_HEIGHT;
pub(crate) const ICON_SIZE: f32 = 14.;
pub(crate) const ICON_GAP: f32 = 6.;
pub(crate) const ICON_SLOT: f32 = ICON_SIZE + ICON_GAP;
pub(crate) const COMPACT_WIDTH: f32 = 208.;
pub(crate) const EXPANDED_WIDTH: f32 = 288.;
pub(crate) const MORPH_DURATION: Duration = Duration::from_millis(240);
pub(crate) const ISLAND_GAP: f32 = 6.;
pub(crate) const EXPANDED_RADIUS: f32 = 20.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Extent {
	pub width: f32,
	pub height: f32,
}

/// One clock moves both dimensions. Retargeting starts at the visible position.
#[derive(Default)]
pub(crate) struct Morph {
	state: Option<(Extent, Extent, Instant)>,
}
impl Morph {
	pub(crate) fn sample(&mut self, target: Extent, now: Instant, reduced: bool) -> (Extent, bool) {
		let (from, to, started) = *self.state.get_or_insert((target, target, now));
		let progress = if reduced {
			1.
		} else {
			(now.duration_since(started).as_secs_f32() / MORPH_DURATION.as_secs_f32()).min(1.)
		};
		let ease = 1. - (1. - progress).powi(3);
		let current = Extent {
			width: from.width + (to.width - from.width) * ease,
			height: from.height + (to.height - from.height) * ease,
		};
		if reduced {
			self.state = Some((target, target, now));
			return (target, false);
		}
		if target != to {
			self.state = Some((current, target, now));
			return (current, current != target);
		}
		(current, from != to && progress < 1.)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Presentation {
	Minimal,
	Compact,
	Expanded,
}

#[derive(Clone, Copy)]
pub(crate) struct Sizing {
	pub minimal: f32,
	pub compact: f32,
	pub expanded: f32,
}
impl Sizing {
	/// Reserve predictable tiers; round content exceptions to the same grid.
	pub(crate) fn content(minimal: f32, compact: f32, expanded: f32) -> Self {
		let snap = |value: f32, grid: f32| (value / grid).ceil() * grid;
		let minimal = snap(minimal.max(MIN_CAPSULE_WIDTH), 8.);
		let compact = snap(compact.max(COMPACT_WIDTH).max(minimal), 16.);
		Self { minimal, compact, expanded: snap(expanded.max(EXPANDED_WIDTH).max(compact), 32.) }
	}

	pub(crate) fn presentation(self, width: f32) -> Presentation {
		if width >= self.expanded {
			Presentation::Expanded
		} else if width >= self.compact {
			Presentation::Compact
		} else {
			Presentation::Minimal
		}
	}

	fn width(self, mode: Presentation) -> f32 {
		match mode {
			Presentation::Minimal => self.minimal,
			Presentation::Compact => self.compact,
			Presentation::Expanded => self.expanded,
		}
	}
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Allocation {
	pub presentation: Presentation,
	pub width: f32,
}

/// Multiple activities rest in Minimal. Only the selected activity claims extra space.
pub(crate) fn arrange(
	available: f32,
	gap: f32,
	sizes: &[Sizing],
	selected: Option<usize>,
) -> Vec<Allocation> {
	let budget = (available - gap * sizes.len().saturating_sub(1) as f32).max(0.);
	let minimum: f32 = sizes.iter().map(|size| size.minimal.max(MIN_CAPSULE_WIDTH)).sum();
	let ratio = (budget / minimum.max(1.)).min(1.);
	let mut allocations: Vec<_> = sizes
		.iter()
		.map(|size| Allocation {
			presentation: Presentation::Minimal,
			width: size.minimal.max(MIN_CAPSULE_WIDTH) * ratio,
		})
		.collect();

	if let Some(index) = selected.filter(|&index| index < sizes.len()) {
		let others: f32 = allocations
			.iter()
			.enumerate()
			.filter(|(i, _)| *i != index)
			.map(|(_, item)| item.width)
			.sum();
		let room = (budget - others).max(0.);
		let resting = sizes[index].minimal.max(MIN_CAPSULE_WIDTH);
		let size = Sizing {
			minimal: resting,
			compact: sizes[index].compact.max(resting),
			expanded: sizes[index].expanded.max(resting),
		};
		let presentation = size.presentation(room);

		// A compact presentation needs only its own content width, not the unused
		// remainder of a budget that cannot fit the expanded presentation.
		allocations[index] = Allocation { presentation, width: size.width(presentation).min(room) };
	}

	allocations
}

/// Keep the resting anchors until expansion reaches an edge. Then translate
/// the pair only as far as needed to use the available space.
pub(crate) fn paired_offset(available: f32, gap: f32, resting: [f32; 2], widths: [f32; 2]) -> f32 {
	let total = resting[0] + gap + resting[1];
	let left = resting[0] - widths[0];
	let right = resting[0] + gap + widths[1];
	let limit_left = (total - available) / 2.;
	let limit_right = (total + available) / 2.;
	if left < limit_left {
		limit_left - left
	} else if right > limit_right {
		limit_right - right
	} else {
		0.
	}
}

#[cfg(test)]
mod tests {
	use crate::ui_live_activity::{self, Extent, Morph, Presentation, Sizing};
	use std::time::{Duration, Instant};

	#[test]
	fn island_morph_keeps_one_path_on_resize_reversal_and_reduced_motion() {
		let now = Instant::now();
		let closed = Extent { width: 72., height: 28. };
		let open = Extent { width: 288., height: 140. };
		let mut motion = Morph::default();
		assert_eq!(motion.sample(closed, now, false), (closed, false));
		motion.sample(open, now, false);
		let mid = now + Duration::from_millis(90);
		let (visible, moving) = motion.sample(open, mid, false);
		assert!(moving && visible.width > closed.width && visible.width < open.width);
		assert!(
			((visible.width - closed.width) / (open.width - closed.width)
				- (visible.height - closed.height) / (open.height - closed.height))
				.abs()
				< 0.0001
		);
		assert_eq!(motion.sample(closed, mid, false).0, visible);
		let reversed = motion.sample(closed, mid + Duration::from_millis(60), false).0;
		assert!(reversed.width < visible.width && reversed.height < visible.height);
		assert_eq!(motion.sample(open, mid, true), (open, false));
	}

	#[test]
	fn activities_release_space_and_only_one_expands() {
		let sizes = [
			Sizing { minimal: 64., compact: 256., expanded: 288. },
			Sizing { minimal: 58., compact: 148., expanded: 272. },
		];
		let resting = ui_live_activity::arrange(400., 6., &sizes, None);

		assert!(resting.iter().all(|item| item.presentation == Presentation::Minimal));
		assert_eq!(resting.iter().map(|item| item.width).sum::<f32>(), 144.);
		assert_eq!(resting[0].width, resting[1].width);

		let focus = ui_live_activity::arrange(550., 6., &sizes, Some(1));

		assert_eq!(focus[0].presentation, Presentation::Minimal);
		assert_eq!(focus[1].presentation, Presentation::Expanded);
		assert_eq!(focus[0].width + focus[1].width + 6., 350.);

		let narrow = ui_live_activity::arrange(310., 6., &sizes, Some(1));

		assert_eq!(narrow[1].presentation, Presentation::Compact);
		assert_eq!(narrow[1].width, 148., "unused room is not padding for a compact activity");
		assert!(narrow.iter().map(|item| item.width).sum::<f32>() + 6. <= 310.);
		let narrow_rest = ui_live_activity::arrange(310., 6., &sizes, None);
		let centered_right = (310. + narrow_rest[0].width + 6. + narrow_rest[1].width) / 2.;
		assert!(centered_right + narrow[1].width - narrow_rest[1].width <= 310.);
		assert_eq!(sizes[0].presentation(120.), Presentation::Minimal);

		let mut uneven = sizes;

		uneven[0].minimal = 108.;

		let resting = ui_live_activity::arrange(400., 6., &uneven, None);

		assert_eq!(resting[0].width, 108.);
		assert_eq!(resting[1].width, 72., "one activity must not widen the other");
	}
}
