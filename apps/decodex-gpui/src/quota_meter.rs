//! One value drives the quota percentage and bar during a confirmed reset.
use decodex_protocol::{AccountQuotaStateDto, AccountQuotaWindowDto, EntityRevision};
use gpui::{App, IntoElement, RenderOnce, Window, div, prelude::*, px, rgb, rgba};
use std::time::{Duration, Instant};

pub(super) const FILL_DURATION: Duration = Duration::from_millis(850);

#[derive(Clone)]
pub(super) struct ResetFill {
	pub(super) revision: EntityRevision,
	pub(super) initial: [AccountQuotaWindowDto; 2],
	pub(super) started: Instant,
	pub(super) confirmed_at_micros: i64,
}
impl ResetFill {
	fn remaining(&self, quota: AccountQuotaWindowDto, now: Instant) -> Option<f32> {
		let initial =
			self.initial.iter().find(|window| window.duration_minutes == quota.duration_minutes)?;
		let from = remaining(*initial)?;
		let elapsed = now.saturating_duration_since(self.started);
		if elapsed >= FILL_DURATION
			&& quota.observed_at_unix_micros.is_some_and(|at| at > self.confirmed_at_micros)
		{
			return None;
		}
		Some(fill_value(from, elapsed))
	}
}
fn fill_value(from: f32, elapsed: Duration) -> f32 {
	let progress = (elapsed.as_secs_f32() / FILL_DURATION.as_secs_f32()).clamp(0.0, 1.0);
	let eased = 1.0 - (1.0 - progress).powi(3);
	from + (100.0 - from) * eased
}
fn remaining(quota: AccountQuotaWindowDto) -> Option<f32> {
	match quota.result {
		AccountQuotaStateDto::Current { used_percent, .. } => Some(100.0 - f32::from(used_percent)),
		_ => None,
	}
}
#[derive(IntoElement)]
pub(super) struct QuotaMeter {
	pub(super) label: &'static str,
	pub(super) quota: AccountQuotaWindowDto,
	pub(super) fill: Option<ResetFill>,
}
impl RenderOnce for QuotaMeter {
	fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
		let now = Instant::now();
		let animated = self.fill.as_ref().and_then(|fill| fill.remaining(self.quota, now));
		let value = animated.or_else(|| remaining(self.quota)).unwrap_or(0.0).clamp(0.0, 100.0);
		if animated.is_some()
			&& self
				.fill
				.as_ref()
				.is_some_and(|fill| now.saturating_duration_since(fill.started) < FILL_DURATION)
		{
			window.request_animation_frame();
		}
		let color = if value <= 10.0 {
			0xef4444
		} else if value <= 30.0 {
			super::WB_AMBER
		} else {
			super::WB_BLUE
		};
		div()
			.w(px(122.))
			.flex()
			.flex_col()
			.gap_1()
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.font_family(crate::ui_theme::FONT_FAMILY)
					.text_size(px(11.))
					.text_color(rgb(super::WB_TEXT_FAINT))
					.child(self.label)
					.child(format!("{value:.0}% left")),
			)
			.child(
				div()
					.h(px(3.))
					.w_full()
					.rounded_full()
					.bg(rgba(0xffffff0c))
					.child(div().h_full().w(px(value * 1.22)).rounded_full().bg(rgb(color))),
			)
	}
}
pub(super) fn meter(
	label: &'static str,
	quota: AccountQuotaWindowDto,
	fill: Option<ResetFill>,
) -> Option<gpui::AnyElement> {
	let has_fill = fill.as_ref().and_then(|fill| fill.remaining(quota, Instant::now())).is_some();
	if quota.result == AccountQuotaStateDto::NotApplicable
		|| (remaining(quota).is_none() && !has_fill)
	{
		return None;
	}
	Some(QuotaMeter { label, quota, fill }.into_any_element())
}
#[cfg(test)]
mod tests {
	use super::*;
	fn quota(used: u8, observed: i64) -> AccountQuotaWindowDto {
		AccountQuotaWindowDto {
			duration_minutes: 300,
			observed_at_unix_micros: Some(observed),
			result: AccountQuotaStateDto::Current {
				used_percent: used,
				resets_at_unix_micros: i64::MAX,
			},
		}
	}
	#[test]
	fn percentage_and_bar_value_fill_smoothly_without_overshoot() {
		for start in [0., 36., 72., 100.] {
			assert_eq!(fill_value(start, Duration::ZERO), start);
			let samples: Vec<_> =
				(0..=100).map(|step| fill_value(start, Duration::from_millis(step * 10))).collect();
			assert!(samples.windows(2).all(|pair| pair[0] <= pair[1]));
			assert!(samples.iter().all(|value| *value >= start && *value <= 100.));
			assert_eq!(fill_value(start, FILL_DURATION), 100.);
		}
	}
	#[test]
	fn stale_observations_do_not_snap_back_and_fresh_usage_takes_over_after_animation() {
		let started = Instant::now();
		let fill = ResetFill {
			revision: EntityRevision(1),
			initial: [quota(64, 1), quota(28, 1)],
			started,
			confirmed_at_micros: 10,
		};
		assert_eq!(fill.remaining(quota(0, 11), started), Some(36.));
		assert_eq!(fill.remaining(quota(64, 1), started + FILL_DURATION), Some(100.));
		assert_eq!(fill.remaining(quota(1, 11), started + FILL_DURATION), None);
		assert_eq!(remaining(quota(1, 11)), Some(99.));
	}
}
