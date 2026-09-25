//! Explicit account recovery interactions with daemon-owned notification receipts.
use super::{Shell, account_login_button};
use crate::account_profile::RecoveryActionTicket;
use decodex_protocol::{
	AccountClient, AccountRecoveryAction as A, AccountRecoveryDestination as D,
	AccountRecoveryNudgeOperation as Operation, AccountRecoveryNudgeResult as NudgeResult,
	AccountRecoveryNudgeStatus as Status, AccountRecoveryPreparation as Preparation,
	AccountRecoveryResult, AccountRecoveryState, EntityId, EntityRevision, IdempotencyKey,
};
use gpui::{AnyElement, Context, div, prelude::*};
use std::sync::mpsc::{self, Receiver};
#[path = "recovery_nudge_policy.rs"] mod nudge_policy;

#[derive(Default)]
pub(super) struct RecoveryActions {
	updates: Option<Receiver<Update>>,
	message: Option<(EntityId, EntityRevision, String)>,
	history_scope: Option<(EntityId, EntityRevision)>,
	history: Vec<Notice>,
	history_error: bool,
	history_generation: u64,
}
struct Notice {
	account_id: EntityId,
	action: A,
	operation_key: IdempotencyKey,
	outcome: Status,
}
impl From<Operation> for Notice {
	fn from(operation: Operation) -> Self {
		Self {
			account_id: operation.account_id,
			action: operation.action,
			operation_key: operation.operation_key,
			outcome: operation.outcome,
		}
	}
}

enum Update {
	Prepared {
		ticket: RecoveryActionTicket,
		prepared: Option<Preparation>,
		prior: Option<NudgeResult>,
		acknowledged: Option<IdempotencyKey>,
	},
	Sent {
		ticket: RecoveryActionTicket,
		key: IdempotencyKey,
		result: NudgeResult,
		confirmed: Option<Status>,
	},
	History {
		account: EntityId,
		revision: EntityRevision,
		results: Vec<NudgeResult>,
	},
}

impl Shell {
	fn spawn_recovery_work(
		&mut self,
		work: impl std::future::Future<Output = Update> + Send + 'static,
		cx: &mut Context<Self>,
	) {
		let (sender, receiver) = mpsc::channel();
		self.recovery_actions.updates = Some(receiver);
		cx.background_executor()
			.spawn(async move {
				if let Ok(runtime) =
					tokio::runtime::Builder::new_current_thread().enable_all().build()
				{
					let _ = sender.send(runtime.block_on(work));
				}
			})
			.detach();
		cx.notify();
	}

	fn recovery_message(&mut self, source: &AccountRecoveryResult, text: &str) {
		self.recovery_actions.message =
			Some((source.account_id.clone(), source.account_revision, text.into()));
	}

	fn start_recovery_action(
		&mut self,
		source: &AccountRecoveryResult,
		action: A,
		acknowledged: Option<IdempotencyKey>,
		cx: &mut Context<Self>,
	) {
		if self.recovery_actions.updates.is_some() {
			return;
		}
		let Some(profile) = self.reset_cards.profile.clone() else {
			return;
		};
		let Some(ticket) = self.account_profile_controller.begin_recovery_action(source, action)
		else {
			return;
		};
		self.recovery_message(source, "Checking this action…");
		self.spawn_recovery_work(
			async move {
				let client = AccountClient::new(profile);
				let prepared =
					client.prepare_recovery(ticket.source.clone(), ticket.action).await.ok();
				let prior = if matches!(
					&prepared,
					Some(Preparation::Ready {
						destination: D::RequestCredits | D::RequestUsageIncrease,
						..
					})
				) {
					Some(
						client
							.recovery_nudge_status(
								ticket.source.account_id.clone(),
								ticket.action,
								None,
							)
							.await
							.unwrap_or(NudgeResult::Unavailable),
					)
				} else {
					None
				};
				Update::Prepared { ticket, prepared, prior, acknowledged }
			},
			cx,
		);
	}

