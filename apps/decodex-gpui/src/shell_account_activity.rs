//! Account activity uses the menu-bar label/value hierarchy within the Settings card.
#[cfg(any(test, feature = "visual-capture"))] use std::time::Instant;

use gpui::{
	self, AnyElement, AppContext as _, InteractiveElement as _, IntoElement as _,
	ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};

#[cfg(any(test, feature = "visual-capture"))]
use crate::account_profile::AccountProfileLoadState;
use crate::{
	shell::{ControlTooltip, Shell, WB_BLUE, WB_TEXT_FAINT, WB_TEXT_MUTED, agent_surface},
	ui_feedback::{self, Feedback},
	ui_theme,
};
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
		.flex()
		.flex_col()
		.gap(gpui::px(12.))
		.text_size(gpui::px(ui_theme::SETTINGS_HELP_SIZE))
		.line_height(gpui::px(18.))
		.text_color(ui_theme::rgb(WB_TEXT_MUTED));
	let profile = match snapshot.and_then(|s| s.result.as_ref()) {
		Some(
			AccountProfileResult::Current(profile) | AccountProfileResult::Cached { profile, .. },
		) => profile,
		_ => {
			return if snapshot.is_none() {
				content
					.children(ui_feedback::inline_feedback(
						format!("account-activity-loading-{}", account.as_str()),
						"",
						&Feedback::info("Loading activity…"),
					))
					.into_any_element()
			} else {
				gpui::div().into_any_element()
			};
		},
	};

	content = content.child(
		gpui::div().w_full().grid().grid_cols(2).gap_x(gpui::px(32.)).gap_y(gpui::px(8.)).children(
			metrics(profile).into_iter().enumerate().map(|(index, (label, value))| {
				gpui::div()
					.id(("account-activity-metric", index))
					.debug_selector({
						let account = account.clone();
						move || format!("account-activity-metric-{}-{index}", account.as_str())
					})
					.role(gpui::Role::Label)
					.aria_label(format!("{label}: {value}"))
					.flex()
					.min_w_0()
					.items_center()
					.justify_between()
					.gap_3()
					.whitespace_nowrap()
					.child(label)
					.child(
						gpui::div()
							.flex_none()
							.text_size(gpui::px(ui_theme::SETTINGS_LABEL_SIZE))
							.font_weight(gpui::FontWeight::MEDIUM)
							.text_color(ui_theme::rgb(ui_theme::TEXT))
							.child(value),
					)
			}),
		),
	);

	if !profile.daily_usage.is_empty() {
		let peak = profile.daily_usage.iter().map(|day| day.tokens).max().unwrap_or(1).max(1);

		content = content.child(
			gpui::div()
				.id("account-activity-chart")
				.debug_selector(|| "account-activity-chart".into())
				.h(gpui::px(24.))
				.w_full()
				.flex()
				.items_end()
				.gap(gpui::px(1.5))
				.border_b_1()
				.border_color(ui_theme::rgba(0xffffff16))
				.children(profile.daily_usage.iter().enumerate().map(|(index, day)| {
					let tip = format!("{}: {} tokens", day.start_date.as_str(), day.tokens);

					gpui::div()
						.id(("account-activity-day", index))
						.flex_1()
						.h(gpui::px(if day.tokens == 0 {
							1.
						} else {
							(24. * (day.tokens as f32 / peak as f32).sqrt()).max(2.)
						}))
						.rounded(gpui::px(1.5))
						.bg(ui_theme::rgb(if day.tokens == 0 { WB_TEXT_FAINT } else { WB_BLUE }))
						.tooltip(move |_, cx| cx.new(|_| ControlTooltip(tip.clone())).into())
				})),
		);
	}

	content.into_any_element()
}

fn metrics(profile: &AccountProfileDto) -> Vec<(&'static str, String)> {
	let mut metrics = Vec::new();

	if let Some(value) = profile.lifetime_tokens {
		metrics.push(("Total tokens", agent_surface::compact_tokens(value)));
	}
	if let Some(value) = profile.peak_daily_tokens {
		metrics.push(("Daily peak", agent_surface::compact_tokens(value)));
	}
	let streak = match (profile.current_streak_days, profile.longest_streak_days) {
		(Some(current), Some(longest)) => Some(format!("{current}d (best {longest}d)")),
		(Some(current), None) => Some(format!("{current}d")),
		(None, Some(longest)) => Some(format!("Best {longest}d")),
		(None, None) => None,
	};
	if let Some(value) = streak {
		metrics.push(("Streak", value));
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

		metrics.push(("Longest task", value));
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

		for (width, height) in [(1_440., 1_000.), (920., 620.)] {
			visual.update(|w, cx| {
				w.resize(gpui::size(gpui::px(width), gpui::px(height)));
				w.draw(cx).clear(cx);
			});
			let cells = [
				"account-activity-metric-70000000-0000-4000-8000-000000000001-0",
				"account-activity-metric-70000000-0000-4000-8000-000000000001-1",
				"account-activity-metric-70000000-0000-4000-8000-000000000001-2",
				"account-activity-metric-70000000-0000-4000-8000-000000000001-3",
			]
			.map(|selector| visual.debug_bounds(selector).unwrap());
			assert_eq!(cells[0].top(), cells[1].top());
			assert_eq!(cells[2].top(), cells[3].top());
			assert!(cells[0].right() < cells[1].left(), "metric columns have reading space");
			assert!(cells[0].bottom() < cells[2].top(), "metrics must not wrap into each other");
			assert!(
				cells[3].bottom() < visual.debug_bounds("account-activity-chart").unwrap().top()
			);
		}

		assert!(visual.debug_bounds("account-activity-chart").is_some());

		shell.update(visual, |s, cx| {
			s.accounts.accounts[0].account_revision.0 += 1;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

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
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(
			visual.debug_bounds("account-activity-chart").is_some(),
			"current-revision activity can be shown again"
		);
		let identity = visual.debug_bounds("account-identity-0").unwrap();
		visual.simulate_click(identity.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		shell.read_with(visual, |s, _| assert!(s.expanded_accounts.is_empty()));
		visual.simulate_click(identity.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("account-activity-chart").is_some());
	}
}
