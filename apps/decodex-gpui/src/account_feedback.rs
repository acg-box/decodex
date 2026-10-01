//! Account warnings occupy an icon; their explanation opens only on activation.
use super::*;

#[derive(IntoElement)]
pub(super) struct AccountFeedback {
	pub id: SharedString,
	pub selector: String,
	pub text: String,
	pub color: u32,
}
impl gpui::RenderOnce for AccountFeedback {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(self.id.clone(), cx, |_, _| {
			(false, gpui::Bounds::<gpui::Pixels>::default())
		});
		let (open, bounds) = *state.read(cx);
		let toggle = state.clone();
		let measure = state.clone();
		let popup_selector = format!("{}-popover", self.selector);
		let popup = div()
			.id("account-feedback-explanation")
			.occlude()
			.w(px(240.))
			.p_3()
			.rounded(px(10.))
			.bg(rgba(ui_theme::SURFACE_OVERLAY_MATERIAL))
			.border_1()
			.border_color(rgba(0xffffff14))
			.text_size(px(12.))
			.text_color(rgb(self.color))
			.cursor_default()
			.debug_selector(move || popup_selector.clone())
			.on_click(|_, _, cx| cx.stop_propagation())
			.on_mouse_down_out(move |event, _, cx| {
				if !bounds.contains(&event.position) {
					state.update(cx, |s, cx| {
						s.0 = false;

						cx.notify();
					});
				}
			})
			.child(self.text.clone());

		div()
			.id(self.id)
			.debug_selector(move || self.selector.clone())
			.role(Role::Button)
			.aria_label(self.text)
			.aria_expanded(open)
			.tab_index(0)
			.size(px(24.))
			.relative()
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(px(6.))
			.cursor_pointer()
			.hover(|s| s.bg(rgba(ui_theme::HOVER_FILL)))
			.on_click(move |_, _, cx| {
				cx.stop_propagation();

				toggle.update(cx, |s, cx| {
					s.0 = !s.0;

					cx.notify();
				});
			})
			.child(workspace_symbols::icon(if self.color == ui_theme::ERROR {
				workspace_symbols::Symbol::AccountWarning
			} else {
				workspace_symbols::Symbol::AccountWarningAmber
			}))
			.child(
				gpui::canvas(
					move |bounds, _, cx| {
						measure.update(cx, |s, _| s.1 = bounds);
					},
					|_, _, _, _| {},
				)
				.absolute()
				.inset_0(),
			)
			.when(open, |d| {
				d.child(
					gpui::deferred(
						gpui::anchored()
							.anchor(gpui::Anchor::TopRight)
							.position(bounds.bottom_right())
							.offset(gpui::point(px(0.), px(4.)))
							.snap_to_window_with_margin(px(8.))
							.child(popup),
					)
					.with_priority(4),
				)
			})
	}
}

pub(super) fn for_account(shell: &Shell, account: &AccountDto) -> Option<(String, u32)> {
	if account_needs_login(account) {
		return Some(("Sign in again to use this account, or log out.".into(), ui_theme::ERROR));
	}

	if let Some(result) =
		shell.account_activity.get(&account.account_id).and_then(|(s, _)| s.result.as_ref())
	{
		let error = match result {
			AccountProfileResult::Cached { refresh_error, .. } => Some(*refresh_error),
			AccountProfileResult::Unavailable { error, .. } => Some(*error),
			_ => None,
		};

		if let Some(error) = error {
			return Some(if crate::account_profile::requires_login(error) {
				("Sign in again to update your activity.".into(), ui_theme::ERROR)
			} else {
				("Your activity couldn’t be updated. Try again later.".into(), ui_theme::AMBER)
			});
		}
	}

	match account.lifecycle_readiness {
		AccountLifecycleReadinessDto::StoreUnavailable =>
			Some(("Your sign-in information is temporarily unavailable.".into(), ui_theme::AMBER)),
		AccountLifecycleReadinessDto::StoreMismatch
		| AccountLifecycleReadinessDto::ProviderMismatch =>
			Some(("Your sign-in information has changed. Sign in again.".into(), ui_theme::ERROR)),
		AccountLifecycleReadinessDto::OperationUnsettled =>
			Some(("An account update is still in progress.".into(), ui_theme::AMBER)),
		_ => match account.observed_state {
			AccountObservedStateDto::PluginUnready =>
				Some(("Update the provider to use this account.".into(), ui_theme::AMBER)),
			AccountObservedStateDto::Unknown =>
				Some(("The account status is temporarily unavailable.".into(), ui_theme::AMBER)),
			AccountObservedStateDto::Unavailable =>
				Some(("This account is unavailable.".into(), ui_theme::ERROR)),
			_ => None,
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::client_lifecycle::ConnectionView;
	use gpui::{Modifiers, TestAppContext, point, size};

	#[gpui::test]
	fn warnings_open_on_click_and_close_outside_without_changing_account_state(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

		shell.update(visual, |s, cx| {
			s.visual_accounts_and_health();

			s.accounts.accounts[0].observed_state = AccountObservedStateDto::AuthFailed;
			s.accounts.rejection = Some(AccountCommandRejectionDto::CodexIsRunning);
			s.selected = Destination::Accounts;

			cx.notify();
		});

		visual.update(|w, cx| {
			w.resize(size(px(1_440.), px(1_000.)));
			w.draw(cx).clear();
		});

		for (selector, popup) in [
			("account-login-warning-0", "account-login-warning-0-popover"),
			("account-global-warning", "account-global-warning-popover"),
		] {
			assert!(visual.debug_bounds(popup).is_none());

			let height = visual.debug_bounds("account-card-0").unwrap().size.height;
			let trigger = visual.debug_bounds(selector).unwrap().center();

			visual.simulate_click(trigger, Modifiers::default());
			visual.update(|w, cx| {
				w.draw(cx).clear();
			});

			let bounds =
				visual.debug_bounds(popup).expect("A warning must open on the first click");

			assert!(bounds.left() >= px(0.) && bounds.right() <= px(1_440.));
			assert_eq!(visual.debug_bounds("account-card-0").unwrap().size.height, height);

			shell.read_with(visual, |s, _| assert!(s.expanded_accounts.is_empty()));
			visual.simulate_click(point(px(5.), px(5.)), Modifiers::default());
			visual.update(|w, cx| {
				w.draw(cx).clear();
			});

			assert!(visual.debug_bounds(popup).is_none());

			shell.read_with(visual, |s, _| {
				assert_eq!(s.accounts.rejection, Some(AccountCommandRejectionDto::CodexIsRunning))
			});
		}
	}
}
