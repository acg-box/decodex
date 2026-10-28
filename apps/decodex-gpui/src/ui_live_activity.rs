//! Shared presentation sizes. Content owners keep their data, interaction and animation state.

use crate::ui_theme::CONTROL_GROUP_HEIGHT;

pub(crate) const MIN_CAPSULE_WIDTH: f32 = 72.;
pub(crate) const CAPSULE_HEIGHT: f32 = CONTROL_GROUP_HEIGHT;

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
		let room = if allocations.len() == 2 {
			// The two controls keep their inner edges fixed while expanding outward.
			// Only half of the unused centered space is available on either side.
			let resting: f32 = allocations.iter().map(|item| item.width).sum();
			allocations[index].width + ((budget - resting) / 2.).max(0.)
		} else {
			(budget - others).max(0.)
		};
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

#[cfg(test)]
mod tests {
	use crate::ui_live_activity::{self, Presentation, Sizing};

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
