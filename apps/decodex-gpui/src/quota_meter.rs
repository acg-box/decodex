//! One value drives the quota percentage and bar during a confirmed reset.
use std::{
	ffi::CStr,
	mem::MaybeUninit,
	sync::Arc,
	time::{Duration, Instant},
};

use gpui::{
	AnyElement, App, FontFeatures, IntoElement, RenderOnce, Window,
	prelude::{InteractiveElement as _, ParentElement as _, Styled as _},
};
use libc::{c_char, time_t, tm};

use crate::{
	shell::{WB_AMBER, WB_BLUE, WB_TEXT_FAINT},
	ui_motion,
	ui_theme::{ERROR, FONT_FAMILY},
};
use decodex_protocol::{AccountQuotaStateDto, AccountQuotaWindowDto, EntityRevision};

pub(super) const FILL_DURATION: Duration = Duration::from_millis(850);

#[derive(Clone)]
pub(super) struct ResetFill {
	pub(super) revision: EntityRevision,
	pub(super) initial: [AccountQuotaWindowDto; 2],
	pub(super) started: Instant,
	pub(super) confirmed_at_micros: i64,
}
impl ResetFill {
	fn remaining(&self, quota: AccountQuotaWindowDto, now: Instant, reduced: bool) -> Option<f32> {
		if quota.result == AccountQuotaStateDto::NotApplicable {
			return None;
		}

		let initial =
			self.initial.iter().find(|window| window.duration_minutes == quota.duration_minutes)?;
		let from = remaining(*initial)?;
		let elapsed = now.saturating_duration_since(self.started);

		if (reduced || elapsed >= FILL_DURATION)
			&& quota.observed_at_unix_micros.is_some_and(|at| at > self.confirmed_at_micros)
		{
			return None;
		}

		Some(if reduced { 100.0 } else { fill_value(from, elapsed) })
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
		let reduced = ui_motion::reduced();
		let animated = self.fill.as_ref().and_then(|fill| fill.remaining(self.quota, now, reduced));
		let observed = animated.or_else(|| remaining(self.quota));
		let value = observed.unwrap_or(0.0).clamp(0.0, 100.0);

		if !reduced
			&& animated.is_some()
			&& self
				.fill
				.as_ref()
				.is_some_and(|fill| now.saturating_duration_since(fill.started) < FILL_DURATION)
		{
			window.request_animation_frame();
		}

		let color = if observed.is_some() { quota_color(value) } else { WB_TEXT_FAINT };

		gpui::div()
			.flex_1()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(4.))
			.font_family(FONT_FAMILY)
			.text_size(gpui::px(10.))
			.text_color(gpui::rgb(WB_TEXT_FAINT))
			.child(gpui::div().w(gpui::px(15.)).flex_none().child(self.label))
			.child(
				gpui::div()
					.h(gpui::px(3.))
					.flex_1()
					.min_w_0()
					.rounded_full()
					.bg(gpui::rgba(0xffffff0c))
					.opacity(if observed.is_some() { 1. } else { 0. })
					.child(
						gpui::div()
							.h_full()
							.w(gpui::relative(value / 100.))
							.rounded_full()
							.bg(gpui::rgb(color)),
					),
			)
			.child(
				gpui::div()
					.w(gpui::px(28.))
					.flex_none()
					.text_right()
					.text_color(gpui::rgb(color))
					.child(observed.map(|_| format!("{value:.0}%")).unwrap_or_else(|| "—".into())),
			)
			.child(
				gpui::div()
					.w(gpui::px(80.))
					.flex_none()
					.text_right()
					.whitespace_nowrap()
					.debug_selector(move || format!("quota-reset-{}", self.label))
					.font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
					.child(match self.quota.result {
						AccountQuotaStateDto::Current { resets_at_unix_micros, .. } =>
							reset_time(resets_at_unix_micros).unwrap_or_else(|| "—".into()),
						AccountQuotaStateDto::NotApplicable => "N/A".into(),
						_ => "—".into(),
					}),
			)
	}
}

pub(super) fn meter(
	label: &'static str,
	quota: AccountQuotaWindowDto,
	fill: Option<ResetFill>,
) -> AnyElement {
	QuotaMeter { label, quota, fill }.into_any_element()
}

pub(super) fn local_date_time(seconds: i64) -> Option<String> {
	let seconds: time_t = seconds;
	let mut local = MaybeUninit::<tm>::uninit();
	let mut output = [0 as c_char; 64];

	// libc applies the host time zone, including the offset at the reset date.
	unsafe {
		if libc::localtime_r(&seconds, local.as_mut_ptr()).is_null() {
			return None;
		}
		if libc::strftime(
			output.as_mut_ptr(),
			output.len(),
			c"%b %d %H:%M".as_ptr(),
			local.as_ptr(),
		) == 0
		{
			return None;
		}

		Some(CStr::from_ptr(output.as_ptr()).to_string_lossy().into_owned())
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

// Keep the framework-specific colors here; the shared fixture checks the quota bands.
fn quota_color(remaining: f32) -> u32 {
	match quota_tone(remaining) {
		"critical" => ERROR,
		"warning" => WB_AMBER,
		_ => WB_BLUE,
	}
}

fn quota_tone(remaining: f32) -> &'static str {
	if remaining > 50. {
		"healthy"
	} else if remaining > 20. {
		"warning"
	} else {
		"critical"
	}
}

/// Match the native menu's local calendar date and 24-hour reset time.
fn reset_time(micros: i64) -> Option<String> {
	local_date_time(micros / 1_000_000)
}

#[cfg(test)]
mod tests {
	use std::time::{Duration, Instant};

	use crate::shell::quota_meter::{self, FILL_DURATION, ResetFill};
	use decodex_protocol::{AccountQuotaStateDto, AccountQuotaWindowDto, EntityRevision};
	#[test]
	fn quota_bands_match_the_menu_bar_contract() {
		let cases: serde_json::Value = serde_json::from_str(include_str!(
			"../../../tests/fixtures/account-quota-presentation.json"
		))
		.unwrap();

		for case in cases.as_array().unwrap() {
			assert_eq!(
				quota_meter::quota_tone(case["remaining"].as_f64().unwrap() as f32),
				case["tone"].as_str().unwrap()
			);
		}
	}

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
			assert_eq!(quota_meter::fill_value(start, Duration::ZERO), start);

			let samples: Vec<_> = (0..=100)
				.map(|step| quota_meter::fill_value(start, Duration::from_millis(step * 10)))
				.collect();

			assert!(samples.windows(2).all(|pair| pair[0] <= pair[1]));
			assert!(samples.iter().all(|value| *value >= start && *value <= 100.));
			assert_eq!(quota_meter::fill_value(start, FILL_DURATION), 100.);
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

		assert_eq!(fill.remaining(quota(0, 11), started, false), Some(36.));
		assert_eq!(fill.remaining(quota(64, 1), started + FILL_DURATION, false), Some(100.));
		assert_eq!(fill.remaining(quota(1, 11), started + FILL_DURATION, false), None);
		assert_eq!(quota_meter::remaining(quota(1, 11)), Some(99.));
		assert_eq!(fill.remaining(quota(64, 1), started, true), Some(100.));
		assert_eq!(fill.remaining(quota(1, 11), started, true), None);

		let not_applicable =
			AccountQuotaWindowDto { result: AccountQuotaStateDto::NotApplicable, ..quota(64, 1) };

		assert_eq!(fill.remaining(not_applicable, started, false), None);
	}
}
