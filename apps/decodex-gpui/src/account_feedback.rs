//! Account feedback stays beside the affected account and shares the form notice style.
use gpui::{
	self, App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
	Window,
};

use crate::{
	account_profile,
	shell::{self, Shell},
	ui_feedback::{self, Feedback, Tone},
	ui_theme::{ERROR, INFO, SUCCESS, TEXT_MUTED, WARNING},
};
use decodex_protocol::{
	AccountCommandRejectionDto, AccountDto, AccountLifecycleReadinessDto, AccountObservedStateDto,
	AccountProfileResult,
};

#[derive(IntoElement)]
pub(super) struct AccountFeedback {
	pub id: SharedString,
	pub selector: String,
	pub text: String,
	pub color: u32,
}
impl RenderOnce for AccountFeedback {
	fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
		let mut feedback = Feedback::neutral(self.text);

		feedback.tone = match self.color {
			ERROR => Tone::Error,
			INFO => Tone::Info,
			SUCCESS => Tone::Success,
			TEXT_MUTED => Tone::Neutral,
			_ => Tone::Warning,
		};

		gpui::div().id(self.id).children(ui_feedback::inline_feedback(self.selector, "", &feedback))
	}
}

pub(super) fn for_account(shell: &Shell, account: &AccountDto) -> Option<(String, u32)> {
	if account.lifecycle_readiness == AccountLifecycleReadinessDto::UnsupportedPlan {
		return Some((
			shell::account_rejection_label(AccountCommandRejectionDto::UnsupportedPlan).into(),
			WARNING,
		));
	}
	if shell::account_needs_login(account) {
		return Some(("Sign in again.".into(), ERROR));
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
				("Sign in again to update activity.".into(), ERROR)
			} else {
				("Could not update activity.".into(), WARNING)
			});
		}
	}

	match account.lifecycle_readiness {
		AccountLifecycleReadinessDto::StoreUnavailable =>
			Some(("Sign-in storage unavailable. Open Diagnostics.".into(), WARNING)),
		AccountLifecycleReadinessDto::StoreMismatch =>
			Some(("Saved sign-in changed. Sign in again.".into(), ERROR)),
		AccountLifecycleReadinessDto::ProviderMismatch =>
			Some(("Saved sign-in belongs to a different account. Sign in again.".into(), ERROR)),
		AccountLifecycleReadinessDto::OperationUnsettled =>
			Some(("Updating account…".into(), INFO)),
		_ => match account.observed_state {
			AccountObservedStateDto::PluginUnready =>
				Some(("Account provider unavailable. Open Diagnostics.".into(), WARNING)),
			AccountObservedStateDto::Unknown =>
				Some(("Account status unavailable.".into(), WARNING)),
			AccountObservedStateDto::Unavailable => Some(("Account unavailable.".into(), ERROR)),
			_ => None,
		},
	}
}

#[cfg(test)]
mod tests {
	use gpui::{self, TestAppContext};

	use crate::{
		client_lifecycle::ConnectionView,
		shell::{Destination, Shell, account_feedback, account_feedback::shell},
		ui_theme::{ERROR, WARNING},
	};
	use decodex_protocol::{
		AccountCommandRejectionDto, AccountLifecycleReadinessDto, AccountObservedStateDto,
		AccountProfileEmailDto, AccountProfileErrorDto, AccountProfileResult, EntityRevision,
	};

	#[gpui::test]
	fn account_problems_are_visible_beside_their_owner_without_opening_a_popup(
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
			w.draw(cx).clear(cx);
		});

		let warning =
			visual.debug_bounds("account-login-warning-0").expect("visible account explanation");
		let row = visual.debug_bounds("account-card-0").expect("account card");

		assert!(warning.top() >= row.top() && warning.bottom() <= row.bottom());
		assert!(visual.debug_bounds("account-global-warning").is_some());
		assert!(visual.debug_bounds("account-login-warning-0-popover").is_none());

		shell.read_with(visual, |s, _| {
			assert!(s.expanded_accounts.is_empty());
			assert_eq!(s.accounts.rejection, Some(AccountCommandRejectionDto::CodexIsRunning));
		});
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
				Some(("Sign in again to update activity.".into(), ERROR))
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
				Some(("Sign in again.".into(), ERROR))
			);
		});
	}

	#[gpui::test]
	fn unsupported_plan_hides_quota_and_offers_reauthentication(cx: &mut TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

		shell.update(visual, |s, cx| {
			s.visual_accounts_and_health();

			s.accounts.accounts[0].lifecycle_readiness =
				AccountLifecycleReadinessDto::UnsupportedPlan;
			s.selected = Destination::Accounts;

			let account = &s.accounts.accounts[0];

			assert_eq!(shell::account_readiness_status(account), "Plan not supported");
			assert_eq!(
				account_feedback::for_account(s, account),
				Some(("Use a Plus, Pro, Pro (More), Pro (Max), or Team account.".into(), WARNING,))
			);

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("account-quota-0").is_none());
		assert!(visual.debug_bounds("account-login-0").is_some());
	}

	#[gpui::test]
	fn command_failure_is_scoped_to_the_account_that_requested_it(cx: &mut TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

		shell.update(visual, |s, cx| {
			s.visual_accounts_and_health();

			for account in &mut s.accounts.accounts {
				account.observed_state = AccountObservedStateDto::Available;
				account.lifecycle_readiness = AccountLifecycleReadinessDto::Ready;
			}

			s.accounts.rejection = None;
			s.account_status = Some("This account could not be changed.".into());
			s.account_status_target = Some(s.accounts.accounts[0].account_id.clone());
			s.selected = Destination::Accounts;

			cx.notify();
		});

		visual.update(|w, cx| {
			w.resize(gpui::size(gpui::px(1_440.), gpui::px(1_000.)));
			w.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("account-warning-0").is_some());
		assert!(visual.debug_bounds("account-warning-1").is_none());
		assert!(visual.debug_bounds("account-global-warning").is_none());
	}

	#[gpui::test]
	fn failed_sign_in_remains_visible_at_its_controls(cx: &mut TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

		shell.update(visual, |s, cx| {
			s.visual_accounts_and_health();

			s.selected = Destination::Accounts;
			s.account_login_status = Some(decodex_protocol::AccountLoginStatus {
				session_id: decodex_protocol::EntityId::new("80000000-0000-4000-8000-000000000001")
					.unwrap(),
				state: decodex_protocol::AccountLoginState::Failed,
				prompt: None,
				authorization_url: None,
				failure: Some(decodex_protocol::AccountLoginFailure::LoginTimedOut),
				resolved_account_id: None,
			});

			cx.notify();
		});

		visual.update(|w, cx| {
			w.resize(gpui::size(gpui::px(1_440.), gpui::px(1_000.)));
			w.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("account-login-feedback").is_some());

		shell.read_with(visual, |s, _| {
			let feedback = shell::account_login_feedback(s).unwrap();

			assert_eq!(feedback.tone, crate::ui_feedback::Tone::Error);
			assert!(feedback.contains("expired"));
			assert!(!feedback.contains("LoginTimedOut"));
		});
	}
}
