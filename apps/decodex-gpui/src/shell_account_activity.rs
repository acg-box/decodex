//! Compact saved-activity metrics and daily usage, matching the menu-bar disclosure.
use super::{
	AccountProfileResult, ControlTooltip, Shell, StatusTooltip, WB_BLUE, WB_TEXT_FAINT,
	WB_TEXT_MUTED,
};
use gpui::{AnyElement, div, prelude::*, px, rgb, rgba};
pub(super) fn panel(shell: &Shell, account: &decodex_protocol::EntityId) -> AnyElement {
	let snapshot = shell.account_activity.get(account).map(|(snapshot, _)| snapshot);
	let mut content = div()
		.id(gpui::SharedString::from(format!("account-activity-{}", account.as_str())))
		.debug_selector({
			let account = account.clone();
			move || format!("account-activity-{}", account.as_str())
		})
		.w_full()
		.px(px(14.))
		.py(px(6.))
		.flex()
		.flex_col()
		.gap(px(6.))
		.text_size(px(11.))
		.text_color(rgb(WB_TEXT_MUTED));
	let profile = match snapshot.and_then(|s| s.result.as_ref()) {
		Some(
			AccountProfileResult::Current(profile) | AccountProfileResult::Cached { profile, .. },
		) => profile,
		_ =>
			return content
				.child(if snapshot.is_none() {
					"Loading activity…"
				} else {
					"Activity unavailable"
				})
				.into_any_element(),
	};
	content = content.child(
		div()
			.flex()
			.flex_wrap()
			.gap_3()
			.children(metrics(profile).into_iter().map(|fact| div().child(fact))),
	);
	if !profile.daily_usage.is_empty() {
		let peak = profile.daily_usage.iter().map(|day| day.tokens).max().unwrap_or(1).max(1);
		content = content.child(
			div()
				.id("account-activity-chart")
				.debug_selector(|| "account-activity-chart".into())
				.h(px(20.))
				.w_full()
				.flex()
				.items_end()
				.gap(px(1.5))
				.border_b_1()
				.border_color(rgba(0xffffff16))
				.children(profile.daily_usage.iter().enumerate().map(|(index, day)| {
					let tip = format!("{}: {} tokens", day.start_date.as_str(), day.tokens);
					div()
						.id(("account-activity-day", index))
						.flex_1()
						.h(px(if day.tokens == 0 {
							1.
						} else {
							(20. * (day.tokens as f32 / peak as f32).sqrt()).max(2.)
						}))
						.rounded(px(1.5))
						.bg(rgb(if day.tokens == 0 { WB_TEXT_FAINT } else { WB_BLUE }))
						.tooltip(move |_, cx| cx.new(|_| ControlTooltip(tip.clone())).into())
				})),
		);
	}
	if let Some(AccountProfileResult::Cached { refresh_error, .. }) =
		snapshot.and_then(|s| s.result.as_ref())
	{
		let requires_login = crate::account_profile::requires_login(*refresh_error);
		let color = if requires_login { crate::ui_theme::ERROR } else { crate::ui_theme::AMBER };
		let message = if requires_login { "Sign in again" } else { "Couldn’t update activity" };
		content = content.child(
			div()
				.id("account-activity-status")
				.text_color(rgb(color))
				.truncate()
				.child(message)
				.tooltip(move |_, cx| {
					cx.new(|_| StatusTooltip {
						text: if requires_login {
							"Sign in again to update your activity."
						} else {
							"Your activity couldn’t be updated. Try again later."
						}
						.into(),
						color,
					})
					.into()
				}),
		);
	}
	content.into_any_element()
}

#[cfg(any(test, feature = "visual-capture"))]
impl Shell {
	pub(super) fn seed_account_activity(&mut self) {
		use decodex_protocol::{
			AccountProfileDailyUsageDto, AccountProfileDto, AccountProfileEmailDto, WireText,
		};
		for account in &self.accounts.accounts {
			let profile = AccountProfileDto {
				account_id: account.account_id.clone(),
				account_revision: account.account_revision,
				observed_at_unix_micros: 0,
				email: AccountProfileEmailDto::Redacted,
				plan_type: Some(WireText::new("pro").expect("fixture plan")),
				display_name: None,
				username: None,
				lifetime_tokens: Some(1_240_000),
				peak_daily_tokens: Some(82_000),
				longest_task_seconds: Some(3600),
				current_streak_days: Some(7),
				longest_streak_days: Some(12),
				daily_usage: (1..=28)
					.map(|day| AccountProfileDailyUsageDto {
						start_date: WireText::new(format!("2026-09-{day:02}"))
							.expect("fixture date"),
						tokens: ((day * 137) % 83) * 1000,
					})
					.collect(),
			};
			let mut snapshot = self.account_profile.clone();
			snapshot.selected = Some(account.account_id.clone());
			snapshot.selected_revision = Some(account.account_revision);
			snapshot.load = crate::account_profile::AccountProfileLoadState::Ready;
			snapshot.result = Some(AccountProfileResult::Current(Box::new(profile)));
			self.account_activity
				.insert(account.account_id.clone(), (snapshot, std::time::Instant::now()));
		}
	}
}

fn metrics(profile: &decodex_protocol::AccountProfileDto) -> Vec<String> {
	let mut metrics = Vec::new();
	if let Some(value) = profile.lifetime_tokens {
		metrics.push(format!("total {}", super::agent_surface::compact_tokens(value)));
	}
	if let Some(value) = profile.peak_daily_tokens {
		metrics.push(format!("peak {}", super::agent_surface::compact_tokens(value)));
	}
	match (profile.current_streak_days, profile.longest_streak_days) {
		(Some(current), Some(longest)) => metrics.push(format!("streak {current}/{longest}d")),
		(Some(days), None) | (None, Some(days)) => metrics.push(format!("streak {days}d")),
		_ => {},
	}
	if let Some(seconds) = profile.longest_task_seconds {
		let (hours, minutes, remainder) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
		let value = if hours > 0 {
			if minutes > 0 { format!("{hours}h {minutes}m") } else { format!("{hours}h") }
		} else if minutes > 0 {
			if remainder > 0 { format!("{minutes}m {remainder}s") } else { format!("{minutes}m") }
		} else {
			format!("{seconds}s")
		};
		metrics.push(format!("task {value}"));
	}
	metrics
}
