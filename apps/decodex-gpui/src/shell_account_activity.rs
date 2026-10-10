//! Compact saved-activity metrics and daily usage, matching the menu-bar disclosure.
#[cfg(any(test, feature = "visual-capture"))] use std::time::Instant;

use gpui::{
	self, AnyElement, AppContext as _, InteractiveElement as _, IntoElement as _,
	ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};

#[cfg(any(test, feature = "visual-capture"))]
use crate::account_profile::AccountProfileLoadState;
use crate::shell::{ControlTooltip, Shell, WB_BLUE, WB_TEXT_FAINT, WB_TEXT_MUTED, agent_surface};
use decodex_protocol::{AccountDto, AccountProfileDto, AccountProfileResult};
#[cfg(any(test, feature = "visual-capture"))]
use decodex_protocol::{AccountProfileDailyUsageDto, AccountProfileEmailDto, WireText};

#[cfg(any(test, feature = "visual-capture"))]
impl Shell {
	pub(super) fn seed_account_activity(&mut self) {
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
				longest_task_seconds: Some(3_600),
				current_streak_days: Some(7),
				longest_streak_days: Some(12),
				daily_usage: (1..=28)
					.map(|day| AccountProfileDailyUsageDto {
						start_date: WireText::new(format!("2026-09-{day:02}"))
							.expect("fixture date"),
						tokens: ((day * 137) % 83) * 1_000,
					})
					.collect(),
			};
			let mut snapshot = self.account_profile.clone();

			snapshot.selected = Some(account.account_id.clone());
			snapshot.selected_revision = Some(account.account_revision);
			snapshot.load = AccountProfileLoadState::Ready;
			snapshot.result = Some(AccountProfileResult::Current(Box::new(profile)));

			self.account_activity.insert(account.account_id.clone(), (snapshot, Instant::now()));
		}
	}
}

pub(super) fn panel(shell: &Shell, account: &AccountDto) -> AnyElement {
	let snapshot = shell
		.account_activity
		.get(&account.account_id)
		.map(|(snapshot, _)| snapshot)
		.filter(|snapshot| snapshot.selected_revision == Some(account.account_revision));
	let account = &account.account_id;
	let mut content = gpui::div()
		.id(SharedString::from(format!("account-activity-{}", account.as_str())))
		.debug_selector({
			let account = account.clone();

			move || format!("account-activity-{}", account.as_str())
		})
		.w_full()
		.px(gpui::px(14.))
		.py(gpui::px(6.))
		.flex()
		.flex_col()
		.gap(gpui::px(6.))
		.text_size(gpui::px(11.))
		.text_color(gpui::rgb(WB_TEXT_MUTED));
	let profile = match snapshot.and_then(|s| s.result.as_ref()) {
		Some(
			AccountProfileResult::Current(profile) | AccountProfileResult::Cached { profile, .. },
		) => profile,
		_ => {
			return if snapshot.is_none() {
				content.child("Loading activity…").into_any_element()
			} else {
				gpui::div().into_any_element()
			};
		},
	};

	content = content.child(
		gpui::div()
			.flex()
			.flex_wrap()
			.gap_3()
			.children(metrics(profile).into_iter().map(|fact| gpui::div().child(fact))),
	);

	if !profile.daily_usage.is_empty() {
		let peak = profile.daily_usage.iter().map(|day| day.tokens).max().unwrap_or(1).max(1);

		content = content.child(
			gpui::div()
				.id("account-activity-chart")
				.debug_selector(|| "account-activity-chart".into())
				.h(gpui::px(20.))
				.w_full()
				.flex()
				.items_end()
				.gap(gpui::px(1.5))
				.border_b_1()
				.border_color(gpui::rgba(0xffffff16))
				.children(profile.daily_usage.iter().enumerate().map(|(index, day)| {
					let tip = format!("{}: {} tokens", day.start_date.as_str(), day.tokens);

					gpui::div()
						.id(("account-activity-day", index))
						.flex_1()
						.h(gpui::px(if day.tokens == 0 {
							1.
						} else {
							(20. * (day.tokens as f32 / peak as f32).sqrt()).max(2.)
						}))
						.rounded(gpui::px(1.5))
						.bg(gpui::rgb(if day.tokens == 0 { WB_TEXT_FAINT } else { WB_BLUE }))
						.tooltip(move |_, cx| cx.new(|_| ControlTooltip(tip.clone())).into())
				})),
		);
	}

	content.into_any_element()
}

fn metrics(profile: &AccountProfileDto) -> Vec<String> {
	let mut metrics = Vec::new();

	if let Some(value) = profile.lifetime_tokens {
		metrics.push(format!("total {}", agent_surface::compact_tokens(value)));
	}
	if let Some(value) = profile.peak_daily_tokens {
		metrics.push(format!("peak {}", agent_surface::compact_tokens(value)));
	}

	match (profile.current_streak_days, profile.longest_streak_days) {
		(Some(current), Some(longest)) => metrics.push(format!("streak {current}/{longest}d")),
		(Some(days), None) | (None, Some(days)) => metrics.push(format!("streak {days}d")),
		_ => {},
	}

	if let Some(seconds) = profile.longest_task_seconds {
		let (hours, minutes, remainder) = (seconds / 3_600, (seconds % 3_600) / 60, seconds % 60);
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

#[cfg(test)]
mod tests {
	use gpui::TestAppContext;

	use crate::{
		client_lifecycle::ConnectionView,
		shell::{Destination, Shell},
	};

	#[gpui::test]
	fn activity_chart_waits_for_the_current_account_revision(cx: &mut TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

		shell.update(visual, |s, cx| {
			s.visual_accounts_and_health();

			s.selected = Destination::Accounts;

			let id = s.accounts.accounts[0].account_id.clone();

			s.expanded_accounts.insert(id.clone());
			cx.notify();
		});

		visual.update(|w, cx| {
			w.resize(gpui::size(gpui::px(1_440.), gpui::px(1_000.)));
			w.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("account-activity-chart").is_some());

		shell.update(visual, |s, cx| {
			s.accounts.accounts[0].account_revision.0 += 1;

			cx.notify();
		});

		visual.update(|w, cx| {
			w.draw(cx).clear(cx);
		});

		assert!(
			visual.debug_bounds("account-activity-chart").is_none(),
			"old-revision usage must not be rendered for the current account"
		);
		assert!(
			visual.debug_bounds("account-activity-70000000-0000-4000-8000-000000000001").is_some(),
			"retain the loading panel while current activity is unavailable"
		);

		shell.update(visual, |s, cx| {
			s.seed_account_activity();
			cx.notify();
		});
		visual.update(|w, cx| {
			w.draw(cx).clear(cx);
		});

		assert!(
			visual.debug_bounds("account-activity-chart").is_some(),
			"current-revision activity can be shown again"
		);
	}
}
