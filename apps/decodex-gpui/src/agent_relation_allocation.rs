//! Recorded allocation changes use the existing interruptible motion clock.
use crate::ui_motion;
use gpui::{App, Div, IntoElement, RenderOnce, SharedString, Stateful, Window, prelude::*};

pub(super) struct Segment {
	pub share: f32,
	pub child: Stateful<Div>,
}

#[derive(IntoElement)]
pub(super) struct Strip {
	pub segments: Vec<Segment>,
	pub width: f32,
	pub gap: f32,
}

impl RenderOnce for Strip {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		// Animate ranked shares, then lay out one continuous partition. Independent
		// position tweens let segments cross and produce overlapping hit targets.
		let segments: Vec<_> = self
			.segments
			.into_iter()
			.enumerate()
			.map(|(rank, segment)| {
				let share = ui_motion::value(
					SharedString::from(format!("graph-allocation-rank-{rank}")),
					segment.share,
					window,
					cx,
				);
				(segment.child, share)
			})
			.collect();
		let total = segments.iter().map(|(_, share)| share).sum::<f32>();
		let available = (self.width - segments.len().saturating_sub(1) as f32 * self.gap).max(0.);
		let density = window.scale_factor();
		let snap = |value: f32| (value * density).round() / density;
		let mut offset = 0.;
		let mut strip = gpui::div().flex_1().min_w_0().h_full().relative();
		for (child, share) in segments {
			let width = if total > 0. { available * share / total } else { 0. };
			strip = strip.child(
				gpui::div()
					.absolute()
					.top_0()
					.left(gpui::px(snap(offset)))
					.w(gpui::px((snap(offset + width) - snap(offset)).max(0.)))
					.h_full()
					.child(child.w_full()),
			);
			offset += width + self.gap;
		}
		strip
	}
}

#[derive(IntoElement)]
pub(super) struct Mark {
	pub key: String,
	pub selected: bool,
	pub hovered: bool,
}

impl RenderOnce for Mark {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let selected = ui_motion::value(
			SharedString::from(format!("graph-allocation-selection-{}", self.key)),
			if self.selected { 1. } else { 0. },
			window,
			cx,
		);
		let hovered = ui_motion::value(
			SharedString::from(format!("graph-allocation-hover-{}", self.key)),
			if self.hovered { 1. } else { 0. },
			window,
			cx,
		);
		let base = crate::ui_theme::rgb(0xaaa0c4);
		let bright = crate::ui_theme::rgb(0xd1c2f4);
		let from = gpui::Rgba {
			r: base.r + (bright.r - base.r) * hovered,
			g: base.g + (bright.g - base.g) * hovered,
			b: base.b + (bright.b - base.b) * hovered,
			a: 1.,
		};
		let to = crate::ui_theme::rgb(crate::ui_theme::BLUE);
		let selector = format!("graph-allocation-mark-{}", self.key);
		gpui::div()
			.debug_selector(move || selector.clone())
			.w_full()
			.h(gpui::px(2.))
			.flex_none()
			.bg(gpui::Rgba {
				r: from.r + (to.r - from.r) * selected,
				g: from.g + (to.g - from.g) * selected,
				b: from.b + (to.b - from.b) * selected,
				a: 0.7 + 0.3 * selected.max(hovered),
			})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	struct Fixture(f32);
	impl gpui::Render for Fixture {
		fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			let portion = |key: &str, share| Segment {
				share,
				child: gpui::div()
					.id(SharedString::from(key.to_owned()))
					.debug_selector({
						let key = key.to_owned();
						move || key.clone()
					})
					.min_w_0()
					.h_full(),
			};
			let mut segments = vec![portion("first", self.0), portion("second", 1. - self.0)];
			segments.sort_by(|a, b| b.share.total_cmp(&a.share));
			gpui::div().w(gpui::px(200.)).h(gpui::px(16.)).flex().child(Strip {
				segments,
				width: 200.,
				gap: 0.,
			})
		}
	}

	#[gpui::test]
	fn ranked_shares_animate_and_reverse_without_overlapping_hit_targets(
		cx: &mut gpui::TestAppContext,
	) {
		let (view, visual) = cx.add_window_view(|_, _| Fixture(0.6));
		let draw =
			|visual: &mut gpui::VisualTestContext| visual.update(crate::ui_frame::draw_frame);
		draw(visual);
		std::thread::sleep(std::time::Duration::from_millis(230));
		draw(visual);
		let width = |visual: &mut gpui::VisualTestContext, selector: &'static str| {
			f32::from(visual.debug_bounds(selector).unwrap().size.width)
		};
		let left = |visual: &mut gpui::VisualTestContext, selector: &'static str| {
			f32::from(visual.debug_bounds(selector).unwrap().left())
		};
		let partition = |visual: &mut gpui::VisualTestContext| {
			let a = visual.debug_bounds("first").unwrap();
			let b = visual.debug_bounds("second").unwrap();
			assert!(
				a.right() <= b.left() || b.right() <= a.left(),
				"segments overlap: {a:?}, {b:?}"
			);
			assert!((f32::from(a.size.width + b.size.width) - 200.).abs() < 1.);
		};
		assert!((width(visual, "first") - 120.).abs() < 1.);
		assert_eq!(left(visual, "first"), 0.);
		view.update(visual, |s, cx| {
			s.0 = 0.9;
			cx.notify();
		});
		draw(visual);
		assert!(width(visual, "first") < 140.);
		std::thread::sleep(std::time::Duration::from_millis(60));
		draw(visual);
		let segment = width(visual, "first");
		assert!(segment > 120. && segment < 180., "strip must interpolate: {segment}");
		partition(visual);
		view.update(visual, |s, cx| {
			s.0 = 0.7;
			cx.notify();
		});
		draw(visual);
		assert!((width(visual, "first") - segment).abs() < 15.);
		partition(visual);
		std::thread::sleep(std::time::Duration::from_millis(230));
		draw(visual);
		assert!((width(visual, "first") - 140.).abs() < 1.);
		assert!((width(visual, "second") - 60.).abs() < 1.);
		view.update(visual, |s, cx| {
			s.0 = 0.2;
			cx.notify();
		});
		draw(visual);
		partition(visual);
		assert_eq!(left(visual, "second"), 0., "rank changes never cross other segments");
		std::thread::sleep(std::time::Duration::from_millis(60));
		draw(visual);
		partition(visual);
		std::thread::sleep(std::time::Duration::from_millis(230));
		draw(visual);
		partition(visual);
		assert!((width(visual, "first") - 40.).abs() < 1.);
		assert_eq!(left(visual, "first"), 160.);
	}
}
