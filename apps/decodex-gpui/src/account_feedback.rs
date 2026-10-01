//! Account warnings occupy an icon; their explanation opens only on activation.
use gpui::{
	self, Anchor, App, Bounds, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
	RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
	prelude::FluentBuilder as _,
};

use crate::{
	account_profile,
	shell::{
		self, Shell,
		workspace_symbols::{self, Symbol},
	},
	ui_theme::{AMBER, ERROR, HOVER_FILL, SURFACE_OVERLAY_MATERIAL},
};
use decodex_protocol::{
	AccountDto, AccountLifecycleReadinessDto, AccountObservedStateDto, AccountProfileResult,
};

#[derive(IntoElement)]
pub(super) struct AccountFeedback {
	pub id: SharedString,
	pub selector: String,
	pub text: String,
	pub color: u32,
}
impl RenderOnce for AccountFeedback {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window
			.use_keyed_state(self.id.clone(), cx, |_, _| (false, Bounds::<Pixels>::default()));
		let (open, bounds) = *state.read(cx);
		let dismiss = state.clone();
		let toggle = state.clone();
		let measure = state.clone();
		let popup_selector = format!("{}-popover", self.selector);
		let popup = gpui::div()
			.id("account-feedback-explanation")
			.occlude()
			.w(gpui::px(240.))
			.p_3()
			.rounded(gpui::px(10.))
			.bg(gpui::rgba(SURFACE_OVERLAY_MATERIAL))
			.border_1()
			.border_color(gpui::rgba(0xffffff14))
			.text_size(gpui::px(12.))
			.text_color(gpui::rgb(self.color))
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

		gpui::div()
			.id(self.id)
			.debug_selector(move || self.selector.clone())
			.role(Role::Button)
			.aria_label(self.text)
			.aria_expanded(open)
			.tab_index(0)
			.size(gpui::px(24.))
			.relative()
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(6.))
			.cursor_pointer()
			.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
			.on_key_down(move |event, _, cx| {
				if open && event.keystroke.key == "escape" {
					dismiss.update(cx, |s, cx| {
						s.0 = false;

						cx.notify();
					});

					cx.stop_propagation();
				}
			})
			.on_click(move |_, _, cx| {
				cx.stop_propagation();

				toggle.update(cx, |s, cx| {
					s.0 = !s.0;

					cx.notify();
				});
			})
			.child(workspace_symbols::icon(if self.color == ERROR {
				Symbol::AccountWarning
			} else {
				Symbol::AccountWarningAmber
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
							.anchor(Anchor::TopRight)
							.position(bounds.bottom_right())
							.offset(gpui::point(gpui::px(0.), gpui::px(4.)))
							.snap_to_window_with_margin(gpui::px(8.))
							.child(popup),
					)
					.with_priority(4),
				)
			})
	}
}

pub(super) fn for_account(shell: &Shell, account: &AccountDto) -> Option<(String, u32)> {
	if shell::account_needs_login(account) {
		return Some(("Sign in again to use this account, or log out.".into(), ERROR));
	}

	if let Some(result) = shell
		.account_activity
		.get(&account.account_id)
		.filter(|(snapshot, _)| snapshot.selected_revision == Some(account.account_revision))
		.and_then(|(snapshot, _)| snapshot.result.as_ref())
	{
		let error = match result {
			AccountProfileResult::Cached { refresh_error, .. } => Some(*refresh_error),
			AccountProfileResult::Unavailable { error, .. } => Some(*error),
			_ => None,
		};

		if let Some(error) = error {
			return Some(if account_profile::requires_login(error) {
				("Sign in again to update your activity.".into(), ERROR)
			} else {
				("Your activity couldn’t be updated. Try again later.".into(), AMBER)
			});
		}
	}

	match account.lifecycle_readiness {
		AccountLifecycleReadinessDto::StoreUnavailable =>
			Some(("Your sign-in information is temporarily unavailable.".into(), AMBER)),
		AccountLifecycleReadinessDto::StoreMismatch
		| AccountLifecycleReadinessDto::ProviderMismatch =>
			Some(("Your sign-in information has changed. Sign in again.".into(), ERROR)),
		AccountLifecycleReadinessDto::OperationUnsettled =>
			Some(("An account update is still in progress.".into(), AMBER)),
		_ => match account.observed_state {
			AccountObservedStateDto::PluginUnready =>
				Some(("Update the provider to use this account.".into(), AMBER)),
			AccountObservedStateDto::Unknown =>
				Some(("The account status is temporarily unavailable.".into(), AMBER)),
			AccountObservedStateDto::Unavailable =>
				Some(("This account is unavailable.".into(), ERROR)),
			_ => None,
		},
	}
}