	fn read_nudge_history(
		&mut self,
		account: EntityId,
		revision: EntityRevision,
		cx: &mut Context<Self>,
	) {
		if self.recovery_actions.updates.is_some() {
			return;
		}
		let Some(profile) = self.reset_cards.profile.clone() else {
			return;
		};
		self.recovery_actions.history_scope = Some((account.clone(), revision));
		self.recovery_actions.history_generation = self.account_profile.notification_generation;
		self.spawn_recovery_work(
			async move {
				let client = AccountClient::new(profile);
				let mut results = Vec::new();
				for action in [A::NotifyOwner, A::RequestIncrease] {
					results.push(
						client
							.recovery_nudge_status(account.clone(), action, None)
							.await
							.unwrap_or(NudgeResult::Unavailable),
					);
				}
				Update::History { account, revision, results }
			},
			cx,
		);
	}

	pub(super) fn poll_recovery_action(&mut self, cx: &mut Context<Self>) {
		let scope =
			self.account_profile.selected.clone().zip(self.account_profile.selected_revision);
		if self.recovery_actions.updates.is_none() && self.recovery_actions.history_scope != scope {
			self.recovery_actions.history.clear();
			self.recovery_actions.history_error = false;
			self.recovery_actions.message = None;
			self.recovery_actions.history_scope = scope.clone();
			if let Some((account, revision)) = scope {
				self.read_nudge_history(account, revision, cx);
			}
		}
		if self.recovery_actions.updates.is_none()
			&& self.recovery_actions.history_generation
				!= self.account_profile.notification_generation
			&& let Some((account, revision)) =
				self.account_profile.selected.clone().zip(self.account_profile.selected_revision)
		{
			self.read_nudge_history(account, revision, cx);
		}
		let Some(receiver) = self.recovery_actions.updates.as_ref() else {
			return;
		};
		let update = match receiver.try_recv() {
			Ok(update) => update,
			Err(mpsc::TryRecvError::Empty) => return,
			Err(mpsc::TryRecvError::Disconnected) => {
				self.recovery_actions.updates = None;
				self.recovery_actions.history_error = true;
				self.recovery_actions.message = None;
				cx.notify();
				return;
			},
		};
		self.recovery_actions.updates = None;
		match update {
			Update::Prepared { ticket, prepared, prior, acknowledged } => {
				if self.selected == super::Destination::Accounts {
					self.finish_recovery_preparation(ticket, prepared, prior, acknowledged, cx);
				} else {
					self.recovery_actions.message = None;
				}
			},
			Update::Sent { ticket, key, result, confirmed } => {
				if self.account_profile.selected.as_ref() == Some(&ticket.source.account_id)
					&& self.account_profile.selected_revision
						== Some(ticket.source.account_revision)
				{
					self.recovery_actions.message = None;
					let operation = match result {
						NudgeResult::Found(operation) => Notice::from(operation),
						_ => Notice {
							account_id: ticket.source.account_id,
							action: ticket.action,
							operation_key: key,
							outcome: confirmed.unwrap_or(Status::Uncertain),
						},
					};
					self.recovery_actions.remember(operation);
				}
			},
			Update::History { account, revision, results } => {
				if self.account_profile.selected.as_ref() == Some(&account)
					&& self.account_profile.selected_revision == Some(revision)
				{
					self.recovery_actions.history_error =
						results.iter().any(|r| matches!(r, NudgeResult::Unavailable));
					for result in results {
						if let NudgeResult::Found(operation) = result {
							self.recovery_actions.remember(operation);
						}
					}
				}
			},
		}
		cx.notify();
	}

