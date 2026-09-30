//! Inline account Reset Cards. The service owns receipts and recovery.
use super::{
	ControlTooltip, Destination, Shell, account_needs_login, account_row_action,
	quota_meter::ResetFill,
};
use decodex_protocol::{
	AccountResetCardOperationResult, ClientProfile, EntityId, EntityRevision, IdempotencyKey,
	ResetCardClient, ResetCardConsumeResponse, ResetCardDescriptorDto, ResetCardInventoryResult,
	ResetCardOperationResult, ResetCardOutcome,
};
use gpui::{AnyElement, Context, div, prelude::*, px, rgb, rgba};
use std::{
	collections::HashMap,
	sync::{
		atomic::{AtomicU64, Ordering},
		mpsc::{self, Receiver},
	},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static NEXT_CONFIRMATION: AtomicU64 = AtomicU64::new(0);
const CONFIRMATION_WINDOW: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct ResetCardsPanel {
	pub(super) profile: Option<ClientProfile>,
	rows: HashMap<EntityId, CardRow>,
	confirmation: Option<Confirmation>,
	updates: Option<Receiver<(EntityId, Update)>>,
	working: Option<EntityId>,
	fills: HashMap<EntityId, ResetFill>,
}
#[derive(Default)]
struct CardRow {
	inventory: Option<ResetCardInventoryResult>,
	source_revision: Option<EntityRevision>,
	checked: Option<Instant>,
	blocked: bool,
	pending_key: Option<IdempotencyKey>,
	pending_fill: Option<ResetFill>,
	used: Option<(ResetCardDescriptorDto, Instant)>,
	used_visible: bool,
	consuming: Option<ResetCardDescriptorDto>,
	message: String,
}
struct Confirmation {
	account: EntityId,
	descriptor: ResetCardDescriptorDto,
	revision: EntityRevision,
	started: Instant,
}
impl Confirmation {
	fn matches(
		&self,
		account: &EntityId,
		descriptor: ResetCardDescriptorDto,
		revision: EntityRevision,
	) -> bool {
		&self.account == account
			&& self.descriptor == descriptor
			&& self.revision == revision
			&& self.started.elapsed() < CONFIRMATION_WINDOW
	}
}
struct Update {
	completed_reset: Option<(EntityId, IdempotencyKey)>,
	inventory: Option<ResetCardInventoryResult>,
	blocked: bool,
	pending_key: Option<IdempotencyKey>,
	message: String,
}
impl Shell {
	pub(super) fn reset_fill_for(
		&self,
		account: &decodex_protocol::AccountDto,
	) -> Option<ResetFill> {
		self.reset_cards
			.fills
			.get(&account.account_id)
			.filter(|fill| fill.revision == account.account_revision)
			.cloned()
	}

	fn tap_reset_card(
		&mut self,
		account: EntityId,
		descriptor: ResetCardDescriptorDto,
		revision: EntityRevision,
		cx: &mut Context<Self>,
	) {
		let eligible = self.accounts.can_manage && self.reset_cards.rows.get(&account).is_some_and(|row| {
            !row.blocked && self.reset_cards.working.is_none()
                && matches!(&row.inventory, Some(ResetCardInventoryResult::Available { account_revision, details_complete: true, cards, .. })
                    if *account_revision == revision && cards.iter().any(|card| card.descriptor == descriptor))
        }) && self.accounts.accounts.iter().any(|row| row.account_id == account && row.account_revision == revision && !account_needs_login(row))
            && descriptor.expires_at_unix_seconds().is_none_or(|expiry| expiry > time::OffsetDateTime::now_utc().unix_timestamp());
		if !eligible {
			return;
		}
		if !self
			.reset_cards
			.confirmation
			.as_ref()
			.is_some_and(|c| c.matches(&account, descriptor, revision))
		{
			self.reset_cards.confirmation =
				Some(Confirmation { account, descriptor, revision, started: Instant::now() });
			cx.notify();
			return;
		}
		let Some(profile) = self.reset_cards.profile.clone() else {
			return;
		};
		self.reset_cards.confirmation = None;
		let Ok(timestamp) = SystemTime::now().duration_since(UNIX_EPOCH) else {
			return;
		};
		let Ok(key) = IdempotencyKey::new(format!(
			"decodex-reset-{}-{}-{}",
			std::process::id(),
			timestamp.as_nanos(),
			NEXT_CONFIRMATION.fetch_add(1, Ordering::Relaxed)
		)) else {
			return;
		};
		let fill = self
			.accounts
			.accounts
			.iter()
			.find(|row| row.account_id == account && row.account_revision == revision)
			.map(|row| ResetFill {
				revision,
				initial: [row.five_hour_quota, row.seven_day_quota],
				started: Instant::now(),
				confirmed_at_micros: 0,
			});
		let row = self.reset_cards.rows.get_mut(&account).expect("eligible row");
		row.pending_fill = fill;
		row.pending_key = Some(key.clone());
		row.consuming = Some(descriptor);
		row.blocked = true;
		self.start_reset_card_work(account.clone(), cx, async move {
			let client = ResetCardClient::new(profile);
			match client.consume(account.clone(), descriptor, revision, key.clone()).await {
				Ok(ResetCardConsumeResponse::Rejected { .. }) => Update {
					completed_reset: None,
					inventory: None,
					blocked: true,
					pending_key: None,
					message: "Card was not used. Checking account availability…".into(),
				},
				Err(_) => Update {
					completed_reset: None,
					inventory: None,
					blocked: true,
					pending_key: Some(key),
					message: "Checking the request result. No request will be resent.".into(),
				},
				Ok(
					ResetCardConsumeResponse::Accepted { .. }
					| ResetCardConsumeResponse::PotentiallyDispatched { .. },
				) => {
					for _ in 0..20 {
						tokio::time::sleep(Duration::from_millis(500)).await;
						match client.status(key.clone()).await {
							Ok(state) if !terminal(state) => {},
							_ => break,
						}
					}
					load(&client, account, Some(key)).await
				},
			}
		});
	}

	fn start_reset_card_work(
		&mut self,
		account: EntityId,
		cx: &mut Context<Self>,
		work: impl std::future::Future<Output = Update> + Send + 'static,
	) {
		let (sender, receiver) = mpsc::channel();
		self.reset_cards.working = Some(account.clone());
		self.reset_cards.updates = Some(receiver);
		cx.background_executor()
			.spawn(async move {
				let result =
					match tokio::runtime::Builder::new_current_thread().enable_all().build() {
						Ok(runtime) => runtime.block_on(work),
						Err(_) => Update {
							completed_reset: None,
							inventory: None,
							blocked: true,
							pending_key: None,
							message: "Reset Cards are temporarily unavailable.".into(),
						},
					};
				let _ = sender.send((account, result));
			})
			.detach();
		cx.notify();
	}

	pub(super) fn poll_reset_cards(&mut self, cx: &mut Context<Self>) {
		if let Some((account, update)) =
			self.reset_cards.updates.as_ref().and_then(|rx| rx.try_recv().ok())
		{
			self.reset_cards.updates = None;
			self.reset_cards.working = None;
			if let Some(row) = self.reset_cards.rows.get_mut(&account) {
				if let Some((_, key)) = &update.completed_reset
					&& row.pending_key.as_ref() == Some(key)
					&& let Some(mut fill) = row.pending_fill.take()
				{
					fill.started = Instant::now();
					fill.confirmed_at_micros =
						(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
					self.reset_cards.fills.insert(account.clone(), fill);
					row.used = row.consuming.take().map(|card| (card, Instant::now()));
					row.used_visible = row.used.is_some();
				}
				if !update.blocked {
					row.pending_fill = None;
					row.consuming = None;
				}
				if update.inventory.is_some() {
					row.inventory = update.inventory;
				}
				row.blocked = update.blocked;
				// A failed read must not discard the identity of an uncertain request.
				if !update.blocked || update.pending_key.is_some() {
					row.pending_key = update.pending_key;
				}
				row.message = update.message;
				row.checked = Some(Instant::now());
			}
			cx.notify();
		}
		if self.reset_cards.confirmation.is_some() {
			if self.reset_cards.confirmation.as_ref().is_some_and(|c| {
				c.started.elapsed() >= CONFIRMATION_WINDOW
					|| !self
						.accounts
						.accounts
						.iter()
						.any(|a| a.account_id == c.account && a.account_revision == c.revision)
			}) {
				self.reset_cards.confirmation = None;
			}
			cx.notify();
		}
		for row in self.reset_cards.rows.values_mut() {
			if row.used_visible
				&& row.used.is_some_and(|(_, time)| time.elapsed() >= Duration::from_millis(350))
			{
				row.used_visible = false;
				cx.notify();
			}
		}
		self.reset_cards
			.rows
			.retain(|id, _| self.accounts.accounts.iter().any(|a| &a.account_id == id));
		self.reset_cards
			.fills
			.retain(|id, _| self.accounts.accounts.iter().any(|a| &a.account_id == id));
		if self.reset_cards.working.is_some() {
			return;
		}
		let visible = self.selected == Destination::Accounts
			|| (self.settings_window.is_some() && self.settings_selected == Destination::Accounts);
		let Some(profile) = self.reset_cards.profile.clone() else {
			return;
		};
		let next = self
			.accounts
			.accounts
			.iter()
			.find(|account| {
				let row = self.reset_cards.rows.get(&account.account_id);
				let pending = row.is_some_and(|row| row.pending_key.is_some());
				(pending
					|| (visible
						&& self.expanded_accounts.contains(&account.account_id)
						&& !account_needs_login(account)))
					&& row.is_none_or(|row| {
						row.checked.is_none_or(|at| {
							at.elapsed() >= Duration::from_secs(if row.blocked { 5 } else { 30 })
						}) || row.source_revision != Some(account.account_revision)
					})
			})
			.map(|a| (a.account_id.clone(), a.account_revision));
		if let Some((account, revision)) = next {
			let row = self.reset_cards.rows.entry(account.clone()).or_default();
			row.source_revision = Some(revision);
			let key = row.pending_key.clone();
			self.start_reset_card_work(account.clone(), cx, async move {
				load(&ResetCardClient::new(profile), account, key).await
			});
		}
	}
}
async fn load(
	client: &ResetCardClient,
	account: EntityId,
	known_key: Option<IdempotencyKey>,
) -> Update {
	let inventory = client.list(account.clone()).await.ok();
	let (state, key) = if let Some(key) = known_key {
		(client.status(key.clone()).await.ok(), Some(key))
	} else {
		match client.latest_operation(account.clone()).await {
			Ok(AccountResetCardOperationResult::NotFound) =>
				(Some(ResetCardOperationResult::NotFound), None),
			Ok(AccountResetCardOperationResult::Found(operation)) =>
				(Some(operation.state), Some(operation.idempotency_key)),
			_ => (None, None),
		}
	};
	let (blocked, message) = operation_presentation(state, key.is_some());
	let completed_reset = match (&state, &key) {
		(
			Some(ResetCardOperationResult::Completed { outcome: ResetCardOutcome::Reset }),
			Some(key),
		) => Some((account, key.clone())),
		_ => None,
	};
	let pending_key = if blocked { key } else { None };
	Update { inventory, blocked, pending_key, message: message.into(), completed_reset }
}
fn terminal(state: ResetCardOperationResult) -> bool {
	matches!(
		state,
		ResetCardOperationResult::Completed { .. }
			| ResetCardOperationResult::FailedBeforeEffect { .. }
	)
}
fn operation_presentation(
	state: Option<ResetCardOperationResult>,
	known_request: bool,
) -> (bool, &'static str) {
	match state {
		Some(ResetCardOperationResult::NotFound) if !known_request =>
			(false, "Click a card twice within five seconds to use it."),
		Some(ResetCardOperationResult::Completed { outcome: ResetCardOutcome::Reset }) =>
			(false, "Account limits were reset. The card was used."),
		Some(ResetCardOperationResult::Completed {
			outcome: ResetCardOutcome::AlreadyRedeemed,
		}) => (false, "The selected card was already redeemed. No other card was selected."),
		Some(ResetCardOperationResult::Completed { outcome: ResetCardOutcome::NothingToReset }) =>
			(false, "There was nothing to reset. No card was used."),
		Some(ResetCardOperationResult::Completed { outcome: ResetCardOutcome::NoCredit }) =>
			(false, "The selected card is no longer available. No other card was selected."),
		Some(ResetCardOperationResult::FailedBeforeEffect { .. }) => (
			false,
			"The request stopped before redemption. Account availability will update automatically.",
		),
		Some(ResetCardOperationResult::Prepared) =>
			(true, "Your confirmed request is queued. Checking its result…"),
		Some(ResetCardOperationResult::EffectAmbiguous) =>
			(true, "The result is not confirmed. Checking status without another redemption."),
		_ => (
			true,
			"The previous request could not be verified. Checking status; redemption stays blocked.",
		),
	}
}

pub(super) fn row(
	shell: &Shell,
	account: &decodex_protocol::AccountDto,
	cx: &mut Context<Shell>,
) -> Option<AnyElement> {
	let state = shell.reset_cards.rows.get(&account.account_id)?;
	let Some(ResetCardInventoryResult::Available {
		cards, details_complete, account_revision, ..
	}) = &state.inventory
	else {
		return None;
	};
	let mut descriptors: Vec<_> = cards
		.iter()
		.map(|card| card.descriptor)
		.filter(|card| state.used_visible || !state.used.is_some_and(|(used, _)| used == *card))
		.collect();
	if let Some((used, _)) = state.used
		&& state.used_visible
		&& !descriptors.contains(&used)
	{
		descriptors.push(used);
	}
	if descriptors.is_empty() {
		return None;
	}
	let mut strip = div()
		.id(gpui::SharedString::from(format!("reset-cards-{}", account.account_id.as_str())))
		.w_full()
		.flex()
		.flex_wrap()
		.items_center()
		.gap_1()
		.px(px(14.))
		.pb(px(4.))
		.child(super::workspace_symbols::icon(super::workspace_symbols::Symbol::ResetCards));
	for (index, descriptor) in descriptors.into_iter().enumerate() {
		let account_id = account.account_id.clone();
		let revision = *account_revision;
		let armed = shell
			.reset_cards
			.confirmation
			.as_ref()
			.filter(|c| c.matches(&account_id, descriptor, revision));
		let used = state.used.is_some_and(|(card, _)| card == descriptor);
		let pending = state.consuming == Some(descriptor) && state.pending_key.is_some();
		let title = if used {
			"✓ Used".into()
		} else if pending {
			"Using…".into()
		} else if let Some(c) = armed {
			format!("Confirm · {}s", 5u64.saturating_sub(c.started.elapsed().as_secs()))
		} else {
			descriptor.expires_at_unix_seconds().map(date).unwrap_or_else(|| "No expiry".into())
		};
		let enabled = shell.accounts.can_manage
			&& !account_needs_login(account)
			&& !state.blocked
			&& shell.reset_cards.working.is_none()
			&& *details_complete
			&& account.account_revision == revision
			&& !used
			&& descriptor
				.expires_at_unix_seconds()
				.is_none_or(|expiry| expiry > time::OffsetDateTime::now_utc().unix_timestamp());
		let tip = if state.blocked {
			state.message.clone()
		} else {
			format!("Reset Card · {}. Click twice within five seconds to use it.", title)
		};
		strip = strip.child(
			account_row_action("reset-card", index, "Use Reset Card", "", enabled)
				.border_1()
				.border_color(rgba(0xffffff26))
				.id(gpui::SharedString::from(format!("reset-card-{}-{index}", account_id.as_str())))
				.debug_selector({
					let id = account_id.clone();
					move || format!("reset-card-{}-{index}", id.as_str())
				})
				.text_color(rgb(if armed.is_some() {
					super::WB_AMBER
				} else {
					super::WB_TEXT_MUTED
				}))
				.tooltip(move |_, cx| cx.new(|_| ControlTooltip(tip.clone())).into())
				.child(title)
				.when(enabled, |button| {
					button.on_click(cx.listener(move |shell, _, _, cx| {
						cx.stop_propagation();
						shell.tap_reset_card(account_id.clone(), descriptor, revision, cx);
					}))
				}),
		);
	}
	Some(strip.into_any_element())
}
fn date(seconds: i64) -> String {
	super::quota_meter::local_date_time(seconds).unwrap_or_else(|| "Unknown expiry".into())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn missing_or_ambiguous_receipts_block_new_redemption() {
		assert!(operation_presentation(None, false).0);
		assert!(operation_presentation(Some(ResetCardOperationResult::EffectAmbiguous), true).0);
		assert!(operation_presentation(Some(ResetCardOperationResult::NotFound), true).0);
		assert!(!operation_presentation(Some(ResetCardOperationResult::NotFound), false).0);
	}
	#[test]
	fn confirmation_is_bound_to_account_card_revision_and_deadline() {
		let id = EntityId::new("21000000-0000-4000-8000-000000000099").unwrap();
		let other = EntityId::new("21000000-0000-4000-8000-000000000098").unwrap();
		let card = ResetCardDescriptorDto::new(1, 4102444800).unwrap();
		let mut c = Confirmation {
			account: id.clone(),
			descriptor: card,
			revision: EntityRevision(1),
			started: Instant::now(),
		};
		assert!(c.matches(&id, card, EntityRevision(1)));
		assert!(!c.matches(&other, card, EntityRevision(1)));
		assert!(!c.matches(&id, card, EntityRevision(2)));
		c.started -= Duration::from_secs(6);
		assert!(!c.matches(&id, card, EntityRevision(1)));
	}
	#[test]
	fn completed_receipt_reports_the_exact_outcome() {
		let state =
			ResetCardOperationResult::Completed { outcome: ResetCardOutcome::NothingToReset };
		assert!(terminal(state));
		let (blocked, message) = operation_presentation(Some(state), true);
		assert!(!blocked);
		assert!(message.contains("No card was used"));
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl ResetCardsPanel {
	pub(super) fn seed_visual(&mut self, accounts: &[decodex_protocol::AccountDto]) {
		use decodex_protocol::ResetCardObservationDto;
		for account in accounts.iter().take(2) {
			self.rows.insert(
				account.account_id.clone(),
				CardRow {
					inventory: Some(ResetCardInventoryResult::Available {
						account_id: account.account_id.clone(),
						account_revision: account.account_revision,
						reported_available_count: Some(2),
						details_complete: true,
						cards: vec![
							ResetCardObservationDto {
								descriptor: ResetCardDescriptorDto::new(1, 4102444800).unwrap(),
							},
							ResetCardObservationDto {
								descriptor: ResetCardDescriptorDto::new(2, 4105123200).unwrap(),
							},
						],
						five_hour_quota: account.five_hour_quota,
						seven_day_quota: account.seven_day_quota,
					}),
					source_revision: Some(account.account_revision),
					checked: Some(Instant::now()),
					..Default::default()
				},
			);
		}
	}
}

#[cfg(test)]
mod render_tests {
	use super::*;
	use crate::client_lifecycle::ConnectionView;
	use gpui::{Modifiers, TestAppContext, size};
	#[gpui::test]
	fn multiple_account_disclosures_keep_cards_and_activity_independent(cx: &mut TestAppContext) {
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
		let (first, second) = shell.update(visual, |shell, cx| {
			shell.visual_accounts_and_health();
			shell.selected = Destination::Accounts;
			let first = shell.accounts.accounts[0].account_id.clone();
			let second = shell.accounts.accounts[1].account_id.clone();
			shell.toggle_account_activity(first.clone(), cx);
			shell.toggle_account_activity(second.clone(), cx);
			(first, second)
		});
		visual.update(|window, cx| {
			window.resize(size(px(1440.), px(1000.)));
			window.draw(cx).clear();
		});
		std::thread::sleep(Duration::from_millis(250));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		for selector in [
			"account-activity-70000000-0000-4000-8000-000000000001",
			"account-activity-70000000-0000-4000-8000-000000000002",
			"reset-card-70000000-0000-4000-8000-000000000001-0",
			"reset-card-70000000-0000-4000-8000-000000000002-0",
		] {
			assert!(visual.debug_bounds(selector).is_some(), "{selector}");
		}
		let chip =
			visual.debug_bounds("reset-card-70000000-0000-4000-8000-000000000001-0").unwrap();
		visual.simulate_click(chip.center(), Modifiers::default());
		shell.read_with(visual, |s, _| {
			assert_eq!(s.reset_cards.confirmation.as_ref().map(|c| &c.account), Some(&first));
			assert!(s.reset_cards.updates.is_none(), "First click must not send a request");
		});
		shell.update(visual, |s, cx| s.toggle_account_activity(first.clone(), cx));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		std::thread::sleep(Duration::from_millis(250));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("reset-card-70000000-0000-4000-8000-000000000001-0").is_none());
		assert!(visual.debug_bounds("reset-card-70000000-0000-4000-8000-000000000002-0").is_some());
		shell.update(visual, |s, cx| {
			s.reset_cards.confirmation = None;
			s.reset_cards.rows.get_mut(&second).unwrap().blocked = true;
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let blocked =
			visual.debug_bounds("reset-card-70000000-0000-4000-8000-000000000002-0").unwrap();
		visual.simulate_click(blocked.center(), Modifiers::default());
		shell.read_with(visual, |s, _| {
			assert!(s.reset_cards.confirmation.is_none());
			assert!(
				s.reset_cards.updates.is_none(),
				"Uncertain status must not dispatch a redemption"
			);
		});
		assert!(visual.debug_bounds("accounts-refresh").is_none());
		assert!(visual.debug_bounds("account-reset-cards-0").is_none());
	}
}