#[cfg(test)]
mod tests {
	use gpui::{
		self, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, PlatformInput, TestAppContext,
	};

	use crate::{
		client_lifecycle::ConnectionView,
		shell::{Destination, Shell, account_feedback},
		ui_theme::ERROR,
	};
	use decodex_protocol::{
		AccountCommandRejectionDto, AccountObservedStateDto, AccountProfileEmailDto,
		AccountProfileErrorDto, AccountProfileResult, EntityRevision,
	};

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
			w.resize(gpui::size(gpui::px(1_440.), gpui::px(1_000.)));
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

			assert!(bounds.left() >= gpui::px(0.) && bounds.right() <= gpui::px(1_440.));
			assert_eq!(visual.debug_bounds("account-card-0").unwrap().size.height, height);

			shell.read_with(visual, |s, _| assert!(s.expanded_accounts.is_empty()));

			for (key, should_open) in
				[("escape", false), ("enter", true), ("space", false), ("space", true)]
			{
				visual.update(|w, cx| {
					let keystroke = Keystroke::parse(key).unwrap();

					w.dispatch_event(
						PlatformInput::KeyDown(KeyDownEvent {
							keystroke: keystroke.clone(),
							is_held: false,
							prefer_character_input: false,
						}),
						cx,
					);
					w.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
				});

				visual.update(|w, cx| {
					w.draw(cx).clear();
				});

				assert_eq!(
					visual.debug_bounds(popup).is_some(),
					should_open,
					"warning state after {key}"
				);

				shell.read_with(visual, |s, _| assert!(s.expanded_accounts.is_empty()));
			}

			visual.simulate_click(gpui::point(gpui::px(5.), gpui::px(5.)), Modifiers::default());
			visual.update(|w, cx| {
				w.draw(cx).clear();
			});

			assert!(visual.debug_bounds(popup).is_none());

			shell.read_with(visual, |s, _| {
				assert_eq!(s.accounts.rejection, Some(AccountCommandRejectionDto::CodexIsRunning))
			});
		}
	}

	#[gpui::test]
	fn activity_warning_requires_the_current_account_revision(cx: &mut TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

		shell.update(visual, |s, _| {
			s.visual_accounts_and_health();

			let mut account = s.accounts.accounts[0].clone();
			let (snapshot, _) = s.account_activity.get_mut(&account.account_id).unwrap();

			snapshot.result = Some(AccountProfileResult::Unavailable {
				error: AccountProfileErrorDto::Unauthorized,
				email: AccountProfileEmailDto::Redacted,
				plan_type: None,
			});

			assert_eq!(
				account_feedback::for_account(s, &account),
				Some(("Sign in again to update your activity.".into(), ERROR))
			);

			account.account_revision = EntityRevision(account.account_revision.0 + 1);

			assert_eq!(
				account_feedback::for_account(s, &account),
				None,
				"a previous revision cannot require the current account to sign in again"
			);

			account.observed_state = AccountObservedStateDto::AuthFailed;

			assert_eq!(
				account_feedback::for_account(s, &account),
				Some(("Sign in again to use this account, or log out.".into(), ERROR))
			);
		});
	}
}