	fn finish_recovery_preparation(
		&mut self,
		ticket: RecoveryActionTicket,
		prepared: Option<Preparation>,
		prior: Option<NudgeResult>,
		acknowledged: Option<IdempotencyKey>,
		cx: &mut Context<Self>,
	) {
		let destination = prepared.and_then(|result| {
			self.account_profile_controller.finish_recovery_action(&ticket, result)
		});
		self.recovery_actions.message = None;
		match destination {
			Some(D::OpenUrl(url)) => cx.open_url(url.as_str()),
			Some(D::ResetPicker) => {
				if let Some(account) = self.accounts.accounts.iter().find(|a| {
					a.account_id == ticket.source.account_id
						&& a.account_revision == ticket.source.account_revision
				}) {
					self.show_reset_cards(
						account.account_id.clone(),
						account.alias.as_str().to_owned(),
						cx,
					);
				}
			},
			Some(D::RequestCredits | D::RequestUsageIncrease) => {
				let prior = prior.unwrap_or(NudgeResult::Unavailable);
				if let NudgeResult::Found(operation) = &prior {
					self.recovery_actions.remember(operation.clone());
				}
				let Some(key) = nudge_policy::operation_key(
					&ticket.source,
					ticket.action,
					&prior,
					acknowledged.as_ref(),
				) else {
					self.recovery_message(
						&ticket.source,
						"Check the previous notification before sending another request.",
					);
					return;
				};
				self.send_prepared_nudge(ticket, key, cx);
			},
			None => self.recovery_message(
				&ticket.source,
				"This action is no longer available. Refresh account details.",
			),
		}
	}

	fn send_prepared_nudge(
		&mut self,
		ticket: RecoveryActionTicket,
		key: IdempotencyKey,
		cx: &mut Context<Self>,
	) {
		let Some(profile) = self.reset_cards.profile.clone() else {
			return;
		};
		self.recovery_message(&ticket.source, "Sending request…");
		self.spawn_recovery_work(
			async move {
				let client = AccountClient::new(profile);
				// Read the exact durable key after every result, including rejection or transport
				// loss.
				let sent = client
					.send_recovery_nudge(ticket.source.clone(), ticket.action, key.clone())
					.await;
				let confirmed = match sent {
					Ok(decodex_protocol::AccountCommandResponse::Applied { result, .. }) =>
						match *result {
							decodex_protocol::ResultPayload::AccountRecoveryNudge {
								status,
								..
							} => Some(status),
							_ => None,
						},
					Err(_) => Some(Status::Unavailable),
					_ => None,
				};
				let result = client
					.recovery_nudge_status(
						ticket.source.account_id.clone(),
						ticket.action,
						Some(key.clone()),
					)
					.await
					.unwrap_or(NudgeResult::Unavailable);
				Update::Sent { ticket, key, result, confirmed }
			},
			cx,
		);
	}
}
impl RecoveryActions {
	fn remember(&mut self, operation: impl Into<Notice>) {
		let operation = operation.into();
		self.history
			.retain(|old| old.action != operation.action || old.account_id != operation.account_id);
		self.history.push(operation);
		if self.history.len() > 2 {
			self.history.remove(0);
		}
	}
}

pub(super) fn buttons(
	shell: &Shell,
	source: &AccountRecoveryResult,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let (AccountRecoveryState::Current(banner) | AccountRecoveryState::Stale(banner)) =
		&source.state
	else {
		return div().into_any_element();
	};
	let enabled = matches!(source.state, AccountRecoveryState::Current(_))
		&& shell.recovery_actions.updates.is_none()
		&& shell.reset_cards.profile.is_some();
	div()
		.flex()
		.flex_wrap()
		.gap_2()
		.children(banner.actions.iter().enumerate().map(|(index, cta)| {
			let source = source.clone();
			let action = cta.action;
			account_login_button(("recovery-action", index), cta.label.as_str().to_owned(), enabled)
				.debug_selector(move || format!("recovery-action-{index}"))
				.when(enabled, |button| {
					button.on_click(cx.listener(move |shell, _, _, cx| {
						shell.start_recovery_action(&source, action, None, cx)
					}))
				})
		}))
		.into_any_element()
}

pub(super) fn status_panel(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let Some((account, revision)) =
		shell.account_profile.selected.clone().zip(shell.account_profile.selected_revision)
	else {
		return div().into_any_element();
	};
	let available = shell.recovery_actions.updates.is_none() && shell.reset_cards.profile.is_some();
	let rows = shell
		.recovery_actions
		.history
		.iter()
		.filter(|operation| operation.account_id == account)
		.enumerate()
		.map(|(index, operation)| {
			let purpose = if operation.action == A::NotifyOwner {
				"Credits request"
			} else {
				"Usage-limit request"
			};
			let label = match operation.outcome {
				Status::Sent => "Sent",
				Status::CooldownActive => "Owner was recently notified",
				Status::Unavailable => "Not sent: account unavailable",
				Status::Unsupported => "Not supported by this Codex version",
				Status::Uncertain =>
					"Delivery is unknown. A new request may send another notification.",
			};
			let mut row = div().flex().flex_col().gap_2().child(format!("{purpose}: {label}"));
			if operation.outcome == Status::Uncertain
				&& let Some(source) = shell
					.account_profile
					.recovery
					.as_ref()
					.filter(|source| source.allows_nudge(operation.action))
			{
				let source = source.clone();
				let action = operation.action;
				let previous = operation.operation_key.clone();
				row = row.child(
					account_login_button(
						("nudge-explicit-resend", index),
						"Send new request",
						available,
					)
					.when(available, |button| {
						button.on_click(cx.listener(move |shell, _, _, cx| {
							shell.start_recovery_action(&source, action, Some(previous.clone()), cx)
						}))
					}),
				);
			}
			row
		});
	div()
		.flex()
		.flex_col()
		.gap_2()
		.children(rows)
		.when(shell.recovery_actions.history_error, |panel| {
			panel.child("Notification status is unavailable. No request was resent.")
		})
		.children(
			shell
				.recovery_actions
				.message
				.clone()
				.filter(|(id, rev, _)| id == &account && rev == &revision)
				.map(|(_, _, message)| div().child(message)),
		)
		.child(
			account_login_button(
				"check-account-notification",
				"Check notification status",
				available,
			)
			.debug_selector(|| "check-account-notification".into())
			.when(available, |button| {
				button.on_click(cx.listener(move |shell, _, _, cx| {
					shell.read_nudge_history(account.clone(), revision, cx)
				}))
			}),
		)
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::{TestAppContext, VisualTestContext, px, size};
	fn fixture() -> (crate::account_profile::AccountProfileController, decodex_protocol::ServerId) {
		use decodex_protocol::{
			AccountRecoveryBanner, AccountRecoveryCta, CURRENT_VERSION, QueryResultEnvelope,
			QueryResultPayload, WireText,
		};
		let controller = crate::account_profile::AccountProfileController::production();
		let server =
			decodex_protocol::ServerId::new("20000000-0000-4000-8000-000000000001").unwrap();
		let account = EntityId::new("10000000-0000-4000-8000-000000000001").unwrap();
		controller.bind_session(1, server.clone());
		controller.select_at_revision(account.clone(), EntityRevision(1));
		let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
		let query = runtime.block_on(controller.next_dispatch(1, &server));
		let source = AccountRecoveryResult {
			account_id: account,
			account_revision: EntityRevision(1),
			observed_at_unix_micros: Some(
				i64::try_from(
					std::time::SystemTime::now()
						.duration_since(std::time::UNIX_EPOCH)
						.unwrap()
						.as_micros(),
				)
				.unwrap(),
			),
			state: AccountRecoveryState::Current(Box::new(AccountRecoveryBanner {
				banner_type: WireText::new("limit").unwrap(),
				title: WireText::new("Model limit reached").unwrap(),
				description: WireText::new("Reset at {time}. Other models remain available.")
					.unwrap(),
				reset_at: Some(1800000000),
				model_slug: None,
				blocked_model_slug: Some(WireText::new("blocked-model").unwrap()),
				fallback_model_slugs: Vec::new(),
				dismissible: true,
				actions: vec![AccountRecoveryCta {
					action: A::ResetUsage,
					label: WireText::new("Use reset credit").unwrap(),
				}],
				request_url: None,
			})),
		};
		controller.route_result(
			1,
			&server,
			&QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: server.clone(),
				query_id: query.query_id,
				payload: QueryResultPayload::AccountRecovery(source),
			},
		);
		(controller, server)
	}
	fn disconnected_profile() -> decodex_protocol::ClientProfile {
		use std::{io::Write as _, os::unix::fs::OpenOptionsExt as _};
		let root = tempfile::tempdir().unwrap();
		let mut file = std::fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.mode(0o600)
			.open(root.path().join("config.toml"))
			.unwrap();
		file.write_all(
			br#"version = 1
active_profile = "fixture"
[profiles.fixture]
kind = "remote"
host = "fixture.invalid"
port = 49152
expected_server_identity = "20000000-0000-4000-8000-000000000001"
[cache]
max_entries = 0
max_bytes = 0
max_entry_bytes = 0
"#,
		)
		.unwrap();
		// This profile enables presentation, but account clients refuse remote profiles
		// before any network access. It cannot contact a real service from this test.
		decodex_protocol::ClientProfile::load(&root.path().canonicalize().unwrap(), None).unwrap()
	}

	fn open(cx: &mut TestAppContext, stale: bool) -> (gpui::Entity<Shell>, &mut VisualTestContext) {
		let (controller, _) = fixture();
		if stale {
			controller.session_ended(1);
		}
		let (shell, visual) = cx.add_window_view(|window, cx| {
			Shell::new(window, cx, crate::client_lifecycle::ConnectionView::Stopped)
		});
		shell.update(visual, |shell, cx| {
			shell.reset_cards.profile = Some(disconnected_profile());
			shell.account_profile_controller = controller;
			shell.account_profile = shell.account_profile_controller.snapshot();
			shell.select_destination(super::super::Destination::Accounts, cx);
			let quota = |duration_minutes| decodex_protocol::AccountQuotaWindowDto {
				duration_minutes,
				observed_at_unix_micros: None,
				result: decodex_protocol::AccountQuotaStateDto::Unknown,
			};
			shell.accounts.accounts = vec![decodex_protocol::AccountDto {
				account_id: shell.account_profile.selected.clone().unwrap(),
				alias: decodex_protocol::WireText::new("Fixture account").unwrap(),
				enabled: true,
				account_revision: EntityRevision(1),
				observed_state: decodex_protocol::AccountObservedStateDto::Available,
				lifecycle_readiness: decodex_protocol::AccountLifecycleReadinessDto::Ready,
				credential_binding: None,
				unsettled_operation: None,
				five_hour_quota: quota(300),
				seven_day_quota: quota(10080),
			}];
		});
		visual.update(|window, cx| {
			window.resize(size(px(1440.), px(1000.)));
			window.draw(cx).clear();
		});
		(shell, visual)
	}
	#[gpui::test]
	fn rendered_recovery_dismissal_preserves_durable_status_access(cx: &mut TestAppContext) {
		let (shell, visual) = open(cx, false);
		let bounds = visual
			.debug_bounds("dismiss-account-notice")
			.expect("current dismissible notice renders a real button");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		assert!(shell.read_with(visual, |shell, _| shell.account_profile.recovery.is_none()));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(
			visual.debug_bounds("check-account-notification").is_some(),
			"status remains accessible after banner dismissal"
		);
	}
	#[gpui::test]
	fn rendered_current_action_starts_preparation_before_any_effect(cx: &mut TestAppContext) {
		let (shell, visual) = open(cx, false);
		let bounds = visual.debug_bounds("recovery-action-0").expect("current action renders");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		assert!(shell.read_with(visual, |shell, _| shell.recovery_actions.updates.is_some()));
	}

	#[gpui::test]
	fn rendered_stale_recovery_does_not_dispatch_or_dismiss(cx: &mut TestAppContext) {
		let (shell, visual) = open(cx, true);
		assert!(visual.debug_bounds("dismiss-account-notice").is_none());
		let bounds =
			visual.debug_bounds("recovery-action-0").expect("stale action remains visible");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		assert!(shell.read_with(visual, |shell, _| shell.recovery_actions.updates.is_none()));
		assert!(shell.read_with(visual, |shell, _| matches!(
			shell.account_profile.recovery.as_ref().map(|result| &result.state),
			Some(AccountRecoveryState::Stale(_))
		)));
	}
}
